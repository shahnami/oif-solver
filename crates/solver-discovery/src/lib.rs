use async_trait::async_trait;
use solver_types::Intent;
use thiserror::Error;
use tokio::sync::mpsc;

#[derive(Debug, Error)]
pub enum DiscoveryError {
	#[error("Connection error: {0}")]
	Connection(String),
	#[error("Already monitoring")]
	AlreadyMonitoring,
}

#[async_trait]
pub trait DiscoveryInterface: Send + Sync {
	async fn start_monitoring(
		&self,
		sender: mpsc::UnboundedSender<Intent>,
	) -> Result<(), DiscoveryError>;
	async fn stop_monitoring(&self) -> Result<(), DiscoveryError>;
}

pub struct DiscoveryService {
	sources: Vec<Box<dyn DiscoveryInterface>>,
}

impl DiscoveryService {
	pub fn new(sources: Vec<Box<dyn DiscoveryInterface>>) -> Self {
		Self { sources }
	}

	pub async fn start_all(
		&self,
		sender: mpsc::UnboundedSender<Intent>,
	) -> Result<(), DiscoveryError> {
		for source in &self.sources {
			source.start_monitoring(sender.clone()).await?;
		}
		Ok(())
	}

	pub async fn stop_all(&self) -> Result<(), DiscoveryError> {
		for source in &self.sources {
			source.stop_monitoring().await?;
		}
		Ok(())
	}
}
