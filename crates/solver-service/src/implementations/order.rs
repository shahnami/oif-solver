use alloy::primitives::U256;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solver_order::{ExecutionStrategy, OrderError, OrderInterface};
use solver_types::{
	Address, ExecutionContext, ExecutionDecision, ExecutionParams, FillProof, Intent, Order,
	Transaction,
};

/// EIP-7683 specific order data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip7683OrderData {
	pub user: String,
	pub origin_chain_id: u64,
	pub destination_chain_id: u64,
	pub open_deadline: u32,
	pub fill_deadline: u32,
	pub order_id: [u8; 32],
	pub settle_gas_limit: u64,
	pub outputs: Vec<Output>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
	pub token: String,
	pub amount: U256,
	pub recipient: String,
	pub chain_id: u64,
}

/// EIP-7683 order implementation
pub struct Eip7683OrderImpl {
	output_settler_address: Address,
	input_settler_address: Address,
}

impl Eip7683OrderImpl {
	pub fn new(output_settler: String, input_settler: String) -> Self {
		Self {
			output_settler_address: Address(
				hex::decode(output_settler.trim_start_matches("0x"))
					.expect("Invalid output settler address"),
			),
			input_settler_address: Address(
				hex::decode(input_settler.trim_start_matches("0x"))
					.expect("Invalid input settler address"),
			),
		}
	}
}

#[async_trait]
impl OrderInterface for Eip7683OrderImpl {
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

		if now > order_data.fill_deadline {
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

	async fn generate_fill_transaction(
		&self,
		order: &Order,
		_params: &ExecutionParams,
	) -> Result<Transaction, OrderError> {
		let order_data: Eip7683OrderData =
			serde_json::from_value(order.data.clone()).map_err(|e| {
				OrderError::ValidationFailed(format!("Failed to parse order data: {}", e))
			})?;

		// Generate fill transaction
		// In production, would properly encode the call data
		// For now, simplified version
		let mut call_data = Vec::new();

		// Function selector for fill(bytes32 orderId, bytes fillData)
		call_data.extend_from_slice(&hex::decode("1234abcd").unwrap()); // Placeholder selector

		// Encode order ID
		call_data.extend_from_slice(&order_data.order_id);

		// Encode fill data (simplified)
		call_data.extend_from_slice(&[0u8; 64]); // Placeholder fill data

		Ok(Transaction {
			to: Some(self.output_settler_address.clone()),
			data: call_data,
			value: U256::ZERO,
			chain_id: order_data.destination_chain_id,
			nonce: None,
			gas_limit: None,
			gas_price: None,
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
		})
	}

	async fn generate_claim_transaction(
		&self,
		order: &Order,
		fill_proof: &FillProof,
	) -> Result<Transaction, OrderError> {
		let order_data: Eip7683OrderData =
			serde_json::from_value(order.data.clone()).map_err(|e| {
				OrderError::ValidationFailed(format!("Failed to parse order data: {}", e))
			})?;

		// Generate claim transaction
		let mut call_data = Vec::new();

		// Function selector for claim(bytes32 orderId, bytes proof)
		call_data.extend_from_slice(&hex::decode("5678efab").unwrap()); // Placeholder selector

		// Encode order ID
		call_data.extend_from_slice(&order_data.order_id);

		// Encode proof data
		if let Some(attestation) = &fill_proof.attestation_data {
			call_data.extend_from_slice(attestation);
		} else {
			call_data.extend_from_slice(&[0u8; 64]); // Placeholder proof
		}

		Ok(Transaction {
			to: Some(self.input_settler_address.clone()),
			data: call_data,
			value: U256::ZERO,
			chain_id: order_data.origin_chain_id,
			nonce: None,
			gas_limit: None,
			gas_price: None,
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
		})
	}
}

/// Simple strategy that always executes
pub struct SimpleStrategy {
	max_gas_price: U256,
}

impl SimpleStrategy {
	pub fn new(max_gas_price_gwei: u64) -> Self {
		Self {
			max_gas_price: U256::from(max_gas_price_gwei) * U256::from(10u64.pow(9)),
		}
	}
}

#[async_trait]
impl ExecutionStrategy for SimpleStrategy {
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

pub fn create_order_impl(config: &toml::Value) -> Box<dyn OrderInterface> {
	let output_settler = config
		.get("output_settler_address")
		.and_then(|v| v.as_str())
		.expect("output_settler_address is required");

	let input_settler = config
		.get("input_settler_address")
		.and_then(|v| v.as_str())
		.expect("input_settler_address is required");

	Box::new(Eip7683OrderImpl::new(
		output_settler.to_string(),
		input_settler.to_string(),
	))
}

pub fn create_strategy(config: &toml::Value) -> Box<dyn ExecutionStrategy> {
	let max_gas_price = config
		.get("max_gas_price_gwei")
		.and_then(|v| v.as_integer())
		.unwrap_or(100) as u64;

	Box::new(SimpleStrategy::new(max_gas_price))
}
