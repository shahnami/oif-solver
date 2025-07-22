// solver-plugins/src/orders/eip7683.rs

use async_trait::async_trait;
use bytes::Bytes;
use ethers::abi::{decode, encode, ParamType, Token};
use ethers::utils;
use serde::{Deserialize, Serialize};
use solver_types::plugins::settlement::{
	SettlementMetadata, SettlementPriority, SettlementRequest, SettlementTransaction,
	SettlementType,
};
use solver_types::plugins::*;
use std::any::Any;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tracing::info;

use crate::order::processor::OrderPluginProcessor;

/// EIP-7683 Order implementation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip7683Order {
	pub order_id: String,
	pub user: Address,
	pub origin_chain_id: ChainId,
	pub destination_chain_id: ChainId,
	pub created_at: Timestamp,
	pub expires_at: Timestamp,
	pub nonce: u64,
	pub signature: Bytes,

	// EIP-7683 specific fields
	pub settle_gas_limit: u64,
	pub fill_deadline: Timestamp,
	pub order_data_type: String,
	pub order_data: Bytes,
	pub mandate_outputs: Vec<MandateOutput>,
	pub inputs: Vec<(String, u64, u64)>, // (token, amount, chain_id)
	pub local_oracle: Address,           // Oracle address from the original order
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MandateOutput {
	pub token: Address,
	pub amount: u64,
	pub recipient: Address,
	pub chain_id: ChainId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip7683Metadata {
	pub mandate_outputs_count: usize,
	pub total_output_value: u64,
	pub is_cross_chain: bool,
	pub order_data_type: String,
}

impl Order for Eip7683Order {
	type Id = String;
	type Metadata = Eip7683Metadata;

	fn id(&self) -> Self::Id {
		self.order_id.clone()
	}

	fn user(&self) -> Address {
		self.user.clone()
	}

	fn origin_chain(&self) -> ChainId {
		self.origin_chain_id
	}

	fn destination_chain(&self) -> ChainId {
		self.destination_chain_id
	}

	fn created_at(&self) -> Timestamp {
		self.created_at
	}

	fn expires_at(&self) -> Timestamp {
		self.expires_at
	}

	fn metadata(&self) -> &Self::Metadata {
		// Return a reference to metadata stored in the order
		// This requires storing metadata as a field, which we'll address by
		// making metadata() return by value instead
		unimplemented!("metadata() returning reference not supported; use owned value")
	}

	fn encode(&self) -> PluginResult<Bytes> {
		let serialized = serde_json::to_vec(self)
			.map_err(|e| PluginError::ExecutionFailed(format!("Serialization failed: {}", e)))?;
		Ok(Bytes::from(serialized))
	}

	fn decode(data: &[u8]) -> PluginResult<Self> {
		let order: Self = serde_json::from_slice(data)
			.map_err(|e| PluginError::ExecutionFailed(format!("Deserialization failed: {}", e)))?;
		Ok(order)
	}

	fn validate(&self) -> PluginResult<()> {
		// Basic validation
		if self.order_id.is_empty() {
			info!("Order ID cannot be empty");
			return Err(PluginError::InvalidConfiguration(
				"Order ID cannot be empty".to_string(),
			));
		}

		if self.expires_at <= self.created_at {
			info!("Expiry must be after creation time: {:#?}", self);
			return Err(PluginError::InvalidConfiguration(
				"Expiry must be after creation time".to_string(),
			));
		}

		if self.mandate_outputs.is_empty() {
			info!("Must have at least one mandate output");
			return Err(PluginError::InvalidConfiguration(
				"Must have at least one mandate output".to_string(),
			));
		}

		// Validate fill deadline
		if self.fill_deadline <= self.created_at {
			info!("Fill deadline must be after creation time");
			return Err(PluginError::InvalidConfiguration(
				"Fill deadline must be after creation time".to_string(),
			));
		}

		Ok(())
	}
}

/// EIP-7683 Order Plugin
#[derive(Debug)]
pub struct Eip7683OrderPlugin {
	config: Eip7683Config,
	metrics: PluginMetrics,
	is_initialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip7683Config {
	pub max_order_age_seconds: u64,
	pub min_fill_deadline_seconds: u64,
	pub supported_chains: Vec<ChainId>,
	pub validate_signatures: bool,
	pub order_data_types: Vec<String>,
	pub solver_address: Address,
	pub output_settler_address: Address,
	pub input_settler_addresses: Vec<Address>, // InputSettler addresses per chain
	pub oracle_address: Option<Address>,       // Oracle address for settlement verification
}

impl Default for Eip7683Config {
	fn default() -> Self {
		Self {
			max_order_age_seconds: 86400,                        // 24 hours
			min_fill_deadline_seconds: 300,                      // 5 minutes
			supported_chains: vec![1, 42161, 137, 31337, 31338], // Ethereum, Arbitrum, Polygon, Hardhat
			validate_signatures: true,
			order_data_types: vec!["standard".to_string(), "dutch_auction".to_string()],
			solver_address: "0x0000000000000000000000000000000000000000".to_string(), // Must be configured
			output_settler_address: "0x0000000000000000000000000000000000000000".to_string(), // Must be configured
			input_settler_addresses: vec![], // Must be configured per chain
			oracle_address: None,            // Optional oracle address
		}
	}
}

#[derive(Debug, Clone, Default)]
pub struct Eip7683ParseContext {
	pub expected_chain_id: Option<ChainId>,
	pub validate_signature: bool,
}

impl Default for Eip7683OrderPlugin {
	fn default() -> Self {
		Self::new()
	}
}

impl Eip7683OrderPlugin {
	pub fn new() -> Self {
		Self {
			config: Eip7683Config::default(),
			metrics: PluginMetrics::new(),
			is_initialized: false,
		}
	}

	pub fn with_config(config: Eip7683Config) -> Self {
		Self {
			config,
			metrics: PluginMetrics::new(),
			is_initialized: false,
		}
	}

	fn validate_signature(&self, order: &Eip7683Order) -> PluginResult<bool> {
		if !self.config.validate_signatures {
			return Ok(true);
		}

		// In a real implementation, this would:
		// 1. Reconstruct the message hash from order data
		// 2. Recover the signer from the signature
		// 3. Verify the signer matches the order.user

		// Placeholder implementation
		if order.signature.is_empty() {
			return Ok(false);
		}

		Ok(true) // Assume valid for now
	}

	fn extract_order_metadata(&self, order: &Eip7683Order) -> OrderIndexMetadata {
		OrderIndexMetadata {
			order_type: "eip7683".to_string(),
			user: order.user.clone(),
			origin_chain: order.origin_chain_id,
			destination_chain: order.destination_chain_id,
			created_at: order.created_at,
			expires_at: order.expires_at,
			status: if order.is_expired() {
				OrderStatus::Expired
			} else {
				OrderStatus::Pending
			},
			tags: vec![
				format!(
					"chain_{}_{}",
					order.origin_chain_id, order.destination_chain_id
				),
				order.order_data_type.clone(),
			],
			custom_fields: {
				let mut fields = HashMap::new();
				fields.insert(
					"mandate_outputs_count".to_string(),
					order.mandate_outputs.len().to_string(),
				);
				fields.insert(
					"settle_gas_limit".to_string(),
					order.settle_gas_limit.to_string(),
				);
				fields.insert("fill_deadline".to_string(), order.fill_deadline.to_string());
				fields
			},
		}
	}
}

#[async_trait]
impl BasePlugin for Eip7683OrderPlugin {
	fn plugin_type(&self) -> &'static str {
		"eip7683_order"
	}

	fn name(&self) -> String {
		"EIP-7683 Order Plugin".to_string()
	}

	fn version(&self) -> &'static str {
		"1.0.0"
	}

	fn description(&self) -> &'static str {
		"Plugin for handling EIP-7683 cross-chain orders"
	}

	async fn initialize(&mut self, _config: PluginConfig) -> PluginResult<()> {
		// Configuration is already loaded in with_config()
		// This method is kept for BasePlugin trait compliance
		// and potential future async initialization needs
		if !self.is_initialized {
			return Err(PluginError::ExecutionFailed(
				"Plugin not properly initialized with config".to_string(),
			));
		}
		Ok(())
	}

	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()> {
		// Validate configuration values
		if let Some(max_age) = config.get_number("max_order_age_seconds") {
			if max_age <= 0 {
				return Err(PluginError::InvalidConfiguration(
					"max_order_age_seconds must be positive".to_string(),
				));
			}
		}

		if let Some(min_deadline) = config.get_number("min_fill_deadline_seconds") {
			if min_deadline <= 0 {
				return Err(PluginError::InvalidConfiguration(
					"min_fill_deadline_seconds must be positive".to_string(),
				));
			}
		}

		Ok(())
	}

	async fn health_check(&self) -> PluginResult<PluginHealth> {
		if !self.is_initialized {
			return Ok(PluginHealth::unhealthy("Plugin not initialized"));
		}

		Ok(
			PluginHealth::healthy("EIP-7683 order plugin is operational")
				.with_detail(
					"supported_chains",
					format!("{:?}", self.config.supported_chains),
				)
				.with_detail(
					"validate_signatures",
					self.config.validate_signatures.to_string(),
				),
		)
	}

	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		Ok(self.metrics.clone())
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		self.is_initialized = false;
		Ok(())
	}

	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new()
			.optional(
				"max_order_age_seconds",
				ConfigFieldType::Number,
				"Maximum age of orders in seconds",
				Some(86400.into()),
			)
			.optional(
				"min_fill_deadline_seconds",
				ConfigFieldType::Number,
				"Minimum fill deadline in seconds",
				Some(300.into()),
			)
			.optional(
				"validate_signatures",
				ConfigFieldType::Boolean,
				"Whether to validate order signatures",
				Some(true.into()),
			)
			.required(
				"solver_address",
				ConfigFieldType::String,
				"Ethereum address of the solver",
			)
			.required(
				"output_settler_address",
				ConfigFieldType::String,
				"Address of the OutputSettler7683 contract",
			)
	}

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

