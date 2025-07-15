//! On-chain intent discovery through event monitoring.

use async_trait::async_trait;
use futures::StreamExt;
use solver_types::{
	chains::{ChainAdapter, ChainId},
	common::{Address, BlockNumber, Bytes32},
	errors::{Result, SolverError},
};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::{
	events::EventFilter,
	monitor::{ChainEventSource, MonitorConfig},
	types::{IntentSourceType, RawIntent},
};

/// Configuration for on-chain monitoring
#[derive(Debug, Clone)]
pub struct OnChainConfig {
	/// Chain to monitor
	pub chain_id: ChainId,
	/// Contract addresses to monitor (settlers)
	pub settler_addresses: Vec<Address>,
	/// Block to start monitoring from (None = latest)
	pub start_block: Option<BlockNumber>,
	/// Event signatures to monitor
	pub event_signatures: Vec<Bytes32>,
	/// Event monitoring configuration
	pub monitor_config: MonitorConfig,
}

impl Default for OnChainConfig {
	fn default() -> Self {
		Self {
			chain_id: ChainId(1),
			settler_addresses: vec![],
			start_block: None,
			event_signatures: vec![],
			monitor_config: MonitorConfig::default(),
		}
	}
}

/// On-chain event monitoring source
pub struct OnChainSource {
	config: OnChainConfig,
	chain_adapter: Arc<dyn ChainAdapter>,
	/// Handle to the background monitoring task
	task_handle: tokio::sync::RwLock<Option<tokio::task::JoinHandle<()>>>,
}

impl OnChainSource {
	pub fn new(config: OnChainConfig, chain_adapter: Arc<dyn ChainAdapter>) -> Self {
		Self {
			config,
			chain_adapter,
			task_handle: tokio::sync::RwLock::new(None),
		}
	}

	/// Create event filter for monitoring
	fn create_filter(&self) -> EventFilter {
		let mut filter = EventFilter::default();

		// Monitor all configured settler addresses
		if self.config.settler_addresses.len() == 1 {
			filter.address = Some(self.config.settler_addresses[0]);
		}
		// Note: Multiple addresses would require multiple filters or OR logic

		// Add event signatures as topics
		if !self.config.event_signatures.is_empty() {
			filter.topics = vec![Some(self.config.event_signatures[0])];
		}

		// Set block range
		filter.from_block = self.config.start_block;

		filter
	}
}

#[async_trait]
impl crate::IntentSource for OnChainSource {
	fn name(&self) -> &str {
		"onchain"
	}

	async fn start(&self) -> Result<tokio::sync::mpsc::Receiver<RawIntent>> {
		let mut task_handle = self.task_handle.write().await;
		if task_handle.is_some() {
			return Err(SolverError::Config("Already running".to_string()));
		}

		info!(
			"Starting on-chain monitoring for chain {} with {} settler addresses",
			self.config.chain_id,
			self.config.settler_addresses.len()
		);

		// Create channel for sending intents
		let (tx, rx) = tokio::sync::mpsc::channel(100);

		// Clone data needed for the background task
		let config = self.config.clone();
		let chain_adapter = self.chain_adapter.clone();
		let filter = self.create_filter();

		// Spawn background monitoring task
		let handle = tokio::spawn(async move {
			// Create the monitor
			let mut monitor = ChainEventSource::new(
				config.chain_id,
				chain_adapter.clone(),
				config.monitor_config.clone(),
			);

			// Start watching events
			let events = match monitor.watch_events(filter).await {
				Ok(stream) => stream,
				Err(e) => {
					error!("Failed to start event monitoring: {}", e);
					return;
				}
			};

			// Process events and send intents through the channel
			tokio::pin!(events);
			loop {
				match events.next().await {
					Some(Ok(event)) => {
						debug!(
							"Discovered event from chain {} at block {}: {}",
							config.chain_id,
							event.block_number,
							hex::encode(&event.data)
						);

						// Create context with event details
						let event_context = serde_json::json!({
							"address": format!("0x{}", hex::encode(event.address.as_ref())),
							"topics": event.topics.iter().map(|t| format!("0x{}", hex::encode(t.as_ref()))).collect::<Vec<_>>(),
						});

						let intent = RawIntent {
							source: IntentSourceType::OnChain {
								chain: config.chain_id,
								block: event.block_number,
								transaction_hash: event.transaction_hash.into(),
								log_index: event.log_index,
							},
							data: event.data,
							order_type_hint: Some("EIP7683".to_string()),
							context: Some(event_context),
						};

						// Send intent through channel
						if tx.send(intent).await.is_err() {
							info!("Channel closed, stopping on-chain monitoring");
							break;
						}
					}
					Some(Err(e)) => {
						error!("Error reading event: {}", e);
					}
					None => {
						info!("Event stream ended");
						break;
					}
				}
			}
		});

		*task_handle = Some(handle);
		Ok(rx)
	}

	async fn stop(&self) -> Result<()> {
		let mut task_handle = self.task_handle.write().await;
		if let Some(handle) = task_handle.take() {
			handle.abort();
			info!(
				"Stopped on-chain monitoring for chain {}",
				self.config.chain_id
			);
		}
		Ok(())
	}
}
