//! Arbitrum cross-chain broadcaster settlement strategy.

use async_trait::async_trait;
use ethers::{
	abi::{encode, Token},
	utils::id,
};
use serde::{Deserialize, Serialize};
use solver_config::serde_helpers::{
	deserialize_chain_id_map_generic, serialize_chain_id_map_generic,
};
use solver_types::{
	chains::ChainId,
	common::{Address, TxHash},
	errors::{Result, SolverError},
	orders::{Order, OrderId},
};
use std::collections::HashMap;
use tracing::{debug, info};

use crate::{implementations::SettlementStrategy, types::Attestation};

/// Arbitrum broadcaster configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrumConfig {
	/// Broadcaster contract addresses by chain
	#[serde(
		deserialize_with = "deserialize_chain_id_map_generic",
		serialize_with = "serialize_chain_id_map_generic"
	)]
	pub broadcaster_addresses: HashMap<ChainId, Address>,
	/// Settler contract addresses by chain
	#[serde(
		deserialize_with = "deserialize_chain_id_map_generic",
		serialize_with = "serialize_chain_id_map_generic"
	)]
	pub settler_addresses: HashMap<ChainId, Address>,
	/// Polling interval for attestations
	pub poll_interval_secs: u64,
	/// Maximum wait time for attestations
	pub max_wait_secs: u64,
}

/// Arbitrum cross-chain broadcaster strategy
#[derive(Clone)]
pub struct ArbitrumBroadcasterStrategy {
	config: ArbitrumConfig,
}

impl ArbitrumBroadcasterStrategy {
	pub fn new(config: ArbitrumConfig) -> Self {
		Self { config }
	}

	/// Get broadcaster address for a chain
	fn get_broadcaster(&self, chain_id: &ChainId) -> Result<Address> {
		self.config
			.broadcaster_addresses
			.get(chain_id)
			.copied()
			.ok_or_else(|| {
				SolverError::Settlement(format!("No broadcaster configured for chain {}", chain_id))
			})
	}

	/// Get settler address for a chain
	fn get_settler(&self, chain_id: &ChainId) -> Result<Address> {
		self.config
			.settler_addresses
			.get(chain_id)
			.copied()
			.ok_or_else(|| {
				SolverError::Settlement(format!("No settler configured for chain {}", chain_id))
			})
	}

	/// Build attestation query call
	fn build_attestation_query(&self, order_id: OrderId, fill_tx: TxHash) -> Vec<u8> {
		// Function signature: isAttested(bytes32,bytes32)
		let function_selector = id("isAttested(bytes32,bytes32)");
		let mut calldata = function_selector[..4].to_vec();

		// Encode parameters
		let tokens = vec![
			Token::FixedBytes(order_id.0.to_vec()),
			Token::FixedBytes(fill_tx.0.to_vec()),
		];
		calldata.extend_from_slice(&encode(&tokens));

		calldata
	}

	/// Build settlement claim call data
	fn build_claim_calldata(
		&self,
		order: &dyn Order,
		attestation: &Attestation,
	) -> Result<Vec<u8>> {
		// Function signature: claim(bytes32,bytes)
		let function_selector = id("claim(bytes32,bytes)");
		let mut calldata = function_selector[..4].to_vec();

		// Encode parameters
		let tokens = vec![
			Token::FixedBytes(order.id().0.to_vec()),
			Token::Bytes(attestation.data.clone()),
		];
		calldata.extend_from_slice(&encode(&tokens));

		Ok(calldata)
	}
}

#[async_trait]
impl SettlementStrategy for ArbitrumBroadcasterStrategy {
	fn name(&self) -> &str {
		"ArbitrumBroadcaster"
	}

	async fn check_attestation(
		&self,
		order_id: OrderId,
		fill_tx: TxHash,
		_fill_timestamp: u64,
		origin_chain: ChainId,
		_destination_chain: ChainId,
	) -> Result<Option<Attestation>> {
		debug!(
			"Checking attestation for order {} on chain {}",
			order_id, origin_chain
		);

		// Get broadcaster address on origin chain
		let _broadcaster = self.get_broadcaster(&origin_chain)?;

		// Query the broadcaster contract
		let _calldata = self.build_attestation_query(order_id, fill_tx);

		// This would use the chain adapter to make the call
		// For now, returning a placeholder

		// In real implementation:
		// 1. Call broadcaster contract with (order_id, fill_tx)
		// 2. If attested, fetch the attestation data
		// 3. Return Some(Attestation) or None

		Ok(None) // Placeholder
	}

	async fn claim_settlement(
		&self,
		order: &dyn Order,
		attestation: Attestation,
	) -> Result<TxHash> {
		info!(
			"Claiming settlement for order {} on chain {}",
			order.id(),
			order.origin_chain()
		);

		let _settler = self.get_settler(&order.origin_chain())?;
		let _calldata = self.build_claim_calldata(order, &attestation)?;

		// This would use the delivery service to submit the transaction
		// For now, returning a placeholder

		Ok(TxHash::zero()) // Placeholder
	}

	async fn estimate_attestation_time(&self) -> std::time::Duration {
		// Arbitrum broadcaster typically takes 2-5 minutes
		std::time::Duration::from_secs(self.config.poll_interval_secs * 3)
	}

	async fn is_claimed(&self, _order_id: OrderId, _origin_chain: ChainId) -> Result<bool> {
		// Query settler contract to check if already claimed
		// Function: isClaimed(bytes32)

		Ok(false) // Placeholder
	}
}
