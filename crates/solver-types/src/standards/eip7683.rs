//! EIP-7683 specific types and implementations

use crate::{
	chains::ChainId,
	common::*,
	errors::Result,
	orders::{FillData, FillInstruction, Order, OrderId, OrderStandard},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// EIP-7683: Gasless cross-chain order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaslessCrossChainOrder {
	pub origin_settler: Address,
	pub user: Address,
	pub nonce: U256,
	pub origin_chain_id: U256,
	pub open_deadline: u32,
	pub fill_deadline: u32,
	pub order_data_type: Bytes32,
	pub order_data: Vec<u8>,
}

/// EIP-7683: Onchain cross-chain order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnchainCrossChainOrder {
	pub fill_deadline: u32,
	pub order_data_type: Bytes32,
	pub order_data: Vec<u8>,
}

/// EIP-7683: Output specification (with bytes32 for cross-chain)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
	pub token: Bytes32,
	pub amount: U256,
	pub recipient: Bytes32,
	pub chain_id: U256,
}

/// EIP-7683 MandateOutput struct for OIF StandardOrder.
///
/// This represents the output structure used in OIF MandateERC7683 orders.
/// IMPORTANT: Field order must match Solidity MandateOutput struct exactly.
///
/// # Fields
/// - `oracle`: Oracle address as bytes32
/// - `settler`: Settler contract address as bytes32  
/// - `chain_id`: Destination chain ID
/// - `token`: Token address as bytes32
/// - `amount`: Token amount
/// - `recipient`: Recipient address as bytes32
/// - `call`: Call data for custom actions
/// - `context`: Context data for the order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MandateOutput {
	/// Oracle address as bytes32
	pub oracle: Bytes32,
	/// Settler contract address as bytes32
	pub settler: Bytes32,
	/// Destination chain ID
	pub chain_id: U256,
	/// Token address as bytes32
	pub token: Bytes32,
	/// Token amount
	pub amount: U256,
	/// Recipient address as bytes32
	pub recipient: Bytes32,
	/// Call data
	pub call: Vec<u8>,
	/// Context data
	pub context: Vec<u8>,
}

/// Input specification for OIF StandardOrder
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardOrderInput {
	/// Token address as uint256
	pub token: U256,
	/// Token amount
	pub amount: U256,
}

/// EIP-7683 StandardOrder struct for OIF
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardOrder {
	/// Order creator address
	pub user: Address,
	/// Unique order nonce
	pub nonce: U256,
	/// Origin chain ID
	pub origin_chain_id: U256,
	/// Order expiration timestamp
	pub expires: u32,
	/// Fill deadline timestamp
	pub fill_deadline: u32,
	/// Local oracle address
	pub local_oracle: Address,
	/// Array of input tokens/amounts
	pub inputs: Vec<StandardOrderInput>,
	/// Array of output tokens/amounts
	pub outputs: Vec<MandateOutput>,
}

/// Helper functions for StandardOrder
impl StandardOrder {
	/// Convert EVM address to bytes32 format (right-padded)
	pub fn address_to_bytes32(addr: Address) -> Bytes32 {
		let mut bytes = [0u8; 32];
		bytes[12..].copy_from_slice(addr.as_bytes());
		Bytes32::from(bytes)
	}

	/// Convert bytes32 to EVM address (extract last 20 bytes)
	pub fn bytes32_to_address(bytes: Bytes32) -> Result<Address> {
		use crate::errors::SolverError;
		if bytes.as_bytes()[..12].iter().any(|&b| b != 0) {
			return Err(SolverError::Order(
				"Invalid EVM address in bytes32".to_string(),
			));
		}
		Ok(Address::from_slice(&bytes.as_bytes()[12..]))
	}
}

/// EIP-7683: Resolved order format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedCrossChainOrder {
	pub user: Address,
	pub origin_chain_id: U256,
	pub open_deadline: u32,
	pub fill_deadline: u32,
	pub order_id: Bytes32,
	pub max_spent: Vec<Output>,
	pub min_received: Vec<Output>,
	pub fill_instructions: Vec<EIP7683FillInstruction>,
}

/// EIP-7683 specific fill instruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EIP7683FillInstruction {
	pub destination_chain_id: U256,
	pub destination_settler: Bytes32,
	pub origin_data: Vec<u8>,
}

/// Trait for EIP-7683 orders
#[async_trait]
pub trait EIP7683Order: Order {
	/// Resolve to standard EIP-7683 format
	async fn resolve(&self, filler_data: Option<&[u8]>) -> Result<ResolvedCrossChainOrder>;
}

