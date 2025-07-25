//! Order processing implementations for the solver service.
//!
//! This module provides concrete implementations of the OrderInterface trait
//! for EIP-7683 cross-chain orders, including transaction generation for
//! filling and claiming orders.

use alloy::dyn_abi::DynSolValue;
use alloy::primitives::{keccak256, FixedBytes, U256};
use alloy::{
	sol,
	sol_types::{SolCall, SolValue},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solver_order::{ExecutionStrategy, OrderError, OrderInterface};
use solver_types::{
	Address, ConfigSchema, ExecutionContext, ExecutionDecision, ExecutionParams, Field, FieldType,
	FillProof, Intent, Order, Schema, Transaction,
};

// Solidity type definitions for EIP-7683 contract interactions.
sol! {
	/// MandateOutput structure used in fill operations.
	struct MandateOutput {
		bytes32 oracle;
		bytes32 settler;
		uint256 chainId;
		bytes32 token;
		uint256 amount;
		bytes32 recipient;
		bytes call;
		bytes context;
	}

	/// IDestinationSettler interface for filling orders.
	interface IDestinationSettler {
		function fill(bytes32 orderId, bytes originData, bytes fillerData) external;
	}

	/// Function signature for finaliseSelf on InputSettler.
	function finaliseSelf(
		(address,uint256,uint256,uint32,uint32,address,uint256[2][],(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes)[]) order,
		uint32[] timestamps,
		bytes32 solver
	) external;
}

/// EIP-7683 specific order data structure.
///
/// Contains all the necessary information for processing a cross-chain order
/// according to the EIP-7683 standard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip7683OrderData {
	pub user: String,
	pub nonce: u64,
	pub origin_chain_id: u64,
	pub destination_chain_id: u64,
	pub expires: u32, // Changed from open_deadline to match contract
	pub fill_deadline: u32,
	pub local_oracle: String,   // Added oracle address
	pub inputs: Vec<[U256; 2]>, // Added inputs array [token, amount]
	pub order_id: [u8; 32],
	pub settle_gas_limit: u64,
	pub fill_gas_limit: u64,
	pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
	pub token: String,
	pub amount: U256,
	pub recipient: String,
	pub chain_id: u64,
}

/// EIP-7683 order implementation.
///
/// Handles validation and transaction generation for EIP-7683 cross-chain orders.
/// Manages interactions with both input and output settler contracts.
pub struct Eip7683OrderImpl {
	/// Address of the output settler contract on destination chains.
	output_settler_address: Address,
	/// Address of the input settler contract on origin chains.
	input_settler_address: Address,
	/// Address of the solver for claiming rewards.
	solver_address: Address,
}

impl Eip7683OrderImpl {
	/// Creates a new EIP-7683 order implementation.
	pub fn new(output_settler: String, input_settler: String, solver: String) -> Self {
		Self {
			output_settler_address: Address(
				hex::decode(output_settler.trim_start_matches("0x"))
					.expect("Invalid output settler address"),
			),
			input_settler_address: Address(
				hex::decode(input_settler.trim_start_matches("0x"))
					.expect("Invalid input settler address"),
			),
			solver_address: Address(
				hex::decode(solver.trim_start_matches("0x")).expect("Invalid solver address"),
			),
		}
	}
}

/// Configuration schema for EIP-7683 order implementation.
pub struct Eip7683OrderSchema;

impl ConfigSchema for Eip7683OrderSchema {
	fn validate(&self, config: &toml::Value) -> Result<(), solver_types::ValidationError> {
		let schema = Schema::new(
			// Required fields
			vec![
				Field::new("output_settler_address", FieldType::String).with_validator(|value| {
					let addr = value.as_str().unwrap();
					if addr.len() != 42 || !addr.starts_with("0x") {
						return Err(
							"output_settler_address must be a valid Ethereum address".to_string()
						);
					}
					Ok(())
				}),
				Field::new("input_settler_address", FieldType::String).with_validator(|value| {
					let addr = value.as_str().unwrap();
					if addr.len() != 42 || !addr.starts_with("0x") {
						return Err(
							"input_settler_address must be a valid Ethereum address".to_string()
						);
					}
					Ok(())
				}),
				Field::new("solver_address", FieldType::String).with_validator(|value| {
					let addr = value.as_str().unwrap();
					if addr.len() != 42 || !addr.starts_with("0x") {
						return Err("solver_address must be a valid Ethereum address".to_string());
					}
					Ok(())
				}),
			],
			// Optional fields
			vec![],
		);

		schema.validate(config)
	}
}

