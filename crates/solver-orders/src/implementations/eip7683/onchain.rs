//! Onchain order implementation.

use async_trait::async_trait;
use solver_types::{
	chains::ChainId,
	common::*,
	errors::{Result, SolverError},
	orders::{
		FillData, FillInstruction, Input, Order, OrderId, OrderSemantics, OrderStandard, Output,
	},
	standards::eip7683::{MandateOutput, OnchainCrossChainOrder, Output as EIP7683Output},
};
use std::any::Any;
use tracing::{debug, warn};

/// Parsed onchain order data
#[derive(Debug, Clone)]
struct ParsedOnchainOrder {
	destination_chain: ChainId,
	destination_contract: Address,
	origin_data: Vec<u8>,
}

/// Constants for ABI decoding
mod abi_constants {
	pub const OFFSET_SIZE: usize = 32;
	pub const BYTES32_SIZE: usize = 32;
	pub const MANDATE_OUTPUT_MIN_SIZE: usize = OFFSET_SIZE + 8 * BYTES32_SIZE; // offset + 8 fixed fields
}

/// Wrapper for onchain orders with additional context
#[derive(Debug, Clone)]
pub struct OnchainOrder {
	inner: OnchainCrossChainOrder,
	// Additional fields populated from the Open event
	order_id: OrderId,
	_user: Address,
	origin_chain_id: U256,
	open_timestamp: Timestamp,
}

impl OnchainOrder {
	pub fn new(inner: OnchainCrossChainOrder) -> Self {
		// In production, these would be populated from the Open event
		Self {
			inner,
			order_id: Bytes32::zero(),
			_user: Address::zero(),
			origin_chain_id: U256::from(1),
			open_timestamp: 0,
		}
	}

	/// Create from Open event data
	pub fn from_event(
		inner: OnchainCrossChainOrder,
		order_id: OrderId,
		user: Address,
		origin_chain_id: U256,
		open_timestamp: Timestamp,
	) -> Self {
		Self {
			inner,
			order_id,
			_user: user,
			origin_chain_id,
			open_timestamp,
		}
	}

	/// Get the inner order
	pub fn inner(&self) -> &OnchainCrossChainOrder {
		&self.inner
	}

	/// Parse order data to extract fill instructions
	fn parse_order_data(&self) -> Result<ParsedOnchainOrder> {
		// Validate order data is not empty
		if self.inner.order_data.is_empty() {
			return Err(SolverError::Order(format!(
				"Cannot create fill instructions: order_data is empty for order {}.",
				self.order_id
			)));
		}

		// Decode MandateOutput from the order data
		let mandate_output = self.decode_mandate_output()?;

		// Extract destination information
		let (destination_chain, destination_contract) =
			self.extract_destination_from_mandate(&mandate_output)?;

		debug!(
			"Extracted destination from MandateOutput: chain={}, contract={}",
			destination_chain.0, destination_contract
		);

		Ok(ParsedOnchainOrder {
			destination_chain,
			destination_contract,
			origin_data: self.inner.order_data.clone(),
		})
	}