#[async_trait]
impl OrderPlugin for Eip7683OrderPlugin {
	type Order = Eip7683Order;
	type OrderId = String;
	type ParseContext = Eip7683ParseContext;

	async fn parse_order(
		&self,
		data: &[u8],
		context: Option<Self::ParseContext>,
	) -> PluginResult<Self::Order> {
		// The data is ABI-encoded event data from the Open event
		// Open(bytes32 indexed orderId, ResolvedCrossChainOrder resolvedOrder)
		// The event data contains the ResolvedCrossChainOrder struct

		// For event data with struct parameters, the data is ABI-encoded with dynamic offset
		// First 32 bytes contain the offset to the actual struct data
		// Since we have only one parameter (the struct), the offset should be 0x20 (32)

		// Skip the offset and get the actual struct data
		if data.len() < 32 {
			return Err(PluginError::ExecutionFailed(
				"Event data too short".to_string(),
			));
		}

		// Define the ABI structure for the ResolvedCrossChainOrder struct
		let resolved_order_type = ParamType::Tuple(vec![
			ParamType::Address,        // user
			ParamType::Uint(256),      // originChainId
			ParamType::Uint(256),      // openDeadline (stored as uint256 in ABI encoding)
			ParamType::Uint(256),      // fillDeadline (stored as uint256 in ABI encoding)
			ParamType::FixedBytes(32), // orderId
			ParamType::Array(Box::new(ParamType::Tuple(vec![
				// maxSpent (Output[])
				ParamType::FixedBytes(32), // token (bytes32)
				ParamType::Uint(256),      // amount
				ParamType::FixedBytes(32), // recipient (bytes32)
				ParamType::Uint(256),      // chainId
			]))),
			ParamType::Array(Box::new(ParamType::Tuple(vec![
				// minReceived (Output[])
				ParamType::FixedBytes(32), // token (bytes32)
				ParamType::Uint(256),      // amount
				ParamType::FixedBytes(32), // recipient (bytes32)
				ParamType::Uint(256),      // chainId
			]))),
			ParamType::Array(Box::new(ParamType::Tuple(vec![
				// fillInstructions
				ParamType::Uint(256),      // destinationChainId
				ParamType::FixedBytes(32), // destinationSettler (bytes32)
				ParamType::Bytes,          // originData
			]))),
		]);

		// Decode the event data
		// The event data already includes the offset, so we decode the entire data
		let tokens = decode(&[resolved_order_type], data).map_err(|e| {
			PluginError::ExecutionFailed(format!("Failed to decode event data: {}", e))
		})?;

		// Extract the struct from the decoded data
		let resolved_order = match &tokens[0] {
			Token::Tuple(fields) if fields.len() == 8 => fields,
			Token::Tuple(fields) => {
				return Err(PluginError::ExecutionFailed(format!(
					"Invalid ResolvedCrossChainOrder: expected 8 fields, got {}",
					fields.len()
				)));
			}
			_ => {
				return Err(PluginError::ExecutionFailed(
					"Expected tuple token for ResolvedCrossChainOrder".to_string(),
				));
			}
		};

		let user = match &resolved_order[0] {
			Token::Address(addr) => format!("{:?}", addr),
			_ => {
				return Err(PluginError::ExecutionFailed(
					"Invalid user address".to_string(),
				))
			}
		};

		let origin_chain_id = match &resolved_order[1] {
			Token::Uint(chain_id) => chain_id.as_u64(),
			_ => {
				return Err(PluginError::ExecutionFailed(
					"Invalid origin chain ID".to_string(),
				))
			}
		};

		let open_deadline = match &resolved_order[2] {
			Token::Uint(deadline) => deadline.low_u64(),
			_ => {
				return Err(PluginError::ExecutionFailed(
					"Invalid open deadline".to_string(),
				));
			}
		};

		let fill_deadline = match &resolved_order[3] {
			Token::Uint(deadline) => deadline.low_u64(),
			_ => {
				return Err(PluginError::ExecutionFailed(
					"Invalid fill deadline".to_string(),
				))
			}
		};

		let order_id = match &resolved_order[4] {
			Token::FixedBytes(bytes) => format!("0x{}", hex::encode(bytes)),
			_ => return Err(PluginError::ExecutionFailed("Invalid order ID".to_string())),
		};

		// Parse maxSpent outputs (what the user wants to receive)
		// Note: In the ResolvedCrossChainOrder, minReceived are the inputs (what user provides),
		// and maxSpent are the outputs (what user wants to receive)
		let max_spent =
			match &resolved_order[5] {
				Token::Array(outputs) => outputs
					.iter()
					.map(|output| match output {
						Token::Tuple(fields) => {
							let token = match &fields[0] {
								Token::FixedBytes(bytes) => {
									// Extract address from bytes32 (last 20 bytes)
									if bytes.len() == 32 {
										format!("0x{}", hex::encode(&bytes[12..32]))
									} else {
										return Err(PluginError::ExecutionFailed(
											"Invalid token bytes32".to_string(),
										));
									}
								}
								_ => {
									return Err(PluginError::ExecutionFailed(
										"Invalid token field".to_string(),
									))
								}
							};
							let amount = match &fields[1] {
								Token::Uint(amt) => amt.as_u64(),
								_ => {
									return Err(PluginError::ExecutionFailed(
										"Invalid amount".to_string(),
									))
								}
							};
							let recipient = match &fields[2] {
								Token::FixedBytes(bytes) => {
									// Extract address from bytes32 (last 20 bytes)
									if bytes.len() == 32 {
										format!("0x{}", hex::encode(&bytes[12..32]))
									} else {
										return Err(PluginError::ExecutionFailed(
											"Invalid recipient bytes32".to_string(),
										));
									}
								}
								_ => {
									return Err(PluginError::ExecutionFailed(
										"Invalid recipient field".to_string(),
									))
								}
							};
							let chain_id = match &fields[3] {
								Token::Uint(chain) => chain.as_u64(),
								_ => {
									return Err(PluginError::ExecutionFailed(
										"Invalid chain ID".to_string(),
									))
								}
							};
							info!("Parsed MandateOutput: token={}, amount={}, recipient={}, chain_id={}", 
							token, amount, recipient, chain_id);
							Ok(MandateOutput {
								token,
								amount,
								recipient,
								chain_id,
							})
						}
						_ => Err(PluginError::ExecutionFailed(
							"Invalid output structure".to_string(),
						)),
					})
					.collect::<Result<Vec<_>, _>>()?,
				_ => vec![],
			};

		// Parse minReceived inputs (what the user provides)
		// Note: In the ResolvedCrossChainOrder, minReceived represents the inputs (what user deposits)
		let min_received = match &resolved_order[6] {
			Token::Array(inputs) => inputs
				.iter()
				.map(|input| match input {
					Token::Tuple(fields) => {
						if fields.len() >= 4 {
							// Extract token, amount, recipient, and chain_id
							let token = match &fields[0] {
								Token::FixedBytes(bytes) if bytes.len() == 32 => {
									format!("0x{}", hex::encode(&bytes[12..32]))
								}
								_ => {
									return Err(PluginError::ExecutionFailed(
										"Invalid input token field".to_string(),
									))
								}
							};
							let amount = match &fields[1] {
								Token::Uint(amt) => amt.as_u64(),
								_ => {
									return Err(PluginError::ExecutionFailed(
										"Invalid input amount".to_string(),
									))
								}
							};
							let chain_id = match &fields[3] {
								Token::Uint(chain) => chain.as_u64(),
								_ => {
									return Err(PluginError::ExecutionFailed(
										"Invalid input chain ID".to_string(),
									))
								}
							};
							info!(
								"Parsed minReceived input: token={}, amount={}, chain_id={}",
								token, amount, chain_id
							);
							Ok((token, amount, chain_id))
						} else {
							Err(PluginError::ExecutionFailed(
								"Invalid input structure".to_string(),
							))
						}
					}
					_ => Err(PluginError::ExecutionFailed(
						"Invalid input structure".to_string(),
					)),
				})
				.collect::<Result<Vec<_>, _>>()?,
			_ => vec![],
		};

		// Parse fill instructions to get destination chain
		let destination_chain_id = match &resolved_order[7] {
			Token::Array(instructions) => instructions
				.first()
				.and_then(|instruction| match instruction {
					Token::Tuple(fields) => match &fields[0] {
						Token::Uint(chain_id) => Some(chain_id.as_u64()),
						_ => None,
					},
					_ => None,
				})
				.unwrap_or(origin_chain_id),
			_ => origin_chain_id,
		};

		// The oracle address needs to be obtained from the configuration
		// Since the ResolvedCrossChainOrder event doesn't contain the original MandateERC7683 data,
		// we'll use a configured oracle address. This should be set in the plugin configuration.
		// For local testing, this is the AlwaysYesOracle address.
		let local_oracle = self.config.oracle_address.clone().unwrap_or_else(|| {
			info!("No oracle address configured, using default AlwaysYesOracle for local testing");
			"0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0".to_string()
		});
		info!("Using oracle address: {}", local_oracle);

		// Create the order
		// For onchain orders, openDeadline is 0 (already opened)
		// We use fillDeadline as the expiry for validation purposes
		let order = Eip7683Order {
			order_id,
			user,
			origin_chain_id,
			destination_chain_id,
			created_at: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			expires_at: if open_deadline == 0 {
				fill_deadline
			} else {
				open_deadline
			},
			nonce: 0,                            // Not available in event data
			signature: Bytes::from(vec![0; 65]), // Not available in event data
			settle_gas_limit: 0,                 // Not available in event data
			fill_deadline,
			order_data_type: "standard".to_string(),
			order_data: Bytes::from(data.to_vec()),
			mandate_outputs: max_spent,
			inputs: min_received,
			local_oracle,
		};

		// Apply context-specific validation
		if let Some(ctx) = context {
			if let Some(expected_chain) = ctx.expected_chain_id {
				if order.origin_chain_id != expected_chain
					&& order.destination_chain_id != expected_chain
				{
					return Err(PluginError::ExecutionFailed(format!(
						"Order chains {} -> {} don't match expected chain {}",
						order.origin_chain_id, order.destination_chain_id, expected_chain
					)));
				}
			}

			if ctx.validate_signature && !self.validate_signature(&order)? {
				return Err(PluginError::ExecutionFailed(
					"Invalid signature".to_string(),
				));
			}
		}

		Ok(order)
	}

