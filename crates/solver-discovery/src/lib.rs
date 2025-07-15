//! Intent discovery and event monitoring for the OIF solver.

pub mod discovery;
pub mod events;
pub mod implementations;
pub mod lifecycle;
pub mod monitor;
pub mod pipeline;
pub mod sources;
pub mod types;

use async_trait::async_trait;
use solver_types::errors::Result;
use tokio::sync::mpsc;
use types::RawIntent;

pub use discovery::IntentDiscovery;
pub use events::{Event, EventFilter, EventStream};
pub use implementations::OrderStandardProcessor;
pub use lifecycle::OrderLifecycle;
pub use pipeline::DiscoveryPipeline;
pub use solver_types::orders::OrderStatus;
pub use types::{DiscoveredIntent, DiscoveryMetadata, IntentSourceType};

// Re-export source types
pub use sources::{OffchainSource, OnChainSource};

/// Trait for intent discovery sources
#[async_trait]
pub trait IntentSource: Send + Sync {
	/// Get the name of this source
	fn name(&self) -> &str;

	/// Start the discovery source and return a channel receiver for intents
	/// The source will run in the background and send intents through the channel
	async fn start(&self) -> Result<mpsc::Receiver<RawIntent>>;

	/// Stop the discovery source
	async fn stop(&self) -> Result<()>;
}