	/// Decode MandateOutput struct from ABI-encoded data
	fn decode_mandate_output(&self) -> Result<MandateOutput> {
		use abi_constants::*;

		let data = &self.inner.order_data;

		// Minimum size check
		if data.len() < OFFSET_SIZE {
			return Err(SolverError::Order(format!(
				"Order {}: insufficient data for ABI decoding ({} bytes, need at least {})",
				self.order_id,
				data.len(),
				OFFSET_SIZE
			)));
		}

		// Read the offset to the actual data
		let offset = U256::from_big_endian(&data[0..OFFSET_SIZE]);

		// Validate offset is reasonable
		if offset != U256::from(OFFSET_SIZE) {
			return Err(SolverError::Order(format!(
				"Order {}: unexpected ABI offset {} (expected {})",
				self.order_id, offset, OFFSET_SIZE
			)));
		}

		let data_start = offset.as_usize();

		// Check if we have enough data for the fixed fields
		if data.len() < data_start + MANDATE_OUTPUT_MIN_SIZE - OFFSET_SIZE {
			return Err(SolverError::Order(format!(
                "Order {}: insufficient data for MandateOutput ({} bytes available after offset, need at least {})",
                self.order_id,
                data.len().saturating_sub(data_start),
                MANDATE_OUTPUT_MIN_SIZE - OFFSET_SIZE
            )));
		}

		// Decode fixed fields
		let mut pos = data_start;

		let oracle = Bytes32::from_slice(&data[pos..pos + BYTES32_SIZE]);
		pos += BYTES32_SIZE;

		let settler = Bytes32::from_slice(&data[pos..pos + BYTES32_SIZE]);
		pos += BYTES32_SIZE;

		let chain_id = U256::from_big_endian(&data[pos..pos + BYTES32_SIZE]);
		pos += BYTES32_SIZE;

		let token = Bytes32::from_slice(&data[pos..pos + BYTES32_SIZE]);
		pos += BYTES32_SIZE;

		let amount = U256::from_big_endian(&data[pos..pos + BYTES32_SIZE]);
		pos += BYTES32_SIZE;

		let recipient = Bytes32::from_slice(&data[pos..pos + BYTES32_SIZE]);
		pos += BYTES32_SIZE;

		// Read offsets for dynamic fields
		let call_offset = if data.len() >= pos + BYTES32_SIZE {
			U256::from_big_endian(&data[pos..pos + BYTES32_SIZE]).as_usize()
		} else {
			return Err(SolverError::Order(format!(
				"Order {}: missing call offset in MandateOutput",
				self.order_id
			)));
		};
		pos += BYTES32_SIZE;

		let context_offset = if data.len() >= pos + BYTES32_SIZE {
			U256::from_big_endian(&data[pos..pos + BYTES32_SIZE]).as_usize()
		} else {
			return Err(SolverError::Order(format!(
				"Order {}: missing context offset in MandateOutput",
				self.order_id
			)));
		};

		// Decode dynamic fields (call and context data)
		let call = self.decode_dynamic_bytes(data, data_start + call_offset)?;
		let context = self.decode_dynamic_bytes(data, data_start + context_offset)?;

		Ok(MandateOutput {
			oracle,
			settler,
			chain_id,
			token,
			amount,
			recipient,
			call,
			context,
		})
	}

	/// Decode dynamic bytes from ABI-encoded data
	fn decode_dynamic_bytes(&self, data: &[u8], offset: usize) -> Result<Vec<u8>> {
		use abi_constants::*;

		// Check if we have enough data for the length field
		if data.len() < offset + BYTES32_SIZE {
			return Err(SolverError::Order(format!(
				"Order {}: insufficient data for dynamic field at offset {}",
				self.order_id, offset
			)));
		}

		// Read the length
		let length = U256::from_big_endian(&data[offset..offset + BYTES32_SIZE]).as_usize();

		// Check if we have enough data for the actual bytes
		let data_start = offset + BYTES32_SIZE;
		if data.len() < data_start + length {
			return Err(SolverError::Order(format!(
				"Order {}: insufficient data for dynamic field content (need {} bytes, have {})",
				self.order_id,
				length,
				data.len().saturating_sub(data_start)
			)));
		}

		// Extract the bytes
		Ok(data[data_start..data_start + length].to_vec())
	}

	/// Extract destination chain and contract from MandateOutput
	fn extract_destination_from_mandate(
		&self,
		mandate: &MandateOutput,
	) -> Result<(ChainId, Address)> {
		// Validate and convert chain ID
		let destination_chain = if mandate.chain_id <= U256::from(u64::MAX) {
			ChainId(mandate.chain_id.as_u64())
		} else {
			return Err(SolverError::Order(format!(
				"Order {}: destination chain ID {} exceeds u64::MAX",
				self.order_id, mandate.chain_id
			)));
		};

		// Extract address from bytes32 settler field
		// The settler is stored as bytes32, with the address in the last 20 bytes
		let destination_contract = EIP7683Output::bytes32_to_address(mandate.settler)?;

		Ok((destination_chain, destination_contract))
	}
}

