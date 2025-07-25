use async_trait::async_trait;
use solver_types::{FillProof, Order, TransactionHash};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettlementError {
	#[error("Validation failed: {0}")]
	ValidationFailed(String),
	#[error("Invalid proof")]
	InvalidProof,
	#[error("Fill does not match order requirements")]
	FillMismatch,
}

#[async_trait]
pub trait SettlementInterface: Send + Sync {
	/// Validates a fill transaction and extracts proof data needed for claiming.
	///
	/// This method should:
	/// 1. Fetch the transaction receipt using the tx_hash
	/// 2. Parse logs/events to extract fill details
	/// 3. Verify the fill satisfies the order requirements
	/// 4. Build a FillProof containing all data needed for claiming
	async fn validate_fill(
		&self,
		order: &Order,
		tx_hash: &TransactionHash,
	) -> Result<FillProof, SettlementError>;

	/// Checks if the solver can claim rewards for this fill.
	///
	/// This method should check on-chain conditions such as:
	/// - Time delays or challenge periods
	/// - Oracle attestations if required
	/// - Solver permissions
	/// - Reward availability
	async fn can_claim(&self, order: &Order, fill_proof: &FillProof) -> bool;
}

pub struct SettlementService {
	implementations: HashMap<String, Box<dyn SettlementInterface>>,
}

impl SettlementService {
	pub fn new(implementations: HashMap<String, Box<dyn SettlementInterface>>) -> Self {
		Self { implementations }
	}

	pub async fn validate_fill(
		&self,
		order: &Order,
		tx_hash: &TransactionHash,
	) -> Result<FillProof, SettlementError> {
		let implementation = self
			.implementations
			.get(&order.standard)
			.ok_or_else(|| SettlementError::ValidationFailed("Unknown standard".into()))?;

		implementation.validate_fill(order, tx_hash).await
	}

	pub async fn can_claim(&self, order: &Order, fill_proof: &FillProof) -> bool {
		if let Some(implementation) = self.implementations.get(&order.standard) {
			implementation.can_claim(order, fill_proof).await
		} else {
			false
		}
	}
}
