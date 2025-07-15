//! Settlement types and errors.

use serde::{Deserialize, Serialize};
use solver_types::{
	chains::ChainId,
	common::{Address, Bytes32, TxHash, U256},
	orders::OrderId,
};

/// Settlement mechanism type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettlementType {
	/// Arbitrum's cross-chain broadcaster
	ArbitrumBroadcaster,
	/// Direct on-chain settlement (for testing)
	Direct,
}

/// Status of a settlement
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SettlementStatus {
	/// Order has been filled, awaiting attestation
	AwaitingAttestation { fill_tx: TxHash, filled_at: u64 },
	/// Attestation available, ready to claim
	ReadyToClaim {
		attestation_block: u64,
		attestation_data: Vec<u8>,
	},
	/// Settlement claimed, awaiting confirmation
	Claiming { claim_tx: TxHash, submitted_at: u64 },
	/// Settlement completed
	Completed {
		claim_tx: TxHash,
		completed_at: u64,
		amount_claimed: U256,
	},
	/// Settlement failed
	Failed { reason: String, can_retry: bool },
}

/// Settlement tracking data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementData {
	pub order_id: OrderId,
	pub origin_chain: ChainId,
	pub destination_chain: ChainId,
	pub settler_address: Address,
	pub status: SettlementStatus,
	pub settlement_type: SettlementType,
	pub created_at: u64,
	pub updated_at: u64,
	pub attempts: u32,
	pub fill_timestamp: Option<u64>,
}

/// Attestation data from oracle/broadcaster
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attestation {
	pub order_id: OrderId,
	pub fill_hash: Bytes32,
	pub solver: Address,
	pub timestamp: u64,
	pub data: Vec<u8>, // Encoded attestation data
	pub signature: Option<Vec<u8>>,
}

/// Configuration for Direct settlement strategy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectConfig {
	/// Settler contract addresses per chain
	pub settler_addresses: std::collections::HashMap<ChainId, Address>,
	/// Gas limit for settlement transactions
	pub gas_limit: Option<u64>,
	/// Gas multiplier for buffer
	pub gas_multiplier: Option<f64>,
	/// Optional solver address override (defaults to delivery service address)
	pub solver_address: Option<Address>,
	/// Allocator address for signatures (defaults to zero address for AlwaysOKAllocator)
	pub allocator_address: Option<Address>,
	/// Local oracle address (defaults to zero address)
	pub oracle_address: Option<Address>,
	/// Default expiry duration in seconds (defaults to 1 hour)
	pub default_expiry_duration: u64,
}

impl Default for DirectConfig {
	fn default() -> Self {
		Self {
			settler_addresses: std::collections::HashMap::new(),
			gas_limit: Some(300_000),
			gas_multiplier: Some(1.2),
			solver_address: None,
			allocator_address: None,
			oracle_address: None,
			default_expiry_duration: 3600, // 1 hour
		}
	}
}

// Re-export types from solver_types for convenience
pub use solver_types::standards::eip7683::{
	MandateOutput as Output, StandardOrder, StandardOrderInput as Input,
};

/// Combined signatures for EIP-7683 settlement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementSignatures {
	/// Sponsor signature (EIP-712)
	pub sponsor_signature: Vec<u8>,
	/// Allocator signature (EIP-712, may be empty for AlwaysOKAllocator)
	pub allocator_signature: Vec<u8>,
}

impl SettlementSignatures {
	/// Combine signatures into single bytes array for contract call
	pub fn to_bytes(&self) -> Vec<u8> {
		let mut combined = Vec::new();
		combined.extend_from_slice(&self.sponsor_signature);
		combined.extend_from_slice(&self.allocator_signature);
		combined
	}
}

#[derive(Debug, thiserror::Error)]
pub enum SettlementError {
	#[error("Attestation not yet available")]
	AttestationNotReady,

	#[error("Settlement already claimed")]
	AlreadyClaimed,

	#[error("Invalid attestation: {0}")]
	InvalidAttestation(String),

	#[error("Chain error: {0}")]
	ChainError(String),

	#[error("Configuration error: {0}")]
	ConfigError(String),

	#[error("Order data extraction failed: {0}")]
	OrderDataExtractionFailed(String),

	#[error("Signature generation failed: {0}")]
	SignatureGenerationFailed(String),

	#[error("Parameter construction failed: {0}")]
	ParameterConstructionFailed(String),

	#[error("EIP-7683 validation failed: {0}")]
	Eip7683ValidationFailed(String),
}