#[async_trait]
impl OrderInterface for Eip7683OrderImpl {
	fn config_schema(&self) -> Box<dyn ConfigSchema> {
		Box::new(Eip7683OrderSchema)
	}

	/// Validates an EIP-7683 intent and converts it to an order.
	async fn validate_intent(&self, intent: &Intent) -> Result<Order, OrderError> {
		if intent.standard != "eip7683" {
			return Err(OrderError::ValidationFailed(
				"Not an EIP-7683 order".to_string(),
			));
		}

		// Parse order data
		let order_data: Eip7683OrderData =
			serde_json::from_value(intent.data.clone()).map_err(|e| {
				OrderError::ValidationFailed(format!("Failed to parse order data: {}", e))
			})?;

		// Validate deadlines
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs() as u32;

		if now > order_data.expires {
			return Err(OrderError::ValidationFailed("Order expired".to_string()));
		}

		// Create order
		Ok(Order {
			id: intent.id.clone(),
			standard: intent.standard.clone(),
			created_at: intent.metadata.discovered_at,
			data: serde_json::to_value(&order_data)
				.map_err(|e| OrderError::ValidationFailed(format!("Failed to serialize: {}", e)))?,
		})
	}

	/// Generates a transaction to fill an EIP-7683 order on the destination chain.
	async fn generate_fill_transaction(
		&self,
		order: &Order,
		_params: &ExecutionParams,
	) -> Result<Transaction, OrderError> {
		let order_data: Eip7683OrderData =
			serde_json::from_value(order.data.clone()).map_err(|e| {
				OrderError::ValidationFailed(format!("Failed to parse order data: {}", e))
			})?;

		// Check if this is a same-chain order
		if order_data.origin_chain_id == order_data.destination_chain_id {
			return Err(OrderError::ValidationFailed(
				"Same-chain orders are not supported".to_string(),
			));
		}

		// Get the output for the destination chain
		let output = order_data
			.outputs
			.iter()
			.find(|o| o.chain_id == order_data.destination_chain_id)
			.ok_or_else(|| {
				OrderError::ValidationFailed("No output found for destination chain".to_string())
			})?;

		// Create the MandateOutput struct for the fill operation
		let mandate_output = MandateOutput {
			oracle: FixedBytes::<32>::from([0u8; 32]), // No oracle for direct fills
			settler: {
				let mut bytes32 = [0u8; 32];
				bytes32[12..32].copy_from_slice(&self.output_settler_address.0);
				FixedBytes::<32>::from(bytes32)
			},
			chainId: U256::from(output.chain_id),
			token: {
				let token_hex = output.token.trim_start_matches("0x");
				let token_bytes = hex::decode(token_hex).map_err(|e| {
					OrderError::ValidationFailed(format!("Invalid token address: {}", e))
				})?;
				let mut bytes32 = [0u8; 32];
				bytes32[12..32].copy_from_slice(&token_bytes);
				FixedBytes::<32>::from(bytes32)
			},
			amount: output.amount,
			recipient: {
				let recipient_hex = output.recipient.trim_start_matches("0x");
				let recipient_bytes = hex::decode(recipient_hex).map_err(|e| {
					OrderError::ValidationFailed(format!("Invalid recipient address: {}", e))
				})?;
				let mut bytes32 = [0u8; 32];
				bytes32[12..32].copy_from_slice(&recipient_bytes);
				FixedBytes::<32>::from(bytes32)
			},
			call: vec![].into(),    // Empty for direct transfers
			context: vec![].into(), // Empty context
		};

		// Encode fill data
		let fill_data = IDestinationSettler::fillCall {
			orderId: FixedBytes::<32>::from(order_data.order_id),
			originData: mandate_output.abi_encode().into(),
			fillerData: {
				// FillerData should contain the solver address as bytes32
				let mut solver_bytes32 = [0u8; 32];
				solver_bytes32[12..32].copy_from_slice(&self.solver_address.0);
				solver_bytes32.to_vec().into()
			},
		}
		.abi_encode();

		Ok(Transaction {
			to: Some(self.output_settler_address.clone()),
			data: fill_data,
			value: U256::ZERO,
			chain_id: order_data.destination_chain_id,
			nonce: None,
			gas_limit: Some(order_data.fill_gas_limit),
			gas_price: None,
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
		})
	}

