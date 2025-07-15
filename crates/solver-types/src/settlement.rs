//! Settlement-related types and traits.

use crate::{common::*, errors::Result, orders::OrderId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Settlement type identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SettlementType {
	ArbitrumCrossChainBroadcaster,
	OptimisticOracle,
	StorageProof,
	Custom(String),
}

/// Settlement strategy trait
#[async_trait]
pub trait SettlementStrategy: Send + Sync {
	/// Get the settlement type
	fn settlement_type(&self) -> SettlementType;

	/// Check if this strategy can settle the given order
	async fn can_settle(&self, order: &dyn crate::orders::Order) -> Result<bool>;

	/// Generate proof of fill
	async fn generate_proof(&self, fill: &Fill) -> Result<()>;

	/// Submit proof to claim settlement
	async fn claim_settlement(&self, fill: &Fill) -> Result<TxHash>;

	/// Check if settlement is ready to be claimed
	async fn is_claimable(&self, fill: &Fill) -> Result<bool>;

	/// Get estimated time until settlement can be claimed
	async fn time_to_settlement(&self, fill: &Fill) -> Result<std::time::Duration>;
}

/// Fill execution details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fill {
	pub order_id: OrderId,
	pub solver: Address,
	pub tx_hash: TxHash,
	pub block_number: BlockNumber,
	pub timestamp: Timestamp,
	pub outputs: Vec<(Address, U256, Address)>, // (token, amount, recipient)
	pub gas_used: U256,
}
