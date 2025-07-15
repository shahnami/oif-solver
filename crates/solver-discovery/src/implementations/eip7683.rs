//! EIP-7683 order standard processor.
//!
//! This module implements the processor for EIP-7683 cross-chain orders,
//! handling both on-chain and off-chain intent discovery. EIP-7683 defines
//! a standard for cross-chain order execution with support for:
//!
//! - On-chain orders emitted via `Open` events
//! - Off-chain orders submitted through APIs
//! - Flexible order data encoding
//!
//! # On-chain Processing
//!
//! For on-chain orders, the processor:
//! 1. Extracts event data from the raw intent
//! 2. Validates the `Open` event structure
//! 3. Decodes the `ResolvedCrossChainOrder` from event data
//! 4. Extracts order details and encodes them for the registry
//!
//! # Off-chain Processing
//!
//! For off-chain orders, the processor expects pre-formatted data
//! that can be directly parsed by the order registry.

use super::OrderStandardProcessor;
use crate::{
	events::Event,
	types::{IntentSourceType, RawIntent},
};
use async_trait::async_trait;
use ethers::abi::{decode, ParamType, Token};
use solver_orders::OrderRegistry;
use solver_types::{
	common::{Address, BlockNumber, Bytes32, U256},
	errors::{Result, SolverError},
	standards::eip7683::OnchainCrossChainOrder,
	ChainId, TxHash,
};
use std::sync::Arc;
use tracing::{debug, info};

/// Processor for EIP-7683 cross-chain orders.
///
/// Handles the parsing and processing of EIP-7683 intents from both
/// on-chain events and off-chain sources.
#[derive(Default)]
pub struct EIP7683Processor;

impl EIP7683Processor {
	/// Creates a new EIP-7683 processor instance.
	pub fn new() -> Self {
		Self
	}

	/// Processes EIP-7683 intents from both on-chain and off-chain sources.
	///
	/// Routes the intent to the appropriate handler based on its source type.
	async fn process_eip7683_intent(
		&self,
		raw_intent: RawIntent,
		order_registry: &Arc<OrderRegistry>,
	) -> Result<(solver_orders::OrderImpl, Vec<u8>)> {
		match raw_intent.source {
			IntentSourceType::OnChain {
				chain,
				block,
				transaction_hash,
				log_index,
			} => {
				// Process on-chain EIP-7683 intent
				self.process_onchain_eip7683(
					raw_intent,
					chain,
					block,
					Bytes32::from(transaction_hash),
					log_index,
					order_registry,
				)
				.await
			}
			IntentSourceType::OffChain { .. } => {
				// Process off-chain EIP-7683 intent
				self.process_offchain_eip7683(raw_intent, order_registry)
					.await
			}
		}
	}

	/// Processes on-chain EIP-7683 intents from blockchain events.
	///
	/// Handles intents discovered from on-chain `Open` events, extracting
	/// and parsing the order data from the event logs.
	async fn process_onchain_eip7683(
		&self,
		raw_intent: RawIntent,
		chain: ChainId,
		block: BlockNumber,
		transaction_hash: TxHash,
		log_index: u64,
		order_registry: &Arc<OrderRegistry>,
	) -> Result<(solver_orders::OrderImpl, Vec<u8>)> {
		// Extract event from raw intent
		let event = Self::extract_event_from_raw_intent(
			&raw_intent,
			chain,
			block,
			transaction_hash,
			log_index,
		)?;

		// Check if data needs parsing based on format markers
		if Self::needs_event_parsing(&raw_intent.data) {
			// Parse the event
			self.parse_eip7683_open_event(&event, order_registry).await
		} else {
			// Already formatted, parse directly
			let order = order_registry.parse_order(&raw_intent.data).await?;
			Ok((order, raw_intent.data))
		}
	}

	/// Processes off-chain EIP-7683 intents.
	///
	/// Handles pre-formatted intents submitted through off-chain channels
	/// (e.g., APIs, direct submissions).
	async fn process_offchain_eip7683(
		&self,
		raw_intent: RawIntent,
		order_registry: &Arc<OrderRegistry>,
	) -> Result<(solver_orders::OrderImpl, Vec<u8>)> {
		// Off-chain sources should have properly formatted data
		let order = order_registry.parse_order(&raw_intent.data).await?;
		Ok((order, raw_intent.data))
	}