#[async_trait]
impl Order for OnchainOrder {
	fn id(&self) -> OrderId {
		self.order_id
	}

	fn standard(&self) -> OrderStandard {
		OrderStandard::EIP7683
	}

	fn origin_chain(&self) -> ChainId {
		// Safely convert U256 to u64 with bounds checking
		if self.origin_chain_id <= U256::from(u64::MAX) {
			ChainId(self.origin_chain_id.as_u64())
		} else {
			// This shouldn't happen for valid chain IDs, but handle it gracefully
			warn!(
				"Origin chain ID {} is too large for u64, defaulting to 1",
				self.origin_chain_id
			);
			ChainId(1)
		}
	}

	fn destination_chains(&self) -> Vec<ChainId> {
		// Parse destination chain from order data
		match self.parse_order_data() {
			Ok(parsed) => vec![parsed.destination_chain],
			Err(_) => vec![ChainId(1)], // Fallback to Ethereum mainnet
		}
	}

	fn created_at(&self) -> Timestamp {
		self.open_timestamp
	}

	fn expires_at(&self) -> Timestamp {
		self.inner.fill_deadline as u64
	}

	async fn validate(&self) -> Result<()> {
		let now = chrono::Utc::now().timestamp() as u32;

		if now > self.inner.fill_deadline {
			return Err(SolverError::OrderExpired {
				expired_at: self.inner.fill_deadline as u64,
			});
		}

		// Validate that order data can be parsed
		self.parse_order_data()?;

		Ok(())
	}

	fn semantics(&self) -> OrderSemantics {
		// Similar to gasless order
		OrderSemantics::Custom("EIP-7683".to_string())
	}

	async fn to_fill_instructions(&self) -> Result<Vec<FillInstruction>> {
		use tracing::debug;

		debug!(
			"Converting onchain order {} to fill instructions",
			self.order_id
		);

		// Parse the order data to extract fill details
		let parsed_order = self.parse_order_data()?;

		// Create fill instruction
		let fill_instruction = FillInstruction {
			destination_chain: parsed_order.destination_chain,
			destination_contract: parsed_order.destination_contract,
			fill_data: FillData::EIP7683 {
				order_id: self.order_id,
				origin_data: parsed_order.origin_data,
			},
		};

		Ok(vec![fill_instruction])
	}

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn user(&self) -> Address {
		self._user
	}

	fn inputs(&self) -> Result<Vec<Input>> {
		// Decode MandateOutput to extract inputs
		let mandate = self.decode_mandate_output()?;

		// Convert bytes32 token to address
		let token = EIP7683Output::bytes32_to_address(mandate.token)?;

		debug!(
			"Extracted input from MandateOutput: token={:?}, amount={}",
			token, mandate.amount
		);

		Ok(vec![Input {
			token,
			amount: mandate.amount,
		}])
	}

