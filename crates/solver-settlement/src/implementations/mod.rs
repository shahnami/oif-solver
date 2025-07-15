//! Settlement strategy implementations.

pub mod arbitrum;
pub mod direct;

pub use arbitrum::ArbitrumBroadcasterStrategy;
pub use direct::DirectSettlementStrategy;

use crate::types::Attestation;
use crate::types::SettlementType;
use async_trait::async_trait;
use solver_types::errors::Result;
use solver_types::{
	chains::ChainId,
	common::TxHash,
	orders::{Order, OrderId},
};
use std::sync::Arc;

/// Settlement strategy implementation wrapper
#[derive(Clone)]
pub enum SettlementStrategyImpl {
	ArbitrumBroadcaster(ArbitrumBroadcasterStrategy),
	Direct(DirectSettlementStrategy),
}

#[async_trait]
impl SettlementStrategy for SettlementStrategyImpl {
	fn name(&self) -> &str {
		match self {
			SettlementStrategyImpl::ArbitrumBroadcaster(s) => s.name(),
			SettlementStrategyImpl::Direct(s) => s.name(),
		}
	}

	async fn check_attestation(
		&self,
		order_id: OrderId,
		fill_tx: TxHash,
		fill_timestamp: u64,
		origin_chain: ChainId,
		destination_chain: ChainId,
	) -> Result<Option<Attestation>> {
		match self {
			SettlementStrategyImpl::ArbitrumBroadcaster(s) => {
				s.check_attestation(
					order_id,
					fill_tx,
					fill_timestamp,
					origin_chain,
					destination_chain,
				)
				.await
			}
			SettlementStrategyImpl::Direct(s) => {
				s.check_attestation(
					order_id,
					fill_tx,
					fill_timestamp,
					origin_chain,
					destination_chain,
				)
				.await
			}
		}
	}

	async fn claim_settlement(
		&self,
		order: &dyn Order,
		attestation: Attestation,
	) -> Result<TxHash> {
		match self {
			SettlementStrategyImpl::ArbitrumBroadcaster(s) => {
				s.claim_settlement(order, attestation).await
			}
			SettlementStrategyImpl::Direct(s) => s.claim_settlement(order, attestation).await,
		}
	}

	async fn estimate_attestation_time(&self) -> std::time::Duration {
		match self {
			SettlementStrategyImpl::ArbitrumBroadcaster(s) => s.estimate_attestation_time().await,
			SettlementStrategyImpl::Direct(s) => s.estimate_attestation_time().await,
		}
	}

	async fn is_claimed(&self, order_id: OrderId, origin_chain: ChainId) -> Result<bool> {
		match self {
			SettlementStrategyImpl::ArbitrumBroadcaster(s) => {
				s.is_claimed(order_id, origin_chain).await
			}
			SettlementStrategyImpl::Direct(s) => s.is_claimed(order_id, origin_chain).await,
		}
	}
}

/// Create a settlement strategy based on type
pub fn create_strategy(
	settlement_type: SettlementType,
	config: serde_json::Value,
	chain_registry: Arc<solver_chains::ChainRegistry>,
	delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
) -> Result<Arc<SettlementStrategyImpl>> {
	match settlement_type {
		SettlementType::ArbitrumBroadcaster => {
			let config: arbitrum::ArbitrumConfig = serde_json::from_value(config)
				.map_err(|e| solver_types::errors::SolverError::Other(e.into()))?;
			Ok(Arc::new(SettlementStrategyImpl::ArbitrumBroadcaster(
				ArbitrumBroadcasterStrategy::new(config),
			)))
		}
		SettlementType::Direct => {
			let config: crate::types::DirectConfig = serde_json::from_value(config)
				.map_err(|e| solver_types::errors::SolverError::Other(e.into()))?;
			Ok(Arc::new(SettlementStrategyImpl::Direct(
				DirectSettlementStrategy::new(config, chain_registry, delivery_service),
			)))
		}
	}
}

/// Settlement strategy trait
#[async_trait]
pub trait SettlementStrategy: Send + Sync {
	/// Get strategy name
	fn name(&self) -> &str;

	/// Check if attestation is available for a fill
	async fn check_attestation(
		&self,
		order_id: OrderId,
		fill_tx: TxHash,
		fill_timestamp: u64,
		origin_chain: ChainId,
		destination_chain: ChainId,
	) -> Result<Option<Attestation>>;

	/// Claim settlement using attestation
	async fn claim_settlement(&self, order: &dyn Order, attestation: Attestation)
		-> Result<TxHash>;

	/// Get estimated time until attestation is available
	async fn estimate_attestation_time(&self) -> std::time::Duration {
		std::time::Duration::from_secs(180) // 3 minutes default
	}

	/// Check if a settlement has already been claimed
	async fn is_claimed(&self, order_id: OrderId, origin_chain: ChainId) -> Result<bool>;
}