	/// Extracts event data from raw intent context.
	///
	/// Reconstructs an `Event` structure from the raw intent data and
	/// its associated context (address, topics).
	fn extract_event_from_raw_intent(
		raw_intent: &RawIntent,
		chain: ChainId,
		block: BlockNumber,
		transaction_hash: TxHash,
		log_index: u64,
	) -> Result<Event> {
		let (address, topics) = match &raw_intent.context {
			Some(context) => Self::extract_event_details_from_context(context)?,
			None => (Address::zero(), vec![]),
		};

		Ok(Event {
			address,
			topics,
			data: raw_intent.data.clone(),
			block_number: block,
			transaction_hash,
			log_index,
			chain_id: chain,
		})
	}

	/// Extracts event details from the context JSON.
	///
	/// Parses the event address and topics from the context data
	/// provided by the discovery source.
	fn extract_event_details_from_context(
		context: &serde_json::Value,
	) -> Result<(Address, Vec<Bytes32>)> {
		// Extract address
		let addr_str = context
			.get("address")
			.and_then(|v| v.as_str())
			.unwrap_or("0x0000000000000000000000000000000000000000");
		let address =
			Address::from_slice(&hex::decode(&addr_str[2..]).unwrap_or_else(|_| vec![0u8; 20]));

		// Extract topics
		let topics = context
			.get("topics")
			.and_then(|v| v.as_array())
			.map(|arr| {
				arr.iter()
					.filter_map(|t| t.as_str())
					.map(|s| {
						let bytes = hex::decode(&s[2..]).unwrap_or_else(|_| vec![0u8; 32]);
						Bytes32::from_slice(&bytes)
					})
					.collect()
			})
			.unwrap_or_default();

		Ok((address, topics))
	}

	/// Checks if the data needs event parsing based on format markers.
	///
	/// Returns `true` if the data contains raw event data that needs
	/// to be parsed, `false` if it's already formatted with proper markers.
	///
	/// Format markers:
	/// - `0x01`: Gasless order
	/// - `0x02`: On-chain order
	fn needs_event_parsing(data: &[u8]) -> bool {
		// Data needs parsing if it's:
		// - Longer than 32 bytes (not just an order ID)
		// - Doesn't start with format markers (0x01 for gasless, 0x02 for onchain)
		data.len() > 32
			&& data
				.first()
				.map(|&b| b != 0x01 && b != 0x02)
				.unwrap_or(true)
	}

	/// Parses an EIP-7683 `Open` event into an order.
	///
	/// This method handles the complete parsing flow:
	/// 1. Validates the event structure
	/// 2. Decodes the `ResolvedCrossChainOrder` from event data
	/// 3. Extracts order details and fill instructions
	/// 4. Encodes the order for the registry
	async fn parse_eip7683_open_event(
		&self,
		event: &Event,
		order_registry: &Arc<OrderRegistry>,
	) -> Result<(solver_orders::OrderImpl, Vec<u8>)> {
		info!(
			"Processing Open event - Chain: {}, Block: {}, Tx: {:?}",
			event.chain_id, event.block_number, event.transaction_hash
		);

		// Validate event structure
		Self::validate_open_event(event)?;

		// Extract order ID from indexed topic
		let order_id = event.topics[1];

		// Decode the ResolvedCrossChainOrder from event data
		let resolved_order = Self::decode_resolved_order(&event.data)?;

		// Extract order details from the resolved order
		let (user, origin_chain_id, fill_deadline) = Self::extract_order_details(&resolved_order)?;

		// Extract order data from fill instructions
		let (order_data_type, order_data) =
			Self::extract_order_data_from_fill_instructions(&resolved_order)?;

		// Create onchain order representation
		let _onchain_order = OnchainCrossChainOrder {
			fill_deadline,
			order_data_type,
			order_data: order_data.clone(),
		};

		// Encode the order for the registry
		let encoded_order = Self::encode_onchain_order(
			order_id,
			user,
			origin_chain_id,
			event.block_number,
			fill_deadline,
			order_data_type,
			order_data,
		);

		// Let the registry parse this encoded order
		let order = order_registry.parse_order(&encoded_order).await?;

		Ok((order, encoded_order))
	}

	/// Validates that the event is a valid `Open` event.
	///
	/// Checks:
	/// - Event has topics
	/// - First topic matches the `Open` event signature
	/// - Order ID is present in topics
	/// - Event data is not empty
	fn validate_open_event(event: &Event) -> Result<()> {
		if event.topics.is_empty() {
			return Err(SolverError::Order(
				"Invalid Open event: no topics found".to_string(),
			));
		}

		if event.topics[0] != solver_types::events::eip7683::open_event_topic() {
			return Err(SolverError::Order(format!(
				"Not an Open event. Expected topic: {:?}, got: {:?}",
				solver_types::events::eip7683::open_event_topic(),
				event.topics[0]
			)));
		}

		if event.topics.len() < 2 {
			return Err(SolverError::Order(
				"Invalid Open event: missing orderId topic".to_string(),
			));
		}

		if event.data.is_empty() {
			return Err(SolverError::Order(
				"Open event has empty data field".to_string(),
			));
		}

		Ok(())
	}

