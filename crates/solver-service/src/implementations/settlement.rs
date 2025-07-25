//! Settlement mechanism implementations for the solver service.
//!
//! This module provides concrete implementations of the SettlementInterface trait,
//! handling fill validation and claim readiness checks for cross-chain orders.

use alloy::{
	primitives::{Address as AlloyAddress, FixedBytes},
	providers::{Provider, RootProvider},
	rpc::types::BlockTransactionsKind,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solver_settlement::{SettlementError, SettlementInterface};
use solver_types::{ConfigSchema, Field, FieldType, FillProof, Order, Schema, TransactionHash};

/// Direct settlement implementation.
///
/// This implementation validates fills by checking transaction receipts
/// and manages dispute periods before allowing claims.
pub struct DirectSettlement {
	/// The Alloy provider for blockchain interaction.
	provider: alloy::providers::RootProvider<alloy::transports::http::Http<reqwest::Client>>,
	/// Oracle address for attestation verification.
	oracle_address: String,
	/// Minimum confirmations required for fill validation.
	min_confirmations: u32,
	/// Dispute period duration in seconds.
	dispute_period_seconds: u64,
}

/// EIP-7683 specific order data used for parsing order information.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Eip7683OrderData {
	order_id: [u8; 32],
	user: String,
	origin_chain_id: u64,
	destination_chain_id: u64,
}

impl DirectSettlement {
	/// Creates a new DirectSettlement instance.
	///
	/// Configures settlement validation with the specified oracle address,
	/// confirmation requirements, and dispute period.
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
			oracle_address: oracle.to_string(),
			min_confirmations,
			dispute_period_seconds,
		})
	}
}

/// Configuration schema for DirectSettlement.
pub struct DirectSettlementSchema;

impl ConfigSchema for DirectSettlementSchema {
	fn validate(&self, config: &toml::Value) -> Result<(), solver_types::ValidationError> {
		let schema = Schema::new(
			// Required fields
			vec![
				Field::new("rpc_url", FieldType::String).with_validator(|value| {
					let url = value.as_str().unwrap();
					if url.starts_with("http://") || url.starts_with("https://") {
						Ok(())
					} else {
						Err("RPC URL must start with http:// or https://".to_string())
					}
				}),
				Field::new("oracle_address", FieldType::String).with_validator(|value| {
					let addr = value.as_str().unwrap();
					if addr.len() != 42 || !addr.starts_with("0x") {
						return Err("oracle_address must be a valid Ethereum address".to_string());
					}
					Ok(())
				}),
			],
			// Optional fields
			vec![
				Field::new(
					"min_confirmations",
					FieldType::Integer {
						min: Some(1),
						max: Some(100),
					},
				),
				Field::new(
					"dispute_period_seconds",
					FieldType::Integer {
						min: Some(0),
						max: Some(86400),
					},
				),
			],
		);

		schema.validate(config)
	}
}

#[async_trait]
impl SettlementInterface for DirectSettlement {
	fn config_schema(&self) -> Box<dyn ConfigSchema> {
		Box::new(DirectSettlementSchema)
	}

	/// Validates a fill transaction and generates a fill proof.
	///
	/// Checks that the transaction was successful, has sufficient confirmations,
	/// and extracts necessary data for claim generation.
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

		// Get the block timestamp
		let block = self
			.provider
			.get_block_by_number(
				alloy::rpc::types::BlockNumberOrTag::Number(tx_block),
				alloy::rpc::types::BlockTransactionsKind::Hashes,
			)
			.await
			.map_err(|e| {
				SettlementError::ValidationFailed(format!("Failed to get block: {}", e))
			})?;

		let block_timestamp = block
			.ok_or_else(|| SettlementError::ValidationFailed("Block not found".to_string()))?
			.header
			.timestamp;

		Ok(FillProof {
			tx_hash: tx_hash.clone(),
			block_number: tx_block,
			oracle_address: self.oracle_address.to_string(),
			attestation_data: Some(order_data.order_id.to_vec()),
			filled_timestamp: block_timestamp,
		})
	}

	/// Checks if an order is ready to be claimed.
	///
	/// Verifies that the dispute period has passed and all claim
	/// requirements are met.
	async fn can_claim(&self, _order: &Order, fill_proof: &FillProof) -> bool {
		// Get current block to check timestamp
		let current_block = match self.provider.get_block_number().await {
			Ok(block_num) => match self
				.provider
				.get_block_by_number(block_num.into(), BlockTransactionsKind::Hashes)
				.await
			{
				Ok(Some(block)) => block,
				Ok(None) => return false,
				Err(_) => return false,
			},
			Err(_) => return false,
		};

		// Check if dispute period has passed using timestamps
		let current_timestamp = current_block.header.timestamp;
		let dispute_end_timestamp = fill_proof.filled_timestamp + self.dispute_period_seconds;

		if current_timestamp < dispute_end_timestamp {
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

/// Factory function to create a settlement provider from configuration.
///
/// Required configuration parameters:
/// - `rpc_url`: The HTTP RPC endpoint URL
/// - `oracle_address`: Address of the attestation oracle
///
/// Optional configuration parameters:
/// - `min_confirmations`: Minimum confirmations required (default: 1)
/// - `dispute_period_seconds`: Dispute period duration (default: 300)
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
