//! Off-chain intent discovery sources.

use async_trait::async_trait;
use solver_types::errors::{Result, SolverError};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::info;

use crate::types::RawIntent;

/// Configuration for off-chain source
#[derive(Debug, Clone)]
pub struct OffchainConfig {
	pub name: String,
	pub endpoint: String,
	pub poll_interval: Duration,
}

/// Off-chain source (placeholder for future implementation)
///
/// This will eventually connect to external APIs, databases, or message queues
/// to discover intents submitted off-chain. For now, it's stubbed out.
///
/// Future implementation ideas:
/// - REST API polling
/// - WebSocket subscriptions
/// - Database queries
/// - Message queue consumers (Kafka, RabbitMQ, etc.)
pub struct OffchainSource {
	config: OffchainConfig,
	task_handle: tokio::sync::RwLock<Option<tokio::task::JoinHandle<()>>>,
}

impl OffchainSource {
	pub fn new(config: OffchainConfig) -> Self {
		Self {
			config,
			task_handle: tokio::sync::RwLock::new(None),
		}
	}
}

#[async_trait]
impl crate::IntentSource for OffchainSource {
	fn name(&self) -> &str {
		&self.config.name
	}

	async fn start(&self) -> Result<mpsc::Receiver<RawIntent>> {
		let mut task_handle = self.task_handle.write().await;
		if task_handle.is_some() {
			return Err(SolverError::Config("Already running".to_string()));
		}

		info!("Starting off-chain source: {}", self.config.name);

		// Create channel
		let (_tx, rx) = mpsc::channel(100);

		// For now, spawn a task that does nothing
		// Future: Poll endpoint, connect to WebSocket, query database, etc.
		let handle = tokio::spawn(async move {
			// Future implementation would:
			// - Connect to external source
			// - Poll for new intents
			// - Send them through tx
			// Example:
			// let intent = RawIntent {
			//     source: IntentSourceType::OffChain { ... },
			//     data: api_response_bytes,
			//     order_type_hint: Some("EIP7683".to_string()),
			//     context: None,
			// };
			// tx.send(intent).await.ok();
			loop {
				tokio::time::sleep(Duration::from_secs(60)).await;
			}
		});

		*task_handle = Some(handle);
		Ok(rx)
	}

	async fn stop(&self) -> Result<()> {
		let mut task_handle = self.task_handle.write().await;
		if let Some(handle) = task_handle.take() {
			handle.abort();
			info!("Stopped off-chain source: {}", self.config.name);
		}
		Ok(())
	}
}
