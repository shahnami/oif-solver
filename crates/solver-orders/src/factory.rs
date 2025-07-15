//! Factory trait for creating order instances.

use async_trait::async_trait;
use solver_types::{common::Bytes32, errors::Result, orders::Order};

/// Factory for creating order instances from raw data
#[async_trait]
pub trait OrderFactory: Send + Sync {
	/// Parse raw bytes into an order instance
	async fn parse_order(&self, data: &[u8]) -> Result<Box<dyn Order>>;

	/// Validate raw order data without fully parsing
	async fn validate_format(&self, data: &[u8]) -> Result<()>;

	/// Get the event signatures this factory handles for on-chain discovery
	/// Returns empty vec if this order type doesn't use on-chain events
	fn event_signatures(&self) -> Vec<Bytes32> {
		vec![]
	}
}