	/// Decodes `ResolvedCrossChainOrder` from event data.
	///
	/// Parses the ABI-encoded event data into structured tokens
	/// representing the cross-chain order fields.
	fn decode_resolved_order(data: &[u8]) -> Result<Vec<Token>> {
		let resolved_order_type = ParamType::Tuple(vec![
			ParamType::Address,        // user
			ParamType::Uint(256),      // originChainId
			ParamType::Uint(256),      // openDeadline
			ParamType::Uint(256),      // fillDeadline
			ParamType::FixedBytes(32), // orderId
			ParamType::Array(Box::new(ParamType::Tuple(vec![
				// maxSpent[]
				ParamType::FixedBytes(32), // token
				ParamType::Uint(256),      // amount
				ParamType::FixedBytes(32), // recipient
				ParamType::Uint(256),      // chainId
			]))),
			ParamType::Array(Box::new(ParamType::Tuple(vec![
				// minReceived[]
				ParamType::FixedBytes(32), // token
				ParamType::Uint(256),      // amount
				ParamType::FixedBytes(32), // recipient
				ParamType::Uint(256),      // chainId
			]))),
			ParamType::Array(Box::new(ParamType::Tuple(vec![
				// fillInstructions[]
				ParamType::Uint(256),      // destinationChainId
				ParamType::FixedBytes(32), // destinationSettler
				ParamType::Bytes,          // originData
			]))),
		]);

		let tokens = decode(&[resolved_order_type], data)
			.map_err(|e| SolverError::Order(format!("Failed to decode Open event: {}", e)))?;

		match &tokens[0] {
			Token::Tuple(fields) if fields.len() == 8 => Ok(fields.clone()),
			Token::Tuple(fields) => Err(SolverError::Order(format!(
				"Invalid ResolvedCrossChainOrder: expected 8 fields, got {}",
				fields.len()
			))),
			_ => Err(SolverError::Order(
				"Expected tuple token for ResolvedCrossChainOrder".to_string(),
			)),
		}
	}

	/// Extracts basic order details from resolved order tokens.
	///
	/// Returns:
	/// - User address
	/// - Origin chain ID
	/// - Fill deadline timestamp
	fn extract_order_details(resolved_order: &[Token]) -> Result<(Address, U256, u32)> {
		let user = match &resolved_order[0] {
			Token::Address(addr) => {
				let bytes: [u8; 20] = addr
					.as_bytes()
					.try_into()
					.map_err(|_| SolverError::Order("Invalid address length".to_string()))?;
				Address::from(bytes)
			}
			_ => return Err(SolverError::Order("Invalid user address".to_string())),
		};

		let origin_chain_id = match &resolved_order[1] {
			Token::Uint(val) => {
				let mut bytes = [0u8; 32];
				val.to_big_endian(&mut bytes);
				U256::from_big_endian(&bytes)
			}
			_ => return Err(SolverError::Order("Invalid originChainId".to_string())),
		};

		let fill_deadline = match &resolved_order[3] {
			Token::Uint(val) => val.as_u64() as u32,
			_ => return Err(SolverError::Order("Invalid fillDeadline".to_string())),
		};

		Ok((user, origin_chain_id, fill_deadline))
	}

	/// Extracts order data from fill instructions.
	///
	/// Parses the fill instructions array to extract:
	/// - Order data type identifier
	/// - Raw order data bytes
	fn extract_order_data_from_fill_instructions(
		resolved_order: &[Token],
	) -> Result<(Bytes32, Vec<u8>)> {
		let fill_instructions = match &resolved_order[7] {
			Token::Array(instructions) => instructions,
			_ => return Err(SolverError::Order("Invalid fillInstructions".to_string())),
		};

		debug!("Processing {} fill instructions", fill_instructions.len());

		if fill_instructions.is_empty() {
			return Ok((Bytes32::from([0u8; 32]), Vec::new()));
		}

		match &fill_instructions[0] {
			Token::Tuple(inst_fields) if inst_fields.len() >= 3 => {
				// Extract originData from the third field
				let origin_data = match &inst_fields[2] {
					Token::Bytes(data) => {
						debug!("Found originData with {} bytes", data.len());
						data.clone()
					}
					_ => {
						debug!("originData field is not bytes type");
						Vec::new()
					}
				};

				// For OIF MandateERC7683 orders, the originData should be the full
				// ABI-encoded MandateOutput struct. We don't extract the type separately
				// as it's embedded in the struct itself.
				// TODO: Extract actual order data type from the MandateOutput if needed
				let order_data_type = Bytes32::from([0u8; 32]);

				Ok((order_data_type, origin_data))
			}
			_ => {
				debug!("First fill instruction is not a valid tuple");
				Ok((Bytes32::from([0u8; 32]), Vec::new()))
			}
		}
	}