	/// Generates a transaction to claim rewards for a filled order on the origin chain.
	async fn generate_claim_transaction(
		&self,
		order: &Order,
		fill_proof: &FillProof,
	) -> Result<Transaction, OrderError> {
		let order_data: Eip7683OrderData =
			serde_json::from_value(order.data.clone()).map_err(|e| {
				OrderError::ValidationFailed(format!("Failed to parse order data: {}", e))
			})?;

		// Check if this is a same-chain order
		if order_data.origin_chain_id == order_data.destination_chain_id {
			return Err(OrderError::ValidationFailed(
				"Same-chain orders are not supported".to_string(),
			));
		}

		// Define the finaliseSelf function signature
		let function_signature = "finaliseSelf((address,uint256,uint256,uint32,uint32,address,uint256[2][],(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes)[]),uint32[],bytes32)";
		let selector = &keccak256(function_signature.as_bytes())[..4];

		// Parse addresses
		let user_hex = order_data.user.trim_start_matches("0x");
		let user_bytes = hex::decode(user_hex)
			.map_err(|e| OrderError::ValidationFailed(format!("Invalid user address: {}", e)))?;
		let mut user_address = [0u8; 20];
		user_address.copy_from_slice(&user_bytes);

		// Create inputs array from order data
		let inputs_array: Vec<DynSolValue> = order_data
			.inputs
			.iter()
			.map(|input| {
				DynSolValue::FixedArray(vec![
					DynSolValue::Uint(input[0], 256), // token as U256
					DynSolValue::Uint(input[1], 256), // amount
				])
			})
			.collect();

		// Create outputs array (MandateOutput structs)
		let outputs_array: Vec<DynSolValue> = order_data
			.outputs
			.iter()
			.map(|output| {
				// Convert addresses to bytes32
				let oracle_bytes32 = [0u8; 32]; // No oracle

				let mut settler_bytes32 = [0u8; 32];
				if output.chain_id == order_data.origin_chain_id {
					// Use input settler for origin chain
					settler_bytes32[12..32].copy_from_slice(&self.input_settler_address.0);
				} else {
					// Use output settler for other chains
					settler_bytes32[12..32].copy_from_slice(&self.output_settler_address.0);
				}

				let mut token_bytes32 = [0u8; 32];
				let token_hex = output.token.trim_start_matches("0x");
				let token_bytes = hex::decode(token_hex).unwrap_or_else(|_| vec![0; 20]);
				token_bytes32[12..32].copy_from_slice(&token_bytes);

				let mut recipient_bytes32 = [0u8; 32];
				let recipient_hex = output.recipient.trim_start_matches("0x");
				let recipient_bytes = hex::decode(recipient_hex).unwrap_or_else(|_| vec![0; 20]);
				recipient_bytes32[12..32].copy_from_slice(&recipient_bytes);

				DynSolValue::Tuple(vec![
					DynSolValue::FixedBytes(FixedBytes::<32>::from(oracle_bytes32), 32),
					DynSolValue::FixedBytes(FixedBytes::<32>::from(settler_bytes32), 32),
					DynSolValue::Uint(U256::from(output.chain_id), 256),
					DynSolValue::FixedBytes(FixedBytes::<32>::from(token_bytes32), 32),
					DynSolValue::Uint(output.amount, 256),
					DynSolValue::FixedBytes(FixedBytes::<32>::from(recipient_bytes32), 32),
					DynSolValue::Bytes(vec![]), // call
					DynSolValue::Bytes(vec![]), // context
				])
			})
			.collect();

		// Parse oracle address
		let oracle_hex = fill_proof.oracle_address.trim_start_matches("0x");
		let oracle_bytes = hex::decode(oracle_hex)
			.map_err(|e| OrderError::ValidationFailed(format!("Invalid oracle address: {}", e)))?;
		let mut oracle_address = [0u8; 20];
		oracle_address.copy_from_slice(&oracle_bytes);

		// Build the order struct
		let order_struct = DynSolValue::Tuple(vec![
			DynSolValue::Address(alloy::primitives::Address::from_slice(&user_address)),
			DynSolValue::Uint(U256::from(order_data.nonce), 256),
			DynSolValue::Uint(U256::from(order_data.origin_chain_id), 256),
			DynSolValue::Uint(U256::from(order_data.expires as u32), 256),
			DynSolValue::Uint(U256::from(order_data.fill_deadline as u32), 256),
			DynSolValue::Address(alloy::primitives::Address::from_slice(&oracle_address)),
			DynSolValue::Array(inputs_array),
			DynSolValue::Array(outputs_array),
		]);

		// Create timestamps array - use timestamp from fill proof
		let fill_timestamp = fill_proof.filled_timestamp as u32;
		let timestamps =
			DynSolValue::Array(vec![DynSolValue::Uint(U256::from(fill_timestamp), 256)]);

		// Create solver bytes32
		let mut solver_bytes32 = [0u8; 32];
		solver_bytes32[12..32].copy_from_slice(&self.solver_address.0);

		let solver_token = DynSolValue::FixedBytes(FixedBytes::<32>::from(solver_bytes32), 32);

		// Encode parameters
		let params = vec![order_struct, timestamps, solver_token];
		let params_tuple = DynSolValue::Tuple(params);
		let encoded_params = params_tuple.abi_encode_params();

		// Build call data
		let mut call_data = Vec::new();
		call_data.extend_from_slice(selector);
		call_data.extend_from_slice(&encoded_params);

		Ok(Transaction {
			to: Some(self.input_settler_address.clone()),
			data: call_data,
			value: U256::ZERO,
			chain_id: order_data.origin_chain_id,
			nonce: None,
			gas_limit: Some(order_data.settle_gas_limit),
			gas_price: None,
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
		})
	}
}