/// Implementation for GaslessCrossChainOrder
#[async_trait]
impl Order for GaslessCrossChainOrder {
	fn id(&self) -> OrderId {
		// Compute order ID from order data
		// This would typically use EIP-712 hashing
		todo!("Implement EIP-712 order hashing")
	}

	fn standard(&self) -> OrderStandard {
		OrderStandard::EIP7683
	}

	fn origin_chain(&self) -> ChainId {
		ChainId(self.origin_chain_id.as_u64())
	}

	fn destination_chains(&self) -> Vec<ChainId> {
		// Parse from order_data
		todo!("Parse destination chains from order data")
	}

	fn created_at(&self) -> Timestamp {
		// Could be derived from open_deadline or order_data
		0
	}

	fn expires_at(&self) -> Timestamp {
		self.fill_deadline as u64
	}

	async fn validate(&self) -> Result<()> {
		// Validate deadlines, signatures, etc.
		Ok(())
	}

	async fn to_fill_instructions(&self) -> Result<Vec<FillInstruction>> {
		// Convert EIP-7683 specific instructions to generic format
		let resolved = self.resolve(None).await?;

		resolved
			.fill_instructions
			.into_iter()
			.map(|inst| {
				Ok(FillInstruction {
					destination_chain: ChainId(inst.destination_chain_id.as_u64()),
					destination_contract: Output::bytes32_to_address(inst.destination_settler)?,
					fill_data: FillData::EIP7683 {
						order_id: resolved.order_id,
						origin_data: inst.origin_data,
					},
				})
			})
			.collect()
	}

	fn as_any(&self) -> &dyn std::any::Any {
		self
	}

	fn user(&self) -> Address {
		self.user
	}

	fn inputs(&self) -> Result<Vec<crate::orders::Input>> {
		// For gasless orders, we need to parse the order_data
		// This would typically be decoded based on order_data_type
		Err(crate::errors::SolverError::NotImplemented(
			"Input parsing for raw GaslessCrossChainOrder not implemented".to_string(),
		))
	}

	fn outputs(&self) -> Result<Vec<crate::orders::Output>> {
		// For gasless orders, we need to parse the order_data
		// This would typically be decoded based on order_data_type
		Err(crate::errors::SolverError::NotImplemented(
			"Output parsing for raw GaslessCrossChainOrder not implemented".to_string(),
		))
	}
}

#[async_trait]
impl EIP7683Order for GaslessCrossChainOrder {
	async fn resolve(&self, _filler_data: Option<&[u8]>) -> Result<ResolvedCrossChainOrder> {
		// Implementation would decode order_data and construct resolved order
		todo!("Implement order resolution")
	}
}

// Helper functions for bytes32 conversion
impl Output {
	pub fn address_to_bytes32(addr: Address) -> Bytes32 {
		let mut bytes = [0u8; 32];
		bytes[12..].copy_from_slice(addr.as_bytes());
		Bytes32::from(bytes)
	}

	pub fn bytes32_to_address(bytes: Bytes32) -> Result<Address> {
		use crate::errors::SolverError;
		if bytes.as_bytes()[..12].iter().any(|&b| b != 0) {
			return Err(SolverError::Order(
				"Invalid EVM address in bytes32".to_string(),
			));
		}
		Ok(Address::from_slice(&bytes.as_bytes()[12..]))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_address_bytes32_conversion() {
		let addr = Address::from([42u8; 20]);
		let bytes32 = Output::address_to_bytes32(addr);

		// First 12 bytes should be zero
		assert!(bytes32.as_bytes()[..12].iter().all(|&b| b == 0));
		// Last 20 bytes should match address
		assert_eq!(&bytes32.as_bytes()[12..], addr.as_bytes());

		// Round trip
		let recovered = Output::bytes32_to_address(bytes32).unwrap();
		assert_eq!(recovered, addr);
	}

	#[test]
	fn test_invalid_bytes32_to_address() {
		let mut bytes = [0u8; 32];
		bytes[0] = 1; // Non-zero in first 12 bytes
		let bytes32 = Bytes32::from(bytes);

		let result = Output::bytes32_to_address(bytes32);
		assert!(result.is_err());
	}

	#[test]
	fn test_gasless_order_creation() {
		let order = GaslessCrossChainOrder {
			origin_settler: Address::zero(),
			user: Address::from([1u8; 20]),
			nonce: U256::from(1),
			origin_chain_id: U256::from(1),
			open_deadline: 1000,
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![1, 2, 3],
		};

		assert_eq!(order.user, Address::from([1u8; 20]));
		assert_eq!(order.nonce, U256::from(1));
		assert_eq!(order.order_data, vec![1, 2, 3]);
	}
}
