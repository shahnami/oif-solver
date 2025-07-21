// solver-plugin/src/order/processor.rs

use async_trait::async_trait;
use solver_types::events::OrderEvent;
use solver_types::plugins::{DeliveryRequest, OrderPlugin, PluginError, PluginResult};
use solver_types::OrderProcessor;
use std::sync::Arc;

/// Adapter that wraps an OrderPlugin to work as an OrderProcessor
pub struct OrderPluginProcessor<P>
where
	P: OrderPlugin + Send + Sync,
{
	plugin: Arc<P>,
	source_prefix: String,
}

impl<P> OrderPluginProcessor<P>
where
	P: OrderPlugin + Send + Sync,
{
	pub fn new(plugin: Arc<P>, source_prefix: String) -> Self {
		Self {
			plugin,
			source_prefix,
		}
	}
}

#[async_trait]
impl<P> OrderProcessor for OrderPluginProcessor<P>
where
	P: OrderPlugin + Send + Sync + 'static,
	P::Order: 'static,
	P::ParseContext: Default + 'static,
{
	async fn process_order_event(
		&self,
		event: &OrderEvent,
	) -> PluginResult<Option<DeliveryRequest>> {
		// Check if we have the raw data and contract address needed
		if event.raw_data.is_empty() {
			return Err(PluginError::ExecutionFailed(
				"No raw data in order event".to_string(),
			));
		}

		let contract_address = event.contract_address.as_ref().ok_or_else(|| {
			PluginError::ExecutionFailed("No contract address in order event".to_string())
		})?;

		// Parse the order using the plugin
		let order = self
			.plugin
			.parse_order(&event.raw_data, Some(P::ParseContext::default()))
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Failed to parse order: {}", e)))?;

		// Validate the order
		let validation = self.plugin.validate_order(&order).await?;
		if !validation.is_valid {
			return Err(PluginError::ExecutionFailed(format!(
				"Order validation failed: {:?}",
				validation.errors
			)));
		}

		// Create the fill request
		let delivery_request = self.plugin.create_fill_request(&order).await.map_err(|e| {
			PluginError::ExecutionFailed(format!("Failed to create fill request: {}", e))
		})?;

		Ok(Some(delivery_request))
	}

	fn can_handle_source(&self, source: &str) -> bool {
		source.starts_with(&self.source_prefix)
	}
}
