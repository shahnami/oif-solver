//! EIP-7683 specific settlement encoding

use super::SettlementEncoder;
use crate::types::{Input as EIP7683Input, Output as EIP7683Output, StandardOrder};
use async_trait::async_trait;
use ethers::abi::{Function, Param, ParamType, Token};
use solver_types::{
	chains::{ChainId, Transaction},
	common::{Address, Bytes32, U256},
	errors::{Result, SolverError},
	orders::{Order, OrderStandard},
};
use std::collections::HashMap;
use tracing::debug;

/// Configuration for EIP-7683 settlement encoding
#[derive(Debug, Clone)]
pub struct EIP7683EncoderConfig {
	/// Settler addresses per chain
	pub settler_addresses: HashMap<ChainId, Address>,
	/// Optional oracle address
	pub oracle_address: Option<Address>,
	/// Optional solver address
	pub solver_address: Option<Address>,
	/// Gas limit for settlements
	pub gas_limit: Option<u64>,
}

/// EIP-7683 specific settlement encoder
pub struct EIP7683SettlementEncoder {
	config: EIP7683EncoderConfig,
}

impl EIP7683SettlementEncoder {
	pub fn new(config: EIP7683EncoderConfig) -> Self {
		Self { config }
	}

	/// Get settler address for a chain
	fn get_settler_address(&self, chain_id: ChainId) -> Result<Address> {
		self.config
			.settler_addresses
			.get(&chain_id)
			.copied()
			.ok_or_else(|| {
				SolverError::Settlement(format!(
					"No settler address configured for chain {}",
					chain_id
				))
			})
	}

	/// Build StandardOrder struct from Order trait object
	fn build_standard_order(&self, order: &dyn Order) -> Result<StandardOrder> {
		let origin_chain = order.origin_chain();

		// Extract inputs and outputs using the Order methods
		let inputs = order.inputs()?;
		let outputs = order.outputs()?;

		// Convert to EIP7683 format
		let eip7683_inputs = inputs
			.into_iter()
			.map(|input| EIP7683Input {
				token: U256::from_big_endian(input.token.as_bytes()), // Token is represented as U256 instead of bytes32
				amount: input.amount,
			})
			.collect();

		let eip7683_outputs = outputs
			.into_iter()
			.map(|output| EIP7683Output {
				// For outputs, oracle should be zero (not the local oracle)
				oracle: Bytes32::zero(),
				settler: self
					.get_settler_address(output.chain_id)
					.map(StandardOrder::address_to_bytes32)
					.unwrap_or(Bytes32::zero()),
				chain_id: U256::from(output.chain_id.0),
				token: StandardOrder::address_to_bytes32(output.token),
				amount: output.amount,
				recipient: StandardOrder::address_to_bytes32(output.recipient),
				call: vec![],
				context: vec![],
			})
			.collect();

		Ok(StandardOrder {
			user: order.user(),
			nonce: self.extract_nonce(order)?,
			origin_chain_id: U256::from(origin_chain.0),
			expires: self.extract_expires(order)?,
			fill_deadline: self.extract_fill_deadline(order)?,
			local_oracle: self.config.oracle_address.unwrap_or(Address::zero()),
			inputs: eip7683_inputs,
			outputs: eip7683_outputs,
		})
	}

	/// Extract nonce from order ID
	fn extract_nonce(&self, _order: &dyn Order) -> Result<U256> {
		// For OnchainCrossChainOrder (open() call), nonce is always 0
		// For GaslessCrossChainOrder (openFor() call), nonce comes from the order
		// Since we're dealing with open() calls, use nonce 0
		Ok(U256::zero())
	}

	/// Extract expiry timestamp
	fn extract_expires(&self, order: &dyn Order) -> Result<u32> {
		// For EIP-7683, expires comes from the MandateERC7683.expiry field
		// which is the same as the fillDeadline in our case
		let expires_at = order.expires_at();
		if expires_at > u32::MAX as u64 {
			return Err(SolverError::Settlement(
				"Expiry timestamp too large for u32".to_string(),
			));
		}
		Ok(expires_at as u32)
	}

	/// Extract fill deadline
	fn extract_fill_deadline(&self, order: &dyn Order) -> Result<u32> {
		// For now, use expires_at as fill_deadline
		self.extract_expires(order)
	}

	/// Get timestamps array for settlement
	fn get_settlement_timestamps(
		&self,
		_order: &dyn Order,
		attestation: &crate::types::Attestation,
	) -> Result<Vec<u32>> {
		// Use the attestation timestamp (when the order was actually filled)
		let timestamp = attestation.timestamp;
		if timestamp > u32::MAX as u64 {
			return Err(SolverError::Settlement(
				"Attestation timestamp too large for u32".to_string(),
			));
		}
		Ok(vec![timestamp as u32])
	}

