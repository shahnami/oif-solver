//! High-level intent discovery interface.
//!
//! This module provides a flexible discovery service that can monitor
//! intents from various sources (on-chain, off-chain APIs, etc).

use crate::{
	implementations::StandardProcessorRegistry,
	lifecycle::OrderLifecycle,
	types::{DiscoveredIntent, DiscoveryMetadata, RawIntent},
	IntentSource,
};
use futures::Stream;
use solver_orders::OrderRegistry;
use solver_types::{
	errors::Result,
	orders::{Order, OrderId, OrderStatus},
};
use std::{pin::Pin, sync::Arc};
use tracing::{debug, info, warn};

/// Main intent discovery service
pub struct IntentDiscovery {
	sources: Arc<Vec<Box<dyn IntentSource>>>,
	order_registry: Arc<OrderRegistry>,
	lifecycle: Arc<OrderLifecycle>,
	processor_registry: Arc<StandardProcessorRegistry>,
}

impl IntentDiscovery {
	pub fn new(sources: Vec<Box<dyn IntentSource>>, order_registry: Arc<OrderRegistry>) -> Self {
		Self {
			sources: Arc::new(sources),
			order_registry,
			lifecycle: Arc::new(OrderLifecycle::new()),
			processor_registry: Arc::new(StandardProcessorRegistry::new()),
		}
	}

	/// Start discovering intents from all configured sources
	pub async fn start_discovery(
		self: Arc<Self>,
	) -> Result<impl Stream<Item = Result<DiscoveredIntent>> + Send + 'static> {
		info!(
			"Starting intent discovery from {} sources",
			self.sources.len()
		);

		// Start all sources and collect their receivers
		let mut receivers = Vec::new();
		for source in self.sources.iter() {
			info!("Starting source: {}", source.name());
			let receiver = source.start().await?;
			let source_name = source.name().to_string();
			receivers.push((receiver, source_name));
		}

		// Create streams from all receivers
		let mut streams = Vec::new();

		// Process each receiver
		for (mut receiver, source_name) in receivers {
			// Clone necessary data for the stream processing
			let order_registry = self.order_registry.clone();
			let lifecycle = self.lifecycle.clone();
			let processor_registry = self.processor_registry.clone();

			// Convert receiver to stream and process raw intents
			let processed_stream = async_stream::stream! {
				while let Some(raw_intent) = receiver.recv().await {
					debug!("Processing intent from source: {}", source_name);
					match Self::process_raw_intent(
						raw_intent,
						order_registry.clone(),
						lifecycle.clone(),
						processor_registry.clone(),
					).await {
						Ok(Some(discovered)) => yield Ok(discovered),
						Ok(None) => {},
						Err(e) => {
							warn!("Failed to process intent from {}: {}", source_name, e);
							yield Err(e);
						}
					}
				}
			};

			streams.push(Box::pin(processed_stream)
				as Pin<Box<dyn Stream<Item = Result<DiscoveredIntent>> + Send>>);
		}

		// Merge all streams
		Ok(futures::stream::select_all(streams))
	}

	/// Process a raw intent into a discovered intent
	async fn process_raw_intent(
		raw_intent: RawIntent,
		order_registry: Arc<OrderRegistry>,
		lifecycle: Arc<OrderLifecycle>,
		processor_registry: Arc<StandardProcessorRegistry>,
	) -> Result<Option<DiscoveredIntent>> {
		debug!("Processing raw intent from {:?}", raw_intent.source);

		// Get the appropriate processor for this intent
		let processor =
			match processor_registry.get_processor(raw_intent.order_type_hint.as_deref()) {
				Some(p) => p,
				_ => {
					warn!(
						"No processor found for order type hint: {:?}",
						raw_intent.order_type_hint
					);
					return Ok(None);
				}
			};

		// Save source before moving raw_intent
		let source = raw_intent.source.clone();

		// Process the intent using the appropriate processor
		let (order, formatted_data) = processor
			.process_intent(raw_intent, &order_registry)
			.await?;

		// Create order ID
		let order_id = order.id();

		// Set initial status
		lifecycle
			.update_status(order_id, OrderStatus::Discovered)
			.await;

		// Create metadata
		let current_time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let metadata = DiscoveryMetadata {
			discovered_at: current_time,
			last_updated: current_time,
			seen_count: 1,
			extra: serde_json::Value::Null,
		};

		Ok(Some(DiscoveredIntent {
			order,
			raw_order_data: formatted_data,
			source,
			metadata,
			status: OrderStatus::Discovered,
		}))
	}

	/// Get the current status of an order
	pub async fn get_order_status(&self, order_id: &OrderId) -> Option<OrderStatus> {
		self.lifecycle.get_status(order_id).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn test_intent_discovery_with_processor_registry() {
		// Create a mock order registry
		let order_registry = Arc::new(OrderRegistry::new());

		// Create intent discovery with empty sources
		let discovery = IntentDiscovery::new(vec![], order_registry.clone());

		// Verify processor registry is initialized
		assert!(discovery
			.processor_registry
			.get_processor(Some("EIP7683"))
			.is_some());
		assert!(discovery.processor_registry.get_processor(None).is_some()); // Default to EIP7683
		assert!(discovery
			.processor_registry
			.get_processor(Some("CustomStandard"))
			.is_none());
	}
}