	async fn validate_order(&self, order: &Self::Order) -> PluginResult<OrderValidation> {
		let mut errors = Vec::new();

		// Basic validation
		if let Err(e) = order.validate() {
			errors.push(ValidationError {
				code: "BASIC_VALIDATION_FAILED".to_string(),
				message: e.to_string(),
				field: None,
			});
		}

		// EIP-7683 specific validation
		if !self
			.config
			.supported_chains
			.contains(&order.origin_chain_id)
		{
			errors.push(ValidationError {
				code: "UNSUPPORTED_ORIGIN_CHAIN".to_string(),
				message: format!("Origin chain {} not supported", order.origin_chain_id),
				field: Some("origin_chain_id".to_string()),
			});
		}

		if !self
			.config
			.supported_chains
			.contains(&order.destination_chain_id)
		{
			errors.push(ValidationError {
				code: "UNSUPPORTED_DESTINATION_CHAIN".to_string(),
				message: format!(
					"Destination chain {} not supported",
					order.destination_chain_id
				),
				field: Some("destination_chain_id".to_string()),
			});
		}

		// Check order age
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();
		let age = now.saturating_sub(order.created_at);
		if age > self.config.max_order_age_seconds {
			errors.push(ValidationError {
				code: "ORDER_TOO_OLD".to_string(),
				message: format!(
					"Order age {} exceeds maximum {}",
					age, self.config.max_order_age_seconds
				),
				field: Some("created_at".to_string()),
			});
		}

		// Check fill deadline
		let deadline_buffer = order.fill_deadline.saturating_sub(order.created_at);
		if deadline_buffer < self.config.min_fill_deadline_seconds {
			errors.push(ValidationError {
				code: "FILL_DEADLINE_TOO_SHORT".to_string(),
				message: format!(
					"Fill deadline buffer {} is less than minimum {}",
					deadline_buffer, self.config.min_fill_deadline_seconds
				),
				field: Some("fill_deadline".to_string()),
			});
		}

		// Validate order data type
		if !self
			.config
			.order_data_types
			.contains(&order.order_data_type)
		{
			errors.push(ValidationError {
				code: "UNSUPPORTED_ORDER_DATA_TYPE".to_string(),
				message: format!("Order data type '{}' not supported", order.order_data_type),
				field: Some("order_data_type".to_string()),
			});
		}

		// Signature validation
		if self.config.validate_signatures && !self.validate_signature(order)? {
			errors.push(ValidationError {
				code: "INVALID_SIGNATURE".to_string(),
				message: "Order signature is invalid".to_string(),
				field: Some("signature".to_string()),
			});
		}

		let validation = if errors.is_empty() {
			OrderValidation::valid()
		} else {
			OrderValidation::invalid(errors)
		};

		Ok(validation)
	}

