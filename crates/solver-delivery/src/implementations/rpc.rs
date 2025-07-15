//! RPC delivery service using ChainRegistry for blockchain interactions.
//! This implementation delegates all blockchain operations to the appropriate
//! chain adapters instead of making direct RPC calls.

use crate::{types::DeliveryConfig, DeliveryService};
use async_trait::async_trait;
use solver_chains::ChainRegistry;
use solver_types::{
	chains::{ChainAdapter, ChainId, Transaction, TransactionReceipt},
	common::{Address, TxHash, U256},
	errors::{Result, SolverError},
};
use std::sync::Arc;
use tracing::{debug, info};

/// RPC delivery service
#[derive(Clone)]
pub struct RpcDelivery {
	chain_registry: Arc<ChainRegistry>,
}

impl RpcDelivery {
	pub fn new(_config: DeliveryConfig, chain_registry: Arc<ChainRegistry>) -> Self {
		Self { chain_registry }
	}

	/// Get chain adapter for the specified chain.
	fn get_adapter(&self, chain_id: ChainId) -> Result<Arc<dyn ChainAdapter>> {
		self.chain_registry.get(&chain_id).ok_or_else(|| {
			SolverError::Delivery(format!(
				"No chain adapter configured for chain {}",
				chain_id
			))
		})
	}
}

#[async_trait]
impl DeliveryService for RpcDelivery {
	fn name(&self) -> &str {
		"RPC"
	}

	fn supports_chain(&self, chain_id: ChainId) -> bool {
		self.chain_registry.get(&chain_id).is_some()
	}

	async fn submit_transaction(&self, chain_id: ChainId, tx: Transaction) -> Result<TxHash> {
		info!(
			"Submitting transaction via ChainAdapter on chain {}",
			chain_id
		);

		let adapter = self.get_adapter(chain_id)?;

		// The chain adapter now handles gas pricing, signing, and submission
		adapter.submit_transaction(tx).await
	}

	async fn wait_for_confirmation(
		&self,
		chain_id: ChainId,
		tx_hash: TxHash,
		confirmations: u64,
	) -> Result<TransactionReceipt> {
		info!(
			"Waiting for {} confirmations for tx {}",
			confirmations, tx_hash
		);

		let adapter = self.get_adapter(chain_id)?;
		let start_time = tokio::time::Instant::now();
		let timeout = tokio::time::Duration::from_secs(120); // 2 minute timeout
		let mut attempts = 0;
		const MAX_ATTEMPTS: u32 = 60; // Maximum polling attempts

		loop {
			attempts += 1;

			// Check for timeout
			if start_time.elapsed() > timeout {
				return Err(SolverError::Delivery(format!(
					"Transaction {} confirmation timeout after {}s",
					tx_hash,
					timeout.as_secs()
				)));
			}

			// Check for max attempts
			if attempts > MAX_ATTEMPTS {
				return Err(SolverError::Delivery(format!(
					"Transaction {} exceeded maximum polling attempts ({})",
					tx_hash, MAX_ATTEMPTS
				)));
			}

			// Use chain adapter to get transaction receipt
			debug!("Attempt {} to get receipt for tx {}", attempts, tx_hash);
			let receipt = adapter.get_transaction_receipt(tx_hash).await?;
			debug!("Receipt result for tx {}: {:?}", tx_hash, receipt.is_some());

			if let Some(receipt) = receipt {
				// Check if transaction failed
				if !receipt.status {
					return Err(SolverError::Delivery(format!(
						"Transaction {} failed (status: false)",
						tx_hash
					)));
				}

				let current_block = adapter.get_block_number().await?;

				if current_block >= receipt.block_number + confirmations {
					// Fetch block timestamp for the receipt
					let timestamp = adapter.get_block_timestamp(receipt.block_number).await.ok();

					info!(
						"Transaction {} confirmed after {} attempts in {}ms",
						tx_hash,
						attempts,
						start_time.elapsed().as_millis()
					);

					return Ok(TransactionReceipt {
						transaction_hash: receipt.transaction_hash,
						block_number: receipt.block_number,
						gas_used: receipt.gas_used,
						status: receipt.status,
						timestamp,
					});
				} else {
					debug!(
                        "Transaction {} waiting for confirmations: current block {}, tx block {}, need {} confirmations",
                        tx_hash, current_block, receipt.block_number, confirmations
                    );
				}
			} else {
				debug!(
					"Transaction {} not yet mined (attempt {})",
					tx_hash, attempts
				);
			}

			tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
		}
	}

	async fn estimate_gas(&self, chain_id: ChainId, tx: &Transaction) -> Result<U256> {
		let adapter = self.get_adapter(chain_id)?;

		// Use the chain adapter's estimate_gas method
		adapter.estimate_gas(tx).await
	}

	async fn get_gas_price(&self, chain_id: ChainId) -> Result<U256> {
		let adapter = self.get_adapter(chain_id)?;

		// Use the chain adapter's get_gas_price method
		adapter.get_gas_price().await
	}

	async fn get_nonce(&self, chain_id: ChainId, address: Address) -> Result<U256> {
		let adapter = self.get_adapter(chain_id)?;

		// ChainAdapter doesn't have a get_nonce method, but we can use the balance method
		// as a proxy to check if the adapter is working
		// TODO: Add get_nonce method to ChainAdapter trait
		let _balance = adapter.get_balance(address).await?;

		// For now, return a simple incrementing nonce
		// In a real implementation, this would be properly tracked
		Ok(U256::from(0))
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::collections::HashMap;

	#[test]
	fn test_supports_chain() {
		let config = DeliveryConfig {
			endpoints: HashMap::new(),
			api_key: "test".to_string(),
			gas_strategy: crate::types::GasStrategy::Standard,
			max_retries: 3,
			confirmations: 1,
			from_address: Address::zero(),
		};

		let chain_registry = Arc::new(ChainRegistry::new());
		let rpc = RpcDelivery::new(config, chain_registry);

		// Without any adapters registered, should not support any chains
		assert!(!rpc.supports_chain(ChainId(1)));
		assert!(!rpc.supports_chain(ChainId(137)));
	}

	#[test]
	fn test_name() {
		let config = DeliveryConfig {
			endpoints: HashMap::new(),
			api_key: "test".to_string(),
			gas_strategy: crate::types::GasStrategy::Custom { multiplier: 1.5 },
			max_retries: 3,
			confirmations: 1,
			from_address: Address::zero(),
		};

		let chain_registry = Arc::new(ChainRegistry::new());
		let rpc = RpcDelivery::new(config, chain_registry);
		assert_eq!(rpc.name(), "RPC");
	}
}
