use alloy::{
	primitives::{Address as AlloyAddress, FixedBytes},
	providers::{Provider, RootProvider},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solver_settlement::{SettlementError, SettlementInterface};
use solver_types::{FillProof, Order, TransactionHash};

/// Direct settlement implementation
pub struct DirectSettlement {
	provider: alloy::providers::RootProvider<alloy::transports::http::Http<reqwest::Client>>,
	_oracle_address: AlloyAddress,
	min_confirmations: u32,
	dispute_period_seconds: u64,
}

/// EIP-7683 specific order data (for deserialization)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Eip7683OrderData {
	order_id: [u8; 32],
	user: String,
	origin_chain_id: u64,
	destination_chain_id: u64,
}

impl DirectSettlement {
	pub async fn new(
		rpc_url: &str,
		oracle_address: String,
		min_confirmations: u32,
		dispute_period_seconds: u64,
	) -> Result<Self, SettlementError> {
		// Create provider
		let provider =
			RootProvider::new_http(rpc_url.parse().map_err(|e| {
				SettlementError::ValidationFailed(format!("Invalid RPC URL: {}", e))
			})?);

		// Parse oracle address
		let oracle = oracle_address.parse::<AlloyAddress>().map_err(|e| {
			SettlementError::ValidationFailed(format!("Invalid oracle address: {}", e))
		})?;

		Ok(Self {
			provider,
			_oracle_address: oracle,
			min_confirmations,
			dispute_period_seconds,
		})
	}
}

#[async_trait]
impl SettlementInterface for DirectSettlement {
	async fn validate_fill(
		&self,
		order: &Order,
		tx_hash: &TransactionHash,
	) -> Result<FillProof, SettlementError> {
		// Convert tx hash
		let hash = FixedBytes::<32>::from_slice(&tx_hash.0);

		// Get transaction receipt
		let receipt = self
			.provider
			.get_transaction_receipt(hash)
			.await
			.map_err(|e| {
				SettlementError::ValidationFailed(format!("Failed to get receipt: {}", e))
			})?
			.ok_or_else(|| {
				SettlementError::ValidationFailed("Transaction not found".to_string())
			})?;

		// Check if transaction was successful
		if !receipt.status() {
			return Err(SettlementError::ValidationFailed(
				"Transaction failed".to_string(),
			));
		}

		// Check confirmations
		let current_block = self.provider.get_block_number().await.map_err(|e| {
			SettlementError::ValidationFailed(format!("Failed to get block number: {}", e))
		})?;

		let tx_block = receipt.block_number.unwrap_or(0);
		let confirmations = current_block.saturating_sub(tx_block);

		if confirmations < self.min_confirmations as u64 {
			return Err(SettlementError::ValidationFailed(format!(
				"Insufficient confirmations: {} < {}",
				confirmations, self.min_confirmations
			)));
		}

		// Parse order data to get order ID
		let order_data: Eip7683OrderData =
			serde_json::from_value(order.data.clone()).map_err(|e| {
				SettlementError::ValidationFailed(format!("Failed to parse order data: {}", e))
			})?;

		// In production, would parse logs to find the fill event
		// and extract attestation data from oracle
		// For now, create a simple proof

		Ok(FillProof {
			tx_hash: tx_hash.clone(),
			block_number: tx_block,
			attestation_data: Some(order_data.order_id.to_vec()),
		})
	}

	async fn can_claim(&self, _order: &Order, fill_proof: &FillProof) -> bool {
		// Check if dispute period has passed
		let current_block = match self.provider.get_block_number().await {
			Ok(block) => block,
			Err(_) => return false,
		};

		// Estimate blocks for dispute period (assuming ~12 second blocks)
		let blocks_per_dispute_period = self.dispute_period_seconds / 12;
		let dispute_end_block = fill_proof.block_number + blocks_per_dispute_period;

		if current_block < dispute_end_block {
			return false; // Still in dispute period
		}

		// In production, would also check:
		// 1. Oracle attestation exists
		// 2. No disputes were raised
		// 3. Claim window hasn't expired
		// 4. Rewards haven't been claimed yet

		// For now, return true if dispute period passed
		true
	}
}

pub fn create_settlement(config: &toml::Value) -> Box<dyn SettlementInterface> {
	let rpc_url = config
		.get("rpc_url")
		.and_then(|v| v.as_str())
		.expect("rpc_url is required");

	let oracle_address = config
		.get("oracle_address")
		.and_then(|v| v.as_str())
		.expect("oracle_address is required");

	let min_confirmations = config
		.get("min_confirmations")
		.and_then(|v| v.as_integer())
		.unwrap_or(1) as u32;

	let dispute_period_seconds = config
		.get("dispute_period_seconds")
		.and_then(|v| v.as_integer())
		.unwrap_or(300) as u64; // 5 minutes default

	// Create settlement service synchronously
	let settlement = tokio::task::block_in_place(|| {
		tokio::runtime::Handle::current().block_on(async {
			DirectSettlement::new(
				rpc_url,
				oracle_address.to_string(),
				min_confirmations,
				dispute_period_seconds,
			)
			.await
		})
	});

	Box::new(settlement.expect("Failed to create settlement service"))
}