/// Simple execution strategy that considers gas price limits.
///
/// This strategy executes orders when gas prices are below a configured
/// maximum, deferring execution when prices are too high.
pub struct SimpleStrategy {
	/// Maximum gas price the solver is willing to pay.
	max_gas_price: U256,
}

impl SimpleStrategy {
	/// Creates a new SimpleStrategy with the specified maximum gas price in gwei.
	pub fn new(max_gas_price_gwei: u64) -> Self {
		Self {
			max_gas_price: U256::from(max_gas_price_gwei) * U256::from(10u64.pow(9)),
		}
	}
}

/// Configuration schema for SimpleStrategy.
pub struct SimpleStrategySchema;

impl ConfigSchema for SimpleStrategySchema {
	fn validate(&self, config: &toml::Value) -> Result<(), solver_types::ValidationError> {
		let schema = Schema::new(
			// Required fields
			vec![],
			// Optional fields
			vec![Field::new(
				"max_gas_price_gwei",
				FieldType::Integer {
					min: Some(1),
					max: None,
				},
			)],
		);

		schema.validate(config)
	}
}

#[async_trait]
impl ExecutionStrategy for SimpleStrategy {
	fn config_schema(&self) -> Box<dyn ConfigSchema> {
		Box::new(SimpleStrategySchema)
	}

	async fn should_execute(
		&self,
		_order: &Order,
		context: &ExecutionContext,
	) -> ExecutionDecision {
		if context.gas_price > self.max_gas_price {
			return ExecutionDecision::Defer(std::time::Duration::from_secs(60));
		}

		ExecutionDecision::Execute(ExecutionParams {
			gas_price: context.gas_price,
			priority_fee: Some(U256::from(2) * U256::from(10u64.pow(9))), // 2 gwei priority
		})
	}
}

/// Factory function to create an EIP-7683 order implementation from configuration.
///
/// Required configuration parameters:
/// - `output_settler_address`: Address of the output settler contract
/// - `input_settler_address`: Address of the input settler contract
/// - `solver_address`: Address of the solver for claiming rewards
pub fn create_order_impl(config: &toml::Value) -> Box<dyn OrderInterface> {
	let output_settler = config
		.get("output_settler_address")
		.and_then(|v| v.as_str())
		.expect("output_settler_address is required");

	let input_settler = config
		.get("input_settler_address")
		.and_then(|v| v.as_str())
		.expect("input_settler_address is required");

	let solver_address = config
		.get("solver_address")
		.and_then(|v| v.as_str())
		.expect("solver_address is required");

	Box::new(Eip7683OrderImpl::new(
		output_settler.to_string(),
		input_settler.to_string(),
		solver_address.to_string(),
	))
}

/// Factory function to create an execution strategy from configuration.
///
/// Configuration parameters:
/// - `max_gas_price_gwei`: Maximum gas price in gwei (default: 100)
pub fn create_strategy(config: &toml::Value) -> Box<dyn ExecutionStrategy> {
	let max_gas_price = config
		.get("max_gas_price_gwei")
		.and_then(|v| v.as_integer())
		.unwrap_or(100) as u64;

	Box::new(SimpleStrategy::new(max_gas_price))
}