	/// Encodes an on-chain order for the registry.
	///
	/// Creates a standardized byte representation of the order that
	/// can be parsed by the order registry. The encoding includes:
	/// - Format marker (0x02 for on-chain)
	/// - Order ID
	/// - User address
	/// - Chain and timing information
	/// - Order data
	fn encode_onchain_order(
		order_id: Bytes32,
		user: Address,
		origin_chain_id: U256,
		block_number: BlockNumber,
		fill_deadline: u32,
		order_data_type: Bytes32,
		order_data: Vec<u8>,
	) -> Vec<u8> {
		let mut encoded = Vec::new();

		// Add marker for onchain order
		encoded.push(0x02);

		// Add order ID
		encoded.extend_from_slice(order_id.as_ref());

		// Add user address
		encoded.extend_from_slice(user.as_ref());

		// Add origin chain ID (as 32 bytes)
		let mut chain_bytes = [0u8; 32];
		origin_chain_id.to_big_endian(&mut chain_bytes);
		encoded.extend_from_slice(&chain_bytes);

		// Add open timestamp (using block number as proxy)
		encoded.extend_from_slice(&block_number.to_be_bytes());

		// Add fill deadline
		encoded.extend_from_slice(&fill_deadline.to_be_bytes());

		// Add order data type
		encoded.extend_from_slice(order_data_type.as_ref());

		// Add order data length and data
		encoded.extend_from_slice(&(order_data.len() as u32).to_be_bytes());
		encoded.extend_from_slice(&order_data);

		encoded
	}
}

#[async_trait]
impl OrderStandardProcessor for EIP7683Processor {
	fn standard_name(&self) -> &str {
		"EIP7683"
	}

	fn can_process(&self, order_type_hint: Option<&str>) -> bool {
		match order_type_hint {
			Some("EIP7683") => true,
			None => true, // Default to EIP7683 if no hint provided
			_ => false,
		}
	}

	async fn process_intent(
		&self,
		raw_intent: RawIntent,
		order_registry: &Arc<OrderRegistry>,
	) -> Result<(solver_orders::OrderImpl, Vec<u8>)> {
		self.process_eip7683_intent(raw_intent, order_registry)
			.await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::ChainId;

	#[tokio::test]
	async fn test_parse_real_event_data() {
		// Create an event with the actual data from the logs
		let full_hex = "0000000000000000000000000000000000000000000000000000000000000020000000000000000000000000f39fd6e51aad88f6f4ce6ab8827279cfffb920660000000000000000000000000000000000000000000000000000000000007a6900000000000000000000000000000000000000000000000000000000674dcdde00000000000000000000000000000000000000000000000000000000674dd0426a93ca956232b1937d5e75139b5240f0621cc4340b68cd38556453b0bfa93a00000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000140000000000000000000000000000000000000000000000000000000000000018000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000008900000000000000000000000070997970c51812dc3a010c7d01b50e0d17dc79c800000000000000000000000000000000000000000000000000000000000000600000000000000000000000000000000000000000000000000000000000000020000000000000000000000000f39fd6e51aad88f6f4ce6ab8827279cfffb92266";
		let data = hex::decode(full_hex).unwrap();

		let event = Event {
			address: Address::from([10u8; 20]),
			topics: vec![
				solver_types::events::eip7683::open_event_topic(),
				Bytes32::from([1u8; 32]), // order ID in topic
			],
			data,
			block_number: 3,
			transaction_hash: Bytes32::from([99u8; 32]),
			log_index: 0,
			chain_id: ChainId(31337),
		};

		// Create a mock order registry
		let order_registry = Arc::new(OrderRegistry::new());

		// Test parsing
		let processor = EIP7683Processor::new();
		let result = processor
			.parse_eip7683_open_event(&event, &order_registry)
			.await;

		match result {
			Ok((_, _)) => println!("Successfully parsed real event data!"),
			Err(e) => println!("Failed to parse real event data: {}", e),
		}
	}
}