	fn outputs(&self) -> Result<Vec<Output>> {
		// Decode MandateOutput to extract outputs
		let mandate = self.decode_mandate_output()?;

		// Convert bytes32 fields to addresses
		let token = EIP7683Output::bytes32_to_address(mandate.token)?;
		let recipient = EIP7683Output::bytes32_to_address(mandate.recipient)?;

		// Validate chain ID
		let chain_id = if mandate.chain_id <= U256::from(u64::MAX) {
			ChainId(mandate.chain_id.as_u64())
		} else {
			return Err(SolverError::Order(format!(
				"Output chain ID {} exceeds u64::MAX",
				mandate.chain_id
			)));
		};

		debug!(
			"Extracted output from MandateOutput: token={:?}, recipient={:?}, amount={}, chain={}",
			token, recipient, mandate.amount, chain_id.0
		);

		Ok(vec![Output {
			token,
			amount: mandate.amount,
			recipient,
			chain_id,
		}])
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::orders::FillData;

	/// Helper to create valid order data using old format (deprecated)
	fn create_valid_order_data() -> Vec<u8> {
		// Create a proper MandateOutput for tests
		let mut oracle = [0u8; 32];
		oracle[31] = 1;

		let mut settler = [0u8; 32];
		settler[12..32].copy_from_slice(&[0x42u8; 20]); // Valid address in bytes32

		let chain_id = 137u64; // Polygon

		let mut token = [0u8; 32];
		token[31] = 2;

		let amount = U256::from(1000000u64);

		let mut recipient = [0u8; 32];
		recipient[12..32].copy_from_slice(&[0x99u8; 20]);

		create_mandate_output_order_data(oracle, settler, chain_id, token, amount, recipient)
	}

	/// Helper to create ABI-encoded MandateOutput for testing with empty call/context
	fn create_mandate_output_order_data(
		oracle: [u8; 32],
		settler: [u8; 32],
		chain_id: u64,
		token: [u8; 32],
		amount: U256,
		recipient: [u8; 32],
	) -> Vec<u8> {
		create_mandate_output_with_call_data(
			oracle,
			settler,
			chain_id,
			token,
			amount,
			recipient,
			vec![],
			vec![],
		)
	}

	/// Helper to create ABI-encoded MandateOutput with custom call and context data
	#[allow(clippy::too_many_arguments)]
	fn create_mandate_output_with_call_data(
		oracle: [u8; 32],
		settler: [u8; 32],
		chain_id: u64,
		token: [u8; 32],
		amount: U256,
		recipient: [u8; 32],
		call_data: Vec<u8>,
		context_data: Vec<u8>,
	) -> Vec<u8> {
		let mut data = Vec::new();

		// ABI encoding starts with offset pointer
		let mut offset = [0u8; 32];
		offset[31] = 0x20; // offset = 32 bytes
		data.extend_from_slice(&offset);

		// MandateOutput struct fields
		data.extend_from_slice(&oracle);
		data.extend_from_slice(&settler);

		// Chain ID as uint256
		let mut chain_bytes = [0u8; 32];
		U256::from(chain_id).to_big_endian(&mut chain_bytes);
		data.extend_from_slice(&chain_bytes);

		// Token, amount, recipient
		data.extend_from_slice(&token);
		let mut amount_bytes = [0u8; 32];
		amount.to_big_endian(&mut amount_bytes);
		data.extend_from_slice(&amount_bytes);
		data.extend_from_slice(&recipient);

		// Offsets for dynamic fields (call and context)
		// These offsets are relative to the start of the struct (after the main offset)
		let base_offset = 8 * 32; // 8 fields (6 fixed + 2 offset fields) * 32 bytes
		let mut call_offset = [0u8; 32];
		U256::from(base_offset).to_big_endian(&mut call_offset);
		data.extend_from_slice(&call_offset);

		// Context data starts after call data (length + padded content)
		// Calculate padded length for call data (round up to 32 bytes)
		let call_padded_len = call_data.len().div_ceil(32) * 32;
		let mut context_offset = [0u8; 32];
		U256::from(base_offset + 32 + call_padded_len).to_big_endian(&mut context_offset);
		data.extend_from_slice(&context_offset);

		// Dynamic data: call and context
		// Call data
		let mut call_len = [0u8; 32];
		U256::from(call_data.len()).to_big_endian(&mut call_len);
		data.extend_from_slice(&call_len);
		data.extend_from_slice(&call_data);
		// Pad call data to 32 bytes
		if call_data.len() % 32 != 0 {
			let padding = 32 - (call_data.len() % 32);
			data.extend_from_slice(&vec![0u8; padding]);
		}

		// Context data
		let mut context_len = [0u8; 32];
		U256::from(context_data.len()).to_big_endian(&mut context_len);
		data.extend_from_slice(&context_len);
		data.extend_from_slice(&context_data);
		// Pad context data to 32 bytes
		if context_data.len() % 32 != 0 {
			let padding = 32 - (context_data.len() % 32);
			data.extend_from_slice(&vec![0u8; padding]);
		}

		data
	}

	#[test]
	fn test_onchain_order_creation() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: order_data.clone(),
		};