	/// Convert StandardOrder struct to ABI Token for encoding
	fn standard_order_to_token(&self, standard_order: &StandardOrder) -> Result<Token> {
		// Convert inputs to Token::Array of FixedArray[2]
		let inputs_token = Token::Array(
			standard_order
				.inputs
				.iter()
				.map(|input| {
					Token::FixedArray(vec![Token::Uint(input.token), Token::Uint(input.amount)])
				})
				.collect(),
		);

		// Convert outputs to Token::Array
		let outputs_token = Token::Array(
			standard_order
				.outputs
				.iter()
				.map(|output| {
					Token::Tuple(vec![
						Token::FixedBytes(output.oracle.0.to_vec()),
						Token::FixedBytes(output.settler.0.to_vec()),
						Token::Uint(output.chain_id),
						Token::FixedBytes(output.token.0.to_vec()),
						Token::Uint(output.amount),
						Token::FixedBytes(output.recipient.0.to_vec()),
						Token::Bytes(output.call.clone()),
						Token::Bytes(output.context.clone()),
					])
				})
				.collect(),
		);

		Ok(Token::Tuple(vec![
			Token::Address(ethers::types::Address::from_slice(
				standard_order.user.as_bytes(),
			)),
			Token::Uint(standard_order.nonce),
			Token::Uint(standard_order.origin_chain_id),
			Token::Uint(ethers::types::U256::from(standard_order.expires)),
			Token::Uint(ethers::types::U256::from(standard_order.fill_deadline)),
			Token::Address(ethers::types::Address::from_slice(
				standard_order.local_oracle.as_bytes(),
			)),
			inputs_token,
			outputs_token,
		]))
	}
}

#[async_trait]
impl SettlementEncoder for EIP7683SettlementEncoder {
	fn name(&self) -> &str {
		"EIP7683"
	}

	fn supports(&self, order: &dyn Order) -> bool {
		matches!(order.standard(), OrderStandard::EIP7683)
	}

	async fn encode_claim_transaction(
		&self,
		order: &dyn Order,
		settler_address: Address,
		attestation: &crate::types::Attestation,
	) -> Result<Transaction> {
		debug!("Encoding EIP-7683 settlement for order {}", order.id());

		// Verify this encoder supports the order
		if !self.supports(order) {
			return Err(SolverError::Settlement(format!(
				"EIP7683 encoder does not support order standard: {:?}",
				order.standard()
			)));
		}

		#[allow(deprecated)]
		// Define the function using ethers ABI
		let function = Function {
			name: "finaliseSelf".to_string(),
			inputs: vec![
				Param {
					name: "order".to_string(),
					kind: ParamType::Tuple(vec![
						ParamType::Address,   // user
						ParamType::Uint(256), // nonce
						ParamType::Uint(256), // originChainId
						ParamType::Uint(32),  // expires
						ParamType::Uint(32),  // fillDeadline
						ParamType::Address,   // localOracle
						ParamType::Array(Box::new(
							// inputs
							ParamType::FixedArray(Box::new(ParamType::Uint(256)), 2),
						)),
						ParamType::Array(Box::new(
							// outputs
							ParamType::Tuple(vec![
								ParamType::FixedBytes(32), // oracle
								ParamType::FixedBytes(32), // settler
								ParamType::Uint(256),      // chainId
								ParamType::FixedBytes(32), // token
								ParamType::Uint(256),      // amount
								ParamType::FixedBytes(32), // recipient
								ParamType::Bytes,          // call
								ParamType::Bytes,          // context
							]),
						)),
					]),
					internal_type: None,
				},
				Param {
					name: "timestamps".to_string(),
					kind: ParamType::Array(Box::new(ParamType::Uint(32))),
					internal_type: None,
				},
				Param {
					name: "solver".to_string(),
					kind: ParamType::FixedBytes(32),
					internal_type: None,
				},
			],
			outputs: vec![],
			constant: Some(false),
			state_mutability: ethers::abi::StateMutability::NonPayable,
		};

		// Build parameters
		let standard_order = self.build_standard_order(order)?;
		let timestamps = self.get_settlement_timestamps(order, attestation)?;
		let solver_address = self.config.solver_address.unwrap_or(Address::zero());
		let solver_bytes32 = StandardOrder::address_to_bytes32(solver_address);

		// Create tokens for encoding
		let standard_order_token = self.standard_order_to_token(&standard_order)?;
		let timestamps_token = Token::Array(
			timestamps
				.into_iter()
				.map(|t| Token::Uint(ethers::types::U256::from(t)))
				.collect(),
		);
		let solver_token = Token::FixedBytes(solver_bytes32.0.to_vec());

		// Encode the function call
		let tokens = vec![standard_order_token, timestamps_token, solver_token];
		let calldata = function.encode_input(&tokens).map_err(|e| {
			SolverError::Settlement(format!("Failed to encode finaliseSelf: {}", e))
		})?;

		Ok(Transaction {
			to: settler_address,
			value: U256::zero(),
			data: calldata,
			gas_limit: Some(U256::from(self.estimated_gas_limit())),
			gas_price: None,
			nonce: None,
		})
	}

	fn estimated_gas_limit(&self) -> u64 {
		self.config.gas_limit.unwrap_or(300_000)
	}
}