	async fn extract_metadata(&self, order: &Self::Order) -> PluginResult<OrderIndexMetadata> {
		Ok(self.extract_order_metadata(order))
	}

	async fn get_order(&self, _id: &Self::OrderId) -> PluginResult<Option<Self::Order>> {
		// This plugin doesn't store orders, just parses them
		// In a real implementation, this might query a database or cache
		Ok(None)
	}

	fn can_handle(&self, data: &[u8]) -> bool {
		// Try to parse as EIP-7683 order
		match Eip7683Order::decode(data) {
			Ok(order) => {
				// Additional checks to ensure it's really an EIP-7683 order
				!order.order_id.is_empty()
					&& !order.mandate_outputs.is_empty()
					&& order.settle_gas_limit > 0
			}
			Err(_) => false,
		}
	}

	fn order_type(&self) -> &'static str {
		"eip7683"
	}

	async fn estimate_order(&self, order: &Self::Order) -> PluginResult<OrderEstimate> {
		// Estimate various parameters for this order
		let cross_chain_penalty = if order.origin_chain_id != order.destination_chain_id {
			1.5
		} else {
			1.0
		};
		let complexity_score = (order.mandate_outputs.len() as f64 * 0.1).min(1.0);

		let estimated_fill_time = (300.0 * cross_chain_penalty * (1.0 + complexity_score)) as u64; // base 5 minutes
		let estimated_gas = order.settle_gas_limit + (order.mandate_outputs.len() as u64 * 50000); // estimate

		Ok(OrderEstimate {
			estimated_fill_time: Some(estimated_fill_time),
			estimated_gas_cost: Some(estimated_gas),
			estimated_fees: {
				let mut fees = HashMap::new();
				fees.insert("settlement_fee".to_string(), estimated_gas * 20_000_000_000); // 20 gwei
				fees
			},
			feasibility_score: if order.is_expired() {
				0.0
			} else {
				0.8 * cross_chain_penalty.recip()
			},
			recommendations: vec![
				if cross_chain_penalty > 1.0 {
					"Cross-chain order will take longer"
				} else {
					"Same-chain order, faster execution"
				}
				.to_string(),
				if order.mandate_outputs.len() > 3 {
					"Complex order with multiple outputs"
				} else {
					"Simple order structure"
				}
				.to_string(),
			],
		})
	}

	async fn create_fill_request(&self, order: &Self::Order) -> PluginResult<DeliveryRequest> {
		// For EIP-7683, we need to call the fill function on the destination chain contract

		// Extract order_id as bytes32 from the hex string
		let order_id_bytes = if order.order_id.starts_with("0x") {
			hex::decode(&order.order_id[2..]).map_err(|e| {
				PluginError::ExecutionFailed(format!("Failed to decode order ID: {}", e))
			})?
		} else {
			hex::decode(&order.order_id).map_err(|e| {
				PluginError::ExecutionFailed(format!("Failed to decode order ID: {}", e))
			})?
		};

		// Ensure order_id is 32 bytes
		if order_id_bytes.len() != 32 {
			return Err(PluginError::ExecutionFailed(
				"Order ID must be 32 bytes".to_string(),
			));
		}

		// Get the MandateOutput for the destination chain and encode it
		// The OutputSettler expects a MandateOutput struct, not the full order data
		let destination_output = order
			.mandate_outputs
			.iter()
			.find(|output| output.chain_id == order.destination_chain_id)
			.ok_or_else(|| {
				PluginError::ExecutionFailed(
					"No mandate output found for destination chain".to_string(),
				)
			})?;

		// Encode the MandateOutput struct according to Solidity ABI
		// struct MandateOutput {
		//     bytes32 oracle;
		//     bytes32 settler;
		//     uint256 chainId;
		//     bytes32 token;
		//     uint256 amount;
		//     bytes32 recipient;
		//     bytes call;
		//     bytes context;
		// }

		// Convert addresses to bytes32 format (left-padded with zeros)
		let oracle_bytes32 = vec![0u8; 32]; // No oracle for this output

		let mut settler_bytes32 = vec![0u8; 32];
		let settler_hex = self.config.output_settler_address.trim_start_matches("0x");
		let settler_bytes = hex::decode(settler_hex).map_err(|e| {
			PluginError::ExecutionFailed(format!("Failed to decode settler address: {}", e))
		})?;
		settler_bytes32[12..32].copy_from_slice(&settler_bytes);

		let mut token_bytes32 = vec![0u8; 32];
		let token_hex = destination_output.token.trim_start_matches("0x");
		let token_bytes = hex::decode(token_hex).map_err(|e| {
			PluginError::ExecutionFailed(format!("Failed to decode token address: {}", e))
		})?;
		token_bytes32[12..32].copy_from_slice(&token_bytes);

		let mut recipient_bytes32 = vec![0u8; 32];
		let recipient_hex = destination_output.recipient.trim_start_matches("0x");
		let recipient_bytes = hex::decode(recipient_hex).map_err(|e| {
			PluginError::ExecutionFailed(format!("Failed to decode recipient address: {}", e))
		})?;
		recipient_bytes32[12..32].copy_from_slice(&recipient_bytes);

		// Encode the MandateOutput
		let mandate_output_tokens = vec![
			Token::FixedBytes(oracle_bytes32),               // oracle
			Token::FixedBytes(settler_bytes32),              // settler
			Token::Uint(destination_output.chain_id.into()), // chainId
			Token::FixedBytes(token_bytes32),                // token
			Token::Uint(destination_output.amount.into()),   // amount
			Token::FixedBytes(recipient_bytes32),            // recipient
			Token::Bytes(vec![]),                            // call (empty)
			Token::Bytes(vec![]),                            // context (empty)
		];

		let origin_data = encode(&[Token::Tuple(mandate_output_tokens)]);

		info!("Encoded MandateOutput for destination chain {}: settler={}, token={}, amount={}, recipient={}", 
			destination_output.chain_id,
			self.config.output_settler_address,
			destination_output.token,
			destination_output.amount,
			destination_output.recipient
		);

		// Create filler data - 32 bytes with solver address in the last 20 bytes
		let mut filler_data = vec![0u8; 32];
		// Use the configured solver address
		let solver_address = Address::from_str(&self.config.solver_address)
			.map_err(|e| PluginError::ExecutionFailed(format!("Invalid solver address: {}", e)))?;
		filler_data[12..32].copy_from_slice(&solver_address.as_bytes()[..20]);

		// Encode the fill function call
		// Function: fill(bytes32 orderId, bytes originData, bytes fillerData)
		let tokens = vec![
			Token::FixedBytes(order_id_bytes),
			Token::Bytes(origin_data),
			Token::Bytes(filler_data),
		];

		// Function selector for fill(bytes32,bytes,bytes)
		let function_signature = "fill(bytes32,bytes,bytes)";
		let selector = &utils::keccak256(function_signature.as_bytes())[..4];
		let encoded_params = encode(&tokens);

		// Combine selector and encoded parameters
		let mut call_data = Vec::new();
		call_data.extend_from_slice(selector);
		call_data.extend_from_slice(&encoded_params);

		// Calculate gas limit based on the complexity of the fill
		// Base gas for contract call + additional gas per output
		let gas_limit = 150000 + (order.mandate_outputs.len() as u64 * 50000);

		// Create the transaction to call the contract
		// Use the configured OutputSettler address
		info!(
			"Using OutputSettler address: {}",
			self.config.output_settler_address
		);
		let output_settler = Address::from_str(&self.config.output_settler_address)
			.map_err(|e| PluginError::ExecutionFailed(format!("Invalid solver address: {}", e)))?;

		let transaction = Transaction {
			to: output_settler,
			value: 0, // No ETH value, tokens will be transferred by the contract
			data: Bytes::from(call_data),
			gas_limit,
			gas_price: None,
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
			nonce: None,
			chain_id: order.destination_chain_id,
		};

		// Create the delivery request
		Ok(DeliveryRequest {
			transaction,
			priority: DeliveryPriority::Normal,
			metadata: DeliveryMetadata {
				order_id: order.order_id.clone(),
				user: order.user.clone(),
				tags: vec!["eip7683".to_string(), "fill".to_string()],
				custom_fields: {
					let mut fields = HashMap::new();
					fields.insert("order_type".to_string(), "eip7683".to_string());
					fields.insert(
						"origin_chain".to_string(),
						order.origin_chain_id.to_string(),
					);
					fields.insert(
						"destination_chain".to_string(),
						order.destination_chain_id.to_string(),
					);
					fields.insert("fill_type".to_string(), "contract_call".to_string());
					fields
				},
			},
			retry_config: Some(RetryConfig::default()),
		})
	}

	async fn create_settlement_request(
		&self,
		order: &Self::Order,
		fill_timestamp: Timestamp,
	) -> PluginResult<Option<SettlementRequest>> {
		// For EIP-7683, we need to call finaliseSelf on the InputSettler contract
		// on the origin chain to claim the locked funds

		// Calculate total fill amount from mandate outputs
		let fill_amount: u64 = order
			.mandate_outputs
			.iter()
			.map(|output| output.amount)
			.sum();

		// Create the finaliseSelf function call
		use ethers::abi::{Function, Param, ParamType, Token};

		#[allow(deprecated)]
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

		// Get the function selector
		let selector = function.short_signature();

		// Log the oracle address being used
		info!(
			"Using oracle address for settlement: {}",
			order.local_oracle
		);

		// Encode the order struct
		let order_struct = Token::Tuple(vec![
			Token::Address(ethers::types::H160::from_str(&order.user).unwrap()),
			Token::Uint(ethers::types::U256::from(order.nonce)),
			Token::Uint(ethers::types::U256::from(order.origin_chain_id)),
			Token::Uint(ethers::types::U256::from(order.expires_at)), // Use expires_at which should match the original expiry
			Token::Uint(ethers::types::U256::from(order.fill_deadline)),
			Token::Address(
				ethers::types::H160::from_str(&order.local_oracle)
					.unwrap_or_else(|_| ethers::types::H160::zero()),
			),
			Token::Array(if order.inputs.is_empty() {
				// If no inputs were parsed from the event, use the mandate outputs
				// This assumes that for same-chain orders, inputs match outputs
				info!("No inputs found, using mandate outputs as inputs for same-chain order");
				order
					.mandate_outputs
					.iter()
					.filter(|output| output.chain_id == order.origin_chain_id)
					.map(|output| {
						// Convert token address to U256 (as expected by the contract)
						let token_bytes = hex::decode(output.token.trim_start_matches("0x"))
							.unwrap_or_else(|_| vec![0; 20]);
						let mut token_u256_bytes = vec![0u8; 32];
						token_u256_bytes[12..32].copy_from_slice(&token_bytes);
						let token_u256 = ethers::types::U256::from_big_endian(&token_u256_bytes);

						Token::FixedArray(vec![
							Token::Uint(token_u256),
							Token::Uint(ethers::types::U256::from(output.amount)),
						])
					})
					.collect()
			} else {
				// Convert inputs to array of [token, amount] - matching the ABI
				order
					.inputs
					.iter()
					.map(|(token_addr, amount, _chain_id)| {
						// Convert token address to U256 (as expected by the contract)
						let token_bytes = hex::decode(token_addr.trim_start_matches("0x"))
							.unwrap_or_else(|_| vec![0; 20]);
						let mut token_u256_bytes = vec![0u8; 32];
						token_u256_bytes[12..32].copy_from_slice(&token_bytes);
						let token_u256 = ethers::types::U256::from_big_endian(&token_u256_bytes);

						Token::FixedArray(vec![
							Token::Uint(token_u256),
							Token::Uint(ethers::types::U256::from(*amount)),
						])
					})
					.collect()
			}),
			Token::Array(
				order
					.mandate_outputs
					.iter()
					.map(|output| {
						Token::Tuple(vec![
							Token::FixedBytes(vec![0u8; 32]), // oracle - zero for same-chain
							Token::FixedBytes({
								// settler - use the InputSettler address for origin chain
								let mut settler_bytes = vec![0u8; 32];
								if output.chain_id == order.origin_chain_id {
									// For outputs on the origin chain, use the InputSettler address
									if let Some(input_settler) =
										self.config.input_settler_addresses.first()
									{
										if let Ok(settler_addr) =
											ethers::types::H160::from_str(input_settler)
										{
											settler_bytes[12..32]
												.copy_from_slice(settler_addr.as_bytes());
											info!("Using InputSettler address for output on origin chain: {}", input_settler);
										}
									}
								} else {
									// For outputs on other chains, use the OutputSettler address
									if let Ok(output_settler_addr) = ethers::types::H160::from_str(
										&self.config.output_settler_address,
									) {
										settler_bytes[12..32]
											.copy_from_slice(output_settler_addr.as_bytes());
									}
								}
								settler_bytes
							}),
							Token::Uint(ethers::types::U256::from(output.chain_id)),
							Token::FixedBytes({
								let mut token_bytes = vec![0u8; 32];
								let token_addr =
									ethers::types::H160::from_str(&output.token).unwrap();
								token_bytes[12..32].copy_from_slice(token_addr.as_bytes());
								token_bytes
							}),
							Token::Uint(ethers::types::U256::from(output.amount)),
							Token::FixedBytes({
								let mut recipient_bytes = vec![0u8; 32];
								let recipient_addr =
									ethers::types::H160::from_str(&output.recipient).unwrap();
								recipient_bytes[12..32].copy_from_slice(recipient_addr.as_bytes());
								recipient_bytes
							}),
							Token::Bytes(vec![]), // call
							Token::Bytes(vec![]), // context
						])
					})
					.collect(),
			),
		]);

		// Create timestamps array based on fill data
		let timestamps = Token::Array(vec![Token::Uint(ethers::types::U256::from(
			fill_timestamp as u32,
		))]);

		// Create solver bytes32 from the configured solver address
		let solver_token = Token::FixedBytes({
			let mut solver_bytes = vec![0u8; 32];
			// Use the solver address from config
			let solver_addr =
				ethers::types::H160::from_str(&self.config.solver_address).map_err(|e| {
					PluginError::ExecutionFailed(format!("Invalid solver address: {}", e))
				})?;
			solver_bytes[12..32].copy_from_slice(solver_addr.as_bytes());
			solver_bytes
		});

		// Encode the parameters
		let tokens = vec![order_struct, timestamps, solver_token];
		let encoded_params = encode(&tokens);

		// Build the complete call data
		let mut call_data = Vec::new();
		call_data.extend_from_slice(&selector);
		call_data.extend_from_slice(&encoded_params);

		// Get the InputSettler address for the origin chain
		let input_settler = self
			.config
			.input_settler_addresses
			.iter()
			.find(|_addr| {
				// In a real implementation, we'd have a mapping of chain_id -> address
				// For now, just use the first one
				true
			})
			.ok_or_else(|| {
				PluginError::ExecutionFailed(
					"No InputSettler address configured for origin chain".to_string(),
				)
			})?;

		let input_settler_addr = Address::from_str(input_settler).map_err(|e| {
			PluginError::ExecutionFailed(format!("Invalid InputSettler address: {}", e))
		})?;

		// Create the settlement transaction
		let transaction = Transaction {
			to: input_settler_addr,
			value: 0, // No ETH value
			data: Bytes::from(call_data),
			gas_limit: 200000, // Estimated gas for finaliseSelf
			gas_price: None,
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
			nonce: None,
			chain_id: order.origin_chain_id, // Settlement happens on origin chain
		};

		// Create the settlement request
		Ok(Some(SettlementRequest {
			transaction: SettlementTransaction {
				transaction,
				settlement_type: SettlementType::Direct,
				expected_reward: fill_amount, // The amount being settled (from order outputs)
				metadata: SettlementMetadata {
					order_id: order.order_id.clone(),
					strategy: "eip7683_finalise_self".to_string(),
					expected_confirmations: 1,
					custom_fields: {
						let mut fields = HashMap::new();
						fields.insert("order_type".to_string(), "eip7683".to_string());
						fields.insert("settlement_method".to_string(), "finaliseSelf".to_string());
						fields.insert(
							"origin_chain".to_string(),
							order.origin_chain_id.to_string(),
						);
						fields
					},
				},
			},
			priority: SettlementPriority::Immediate,
			preferred_strategy: Some("direct_settlement".to_string()),
			retry_config: Some(RetryConfig::default()),
		}))
	}
}

/// Helper to create EIP-7683 order processor
pub fn create_eip7683_processor(plugin: Arc<Eip7683OrderPlugin>) -> Arc<dyn OrderProcessor> {
	Arc::new(OrderPluginProcessor::new(plugin, "eip7683".to_string()))
}