		let order = OnchainOrder::new(inner.clone());
		assert_eq!(order.inner().fill_deadline, inner.fill_deadline);
		assert_eq!(order.inner().order_data, order_data);
		assert_eq!(order.order_id, Bytes32::zero());
	}

	#[test]
	fn test_onchain_order_from_event() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: order_data.clone(),
		};

		let order_id = Bytes32::from([1u8; 32]);
		let user = Address::from([2u8; 20]);
		let origin_chain_id = U256::from(1);
		let open_timestamp = 1000;

		let order = OnchainOrder::from_event(
			inner.clone(),
			order_id,
			user,
			origin_chain_id,
			open_timestamp,
		);

		assert_eq!(order.order_id, order_id);
		assert_eq!(order.origin_chain_id, origin_chain_id);
		assert_eq!(order.open_timestamp, open_timestamp);
	}

	#[test]
	fn test_parse_order_data_success() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: order_data.clone(),
		};

		let order = OnchainOrder::new(inner);
		let parsed = order.parse_order_data().unwrap();

		// Should parse destination from MandateOutput
		assert_eq!(parsed.destination_chain, ChainId(137));
		assert_eq!(parsed.destination_contract, Address::from([0x42u8; 20]));
		// The entire order_data becomes origin_data
		assert_eq!(parsed.origin_data, order_data);
	}

	#[test]
	fn test_parse_order_data_empty() {
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![],
		};

		let order = OnchainOrder::new(inner);
		let result = order.parse_order_data();
		assert!(result.is_err());

		let error_message = result.unwrap_err().to_string();
		assert!(error_message.contains("order_data is empty"));
	}

	#[test]
	fn test_parse_order_data_with_insufficient_data() {
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![0x01, 0x02, 0x03], // Non-empty but insufficient data
		};

		let order = OnchainOrder::new(inner);
		let result = order.parse_order_data();
		assert!(result.is_err());

		let error_message = result.unwrap_err().to_string();
		assert!(error_message.contains("insufficient data for ABI decoding"));
		assert!(error_message.contains("3 bytes")); // Shows data length
	}

	#[test]
	fn test_destination_chains() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data,
		};

		let order = OnchainOrder::new(inner);
		let chains = order.destination_chains();
		assert_eq!(chains.len(), 1);
		assert_eq!(chains[0], ChainId(137)); // Should parse from MandateOutput
	}

	#[test]
	fn test_destination_chains_with_empty_data() {
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![], // Empty data
		};

		let order = OnchainOrder::new(inner);
		let chains = order.destination_chains();
		// With empty data, parsing fails, so we fall back to origin chain
		assert_eq!(chains.len(), 1);
		assert_eq!(chains[0], ChainId(1)); // Fallback to origin chain
	}

	#[tokio::test]
	async fn test_validate_success() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: u32::MAX, // Far future
			order_data_type: Bytes32::zero(),
			order_data,
		};

		let order = OnchainOrder::new(inner);
		let result = order.validate().await;
		assert!(result.is_ok());
	}

	#[tokio::test]
	async fn test_validate_expired() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: 0, // Already expired
			order_data_type: Bytes32::zero(),
			order_data,
		};

		let order = OnchainOrder::new(inner);
		let result = order.validate().await;
		assert!(result.is_err());
		assert!(matches!(
			result.unwrap_err(),
			SolverError::OrderExpired { .. }
		));
	}

	#[tokio::test]
	async fn test_validate_with_empty_data() {
		let inner = OnchainCrossChainOrder {
			fill_deadline: u32::MAX,
			order_data_type: Bytes32::zero(),
			order_data: vec![], // Empty data should now fail validation
		};

		let order = OnchainOrder::new(inner);
		let result = order.validate().await;
		assert!(result.is_err());

		let error_message = result.unwrap_err().to_string();
		assert!(error_message.contains("order_data is empty"));
	}

	#[tokio::test]
	async fn test_to_fill_instructions() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: u32::MAX,
			order_data_type: Bytes32::zero(),
			order_data,
		};

		let order_id = Bytes32::from([1u8; 32]);
		let order = OnchainOrder::from_event(inner, order_id, Address::zero(), U256::from(1), 1000);

		let instructions = order.to_fill_instructions().await.unwrap();
		assert_eq!(instructions.len(), 1);

		let instruction = &instructions[0];
		assert_eq!(instruction.destination_chain, ChainId(137)); // Parsed from MandateOutput
		assert_eq!(
			instruction.destination_contract,
			Address::from([0x42u8; 20])
		);

		match &instruction.fill_data {
			FillData::EIP7683 {
				order_id: fill_order_id,
				origin_data,
			} => {
				assert_eq!(*fill_order_id, order_id);
				// Origin data should be the full MandateOutput encoded data
				assert_eq!(origin_data.len(), create_valid_order_data().len());
			}
			_ => panic!("Expected EIP7683 fill data"),
		}
	}

	#[tokio::test]
	async fn test_to_fill_instructions_empty_data() {
		let inner = OnchainCrossChainOrder {
			fill_deadline: u32::MAX,
			order_data_type: Bytes32::zero(),
			order_data: vec![], // Empty data
		};

		let order_id = Bytes32::from([1u8; 32]);
		let order = OnchainOrder::from_event(inner, order_id, Address::zero(), U256::from(1), 1000);

		let result = order.to_fill_instructions().await;
		assert!(result.is_err());

		let error_message = result.unwrap_err().to_string();
		assert!(error_message.contains("order_data is empty"));
	}

	#[test]
	fn test_order_trait_implementation() {
		let order_data = create_valid_order_data();
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data,
		};

		let order_id = Bytes32::from([1u8; 32]);
		let order =
			OnchainOrder::from_event(inner, order_id, Address::zero(), U256::from(42161), 1000);

		assert_eq!(order.id(), order_id);
		assert_eq!(order.standard(), OrderStandard::EIP7683);
		assert_eq!(order.origin_chain(), ChainId(42161));
		assert_eq!(order.created_at(), 1000);
		assert_eq!(order.expires_at(), 2000);
		assert_eq!(
			order.semantics(),
			OrderSemantics::Custom("EIP-7683".to_string())
		);
	}

	#[test]
	fn test_parse_mandate_output_valid() {
		// Create valid MandateOutput data
		let mut oracle = [0u8; 32];
		oracle[31] = 1;

		let mut settler = [0u8; 32];
		settler[12..32].copy_from_slice(&[0x42u8; 20]); // Valid address in bytes32

		let chain_id = 137u64; // Polygon

		let mut token = [0u8; 32];
		token[31] = 2;

		let amount = U256::from(1000000u64);

		let mut recipient = [0u8; 32];
		recipient[12..32].copy_from_slice(&[0x99u8; 20]);

		let order_data =
			create_mandate_output_order_data(oracle, settler, chain_id, token, amount, recipient);

		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data,
		};

		let order = OnchainOrder::new(inner);
		let result = order.parse_order_data().unwrap();

		// Verify correct parsing
		assert_eq!(result.destination_chain, ChainId(137));
		assert_eq!(result.destination_contract, Address::from([0x42u8; 20]));
		assert!(!result.origin_data.is_empty());
	}

	#[test]
	fn test_parse_mandate_output_large_chain_id() {
		// Create MandateOutput with chain ID that's too large for u64
		let oracle = [0u8; 32];
		let mut settler = [0u8; 32];
		settler[12..32].copy_from_slice(&[0x42u8; 20]);

		// Create a chain ID larger than u64::MAX
		let large_chain_id = U256::from_dec_str("18446744073709551616").unwrap(); // u64::MAX + 1

		let mut data = Vec::new();

		// ABI encoding starts with offset pointer
		let mut offset = [0u8; 32];
		offset[31] = 0x20; // offset = 32 bytes
		data.extend_from_slice(&offset);

		// MandateOutput struct fields
		data.extend_from_slice(&oracle);
		data.extend_from_slice(&settler);

		// Chain ID as uint256 (too large for u64)
		let mut chain_bytes = [0u8; 32];
		large_chain_id.to_big_endian(&mut chain_bytes);
		data.extend_from_slice(&chain_bytes);

		// Add remaining fixed fields
		data.extend_from_slice(&[0u8; 32]); // token
		data.extend_from_slice(&[0u8; 32]); // amount
		data.extend_from_slice(&[0u8; 32]); // recipient

		// Add offset fields for dynamic data
		let base_offset = 8 * 32; // 8 fields * 32 bytes
		let mut call_offset = [0u8; 32];
		U256::from(base_offset).to_big_endian(&mut call_offset);
		data.extend_from_slice(&call_offset);

		let mut context_offset = [0u8; 32];
		U256::from(base_offset + 32).to_big_endian(&mut context_offset);
		data.extend_from_slice(&context_offset);

		// Add empty dynamic data
		data.extend_from_slice(&[0u8; 32]); // call length = 0
		data.extend_from_slice(&[0u8; 32]); // context length = 0

		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: data,
		};

		let order = OnchainOrder::new(inner);
		let result = order.parse_order_data();

		// Should fail with proper error
		assert!(result.is_err());
		let error_message = result.unwrap_err().to_string();
		assert!(error_message.contains("exceeds u64::MAX"));
	}

	#[test]
	fn test_parse_mandate_output_insufficient_data() {
		// Create MandateOutput with insufficient data
		let mut data = Vec::new();

		// Add offset
		let mut offset = [0u8; 32];
		offset[31] = 0x20;
		data.extend_from_slice(&offset);

		// Add only partial data (not enough for full struct)
		data.extend_from_slice(&[0u8; 32]); // oracle
		data.extend_from_slice(&[0u8; 16]); // partial settler - not enough!

		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: data,
		};

		let order = OnchainOrder::new(inner);
		let result = order.parse_order_data();

		// Should fail due to insufficient data
		assert!(result.is_err());
		let error_message = result.unwrap_err().to_string();
		assert!(error_message.contains("insufficient data for MandateOutput"));
	}

	#[test]
	fn test_parse_mandate_output_no_offset() {
		// Create data without proper ABI offset
		let data = vec![0u8; 16]; // Too small for proper ABI encoding

		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: data,
		};

		let order = OnchainOrder::new(inner);
		let result = order.parse_order_data();

		// Should fail due to invalid format
		assert!(result.is_err());
		let error_message = result.unwrap_err().to_string();
		assert!(error_message.contains("insufficient data for ABI decoding"));
	}

	#[test]
	fn test_origin_chain_with_large_chain_id() {
		let inner = OnchainCrossChainOrder {
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![1, 2, 3],
		};

		// Create order with chain ID larger than u64::MAX
		let large_chain_id = U256::from_dec_str("99999999999999999999999999999").unwrap();
		let order = OnchainOrder::from_event(
			inner,
			Bytes32::zero(),
			Address::zero(),
			large_chain_id,
			1000,
		);

		// Should handle gracefully with warning (defaults to 1)
		assert_eq!(order.origin_chain(), ChainId(1));
	}
}
