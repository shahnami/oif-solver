// solver-plugin/src/order/processor.rs

use async_trait::async_trait;
use solver_types::events::{FillEvent, OrderEvent};
use solver_types::plugins::{
	delivery::{
		TransactionMetadata, TransactionPriority, TransactionRequest, TransactionRequestType,
	},
	OrderPlugin, PluginError, PluginResult,
};
use solver_types::{DeliveryPriority, Order, OrderProcessor, SettlementPriority};
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
	) -> PluginResult<Option<TransactionRequest>> {
		// Check if we have the raw data and contract address needed
		if event.raw_data.is_empty() {
			return Err(PluginError::ExecutionFailed(
				"No raw data in order event".to_string(),
			));
		}

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

		// Convert DeliveryRequest to TransactionRequest
		let transaction_request = TransactionRequest {
			transaction: delivery_request.transaction,
			priority: match delivery_request.priority {
				DeliveryPriority::Low => TransactionPriority::Low,
				DeliveryPriority::Normal => TransactionPriority::Normal,
				DeliveryPriority::High => TransactionPriority::High,
				DeliveryPriority::Urgent => TransactionPriority::Urgent,
				DeliveryPriority::Custom {
					max_fee,
					priority_fee,
					deadline,
				} => TransactionPriority::Custom {
					max_fee,
					priority_fee,
					deadline,
				},
			},
			request_type: TransactionRequestType::Fill {
				order_id: event.order_id.clone(),
				order_type: event.source.clone(),
			},
			metadata: TransactionMetadata {
				order_id: delivery_request.metadata.order_id,
				user: delivery_request.metadata.user,
				source: event.source.clone(),
				tags: delivery_request.metadata.tags,
				custom_fields: delivery_request.metadata.custom_fields,
			},
			retry_config: delivery_request.retry_config,
		};

		Ok(Some(transaction_request))
	}

	async fn process_fill_event(
		&self,
		event: &FillEvent,
	) -> PluginResult<Option<TransactionRequest>> {
		// Check if we have the order data in the event
		let order_data = match &event.order_data {
			Some(data) => data,
			None => return Ok(None), // No order data available
		};

		// Parse the order using the plugin
		let order = self
			.plugin
			.parse_order(order_data, Some(P::ParseContext::default()))
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Failed to parse order: {}", e)))?;

		// Create the settlement request using the plugin
		match self
			.plugin
			.create_settlement_request(&order, event.timestamp)
			.await?
		{
			Some(settlement_req) => {
				// Convert SettlementRequest to TransactionRequest
				let transaction_request = TransactionRequest {
					transaction: settlement_req.transaction.transaction,
					priority: match settlement_req.priority {
						SettlementPriority::Immediate => TransactionPriority::Urgent,
						SettlementPriority::Batched => TransactionPriority::Normal,
						SettlementPriority::Optimized => TransactionPriority::Low,
						SettlementPriority::Scheduled(_) => TransactionPriority::Normal,
					},
					request_type: TransactionRequestType::Settlement {
						order_id: event.order_id.clone(),
						fill_id: event.fill_id.clone(),
						settlement_type: settlement_req.transaction.settlement_type,
						expected_reward: settlement_req.transaction.expected_reward,
					},
					metadata: TransactionMetadata {
						order_id: event.order_id.clone(),
						user: order.user(),
						source: event.source.clone(),
						tags: vec![],
						custom_fields: settlement_req.transaction.metadata.custom_fields,
					},
					retry_config: settlement_req.retry_config,
				};
				Ok(Some(transaction_request))
			}
			None => Ok(None),
		}
	}

	fn can_handle_source(&self, source: &str) -> bool {
		source.starts_with(&self.source_prefix)
	}
}
