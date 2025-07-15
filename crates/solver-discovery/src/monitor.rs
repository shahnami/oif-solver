//! Internal blockchain event monitoring utility for discovery.
//!
//! This module is used internally by the discovery system for direct blockchain
//! event monitoring. It's separate from the OnChainSource which implements the
//! IntentSource trait for the higher-level discovery pipeline.

use crate::{
	events::{Event, EventFilter, EventStream},
	sources::IntentSourceLocation,
};
use solver_types::{
	chains::{ChainAdapter, ChainId},
	common::BlockNumber,
	errors::Result,
};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info};

/// Configuration for blockchain monitoring
#[derive(Debug, Clone)]
pub struct MonitorConfig {
	pub poll_interval: Duration,
	pub block_delay: u64, // How many blocks behind to read (for reorg safety)
	pub batch_size: u64,  // Max blocks per query
}

impl Default for MonitorConfig {
	fn default() -> Self {
		Self {
			poll_interval: Duration::from_secs(2),
			block_delay: 0,
			batch_size: 1000,
		}
	}
}

/// Chain event source for monitoring on-chain events
pub struct ChainEventSource {
	chain_id: ChainId,
	adapter: Arc<dyn ChainAdapter>,
	config: MonitorConfig,
	last_block: Option<BlockNumber>,
}

impl ChainEventSource {
	pub fn new(chain_id: ChainId, adapter: Arc<dyn ChainAdapter>, config: MonitorConfig) -> Self {
		Self {
			chain_id,
			adapter,
			config,
			last_block: None,
		}
	}

	/// Start monitoring for events
	pub async fn watch_events(&mut self, filter: EventFilter) -> Result<EventStream<'static>> {
		info!("Starting event monitoring on chain {}", self.chain_id);

		// Get starting block if not specified
		let mut from_block = if let Some(block) = filter.from_block {
			block
		} else {
			let current = self.adapter.get_block_number().await?;
			current.saturating_sub(self.config.block_delay)
		};

		let adapter = self.adapter.clone();
		let chain_id = self.chain_id;
		let config = self.config.clone();
		let address = filter.address;
		let topics = filter.topics.clone();

		let stream = async_stream::stream! {
			let mut ticker = interval(config.poll_interval);

			loop {
				ticker.tick().await;

				// Get current block
				let current_block = match adapter.get_block_number().await {
					Ok(block) => block.saturating_sub(config.block_delay),
					Err(e) => {
						error!("Failed to get block number: {}", e);
						continue;
					}
				};

				if from_block > current_block {
					debug!("Waiting for new blocks (from={}, current={})", from_block, current_block);
					continue;
				}

				// Process in batches
				while from_block <= current_block {
					let to_block = (from_block + config.batch_size - 1).min(current_block);

					debug!(
						"Fetching logs from {} to {} on chain {}",
						from_block, to_block, chain_id
					);

					match adapter.get_logs(address, topics.clone(), from_block, to_block).await {
						Ok(logs) => {
							for log in logs {
								yield Ok(Event::from_log(log, chain_id));
							}
						}
						Err(e) => {
							error!("Failed to get logs: {}", e);
							yield Err(e);
						}
					}

					from_block = to_block + 1;
				}
			}
		};

		Ok(Box::pin(stream))
	}

	/// Get the source identifier for an event
	pub fn event_source(&self, event: &Event) -> IntentSourceLocation {
		IntentSourceLocation::OnChain {
			chain_id: self.chain_id,
			block: event.block_number,
			transaction_hash: event.transaction_hash,
			log_index: event.log_index,
		}
	}
}

impl Clone for ChainEventSource {
	fn clone(&self) -> Self {
		Self {
			chain_id: self.chain_id,
			adapter: self.adapter.clone(),
			config: self.config.clone(),
			last_block: self.last_block,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;
	use futures::StreamExt;
	use solver_types::{
		chains::{Log, Transaction, TransactionReceipt},
		common::{Address, Bytes32, TxHash, U256},
		errors::SolverError,
	};

	// Mock chain adapter for testing
	struct MockChainAdapter {
		logs: Vec<Log>,
		block_number: BlockNumber,
		confirmations: u64,
	}

	#[async_trait]
	impl ChainAdapter for MockChainAdapter {
		fn chain_id(&self) -> ChainId {
			ChainId(1)
		}

		fn confirmations(&self) -> u64 {
			self.confirmations
		}

		async fn get_block_number(&self) -> Result<BlockNumber> {
			Ok(self.block_number)
		}

		async fn get_logs(
			&self,
			_: Option<Address>,
			_: Vec<Option<Bytes32>>,
			from: BlockNumber,
			to: BlockNumber,
		) -> Result<Vec<Log>> {
			Ok(self
				.logs
				.iter()
				.filter(|log| log.block_number >= from && log.block_number <= to)
				.cloned()
				.collect())
		}

		// Other trait methods with stub implementations...
		async fn get_balance(&self, _: Address) -> Result<U256> {
			Ok(U256::zero())
		}
		async fn submit_transaction(&self, _: Transaction) -> Result<TxHash> {
			Err(SolverError::NotImplemented("mock".to_string()))
		}
		async fn get_transaction_receipt(&self, _: TxHash) -> Result<Option<TransactionReceipt>> {
			Ok(None)
		}
		async fn call(&self, _: Transaction, _: Option<BlockNumber>) -> Result<Vec<u8>> {
			Ok(vec![])
		}

		async fn estimate_gas(&self, _tx: &Transaction) -> Result<U256> {
			Ok(U256::from(100_000))
		}

		async fn get_gas_price(&self) -> Result<U256> {
			Ok(U256::from(20_000_000_000u64)) // 20 gwei
		}

		async fn get_block_timestamp(&self, _block: BlockNumber) -> Result<u64> {
			Ok(0)
		}
	}

	#[tokio::test]
	async fn test_chain_event_source() {
		let logs = vec![Log {
			address: Address::from([1u8; 20]),
			topics: vec![Bytes32::from([1u8; 32])],
			data: vec![1, 2, 3],
			block_number: 100,
			transaction_hash: Bytes32::zero(),
			log_index: 0,
		}];

		let adapter = Arc::new(MockChainAdapter {
			logs,
			block_number: 100,
			confirmations: 1,
		});

		let mut source = ChainEventSource::new(
			ChainId(1),
			adapter,
			MonitorConfig {
				poll_interval: Duration::from_millis(10),
				block_delay: 0,
				batch_size: 100,
			},
		);

		let filter = EventFilter::new().from_block(100).to_block(100);

		let mut stream = source.watch_events(filter).await.unwrap();

		// Should get our test event
		tokio::time::timeout(Duration::from_secs(1), async {
			if let Some(Ok(event)) = stream.next().await {
				assert_eq!(event.address, Address::from([1u8; 20]));
				assert_eq!(event.data, vec![1, 2, 3]);
				assert_eq!(event.chain_id, ChainId(1));
			} else {
				panic!("Expected event");
			}
		})
		.await
		.unwrap();
	}
}
