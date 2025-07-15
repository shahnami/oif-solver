//! Unified state manager combining queue and storage.

use crate::{
	queue::OrderQueue,
	storage::{create_storage, Storage, StorageBackend, StorageImpl},
	types::{OrderPriority, OrderState, StateError},
};
use solver_discovery::{DiscoveredIntent, OrderStatus};
use solver_orders::classification::{OrderClassifier, Urgency};
use solver_types::{
	errors::Result,
	orders::{Order, OrderId},
};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tracing::{debug, info};

/// Configuration for state management
#[derive(Debug, Clone)]
pub struct StateConfig {
	/// Maximum queue size
	pub max_queue_size: usize,
	/// Storage backend type
	pub storage_backend: StorageBackend,
	/// Recovery on startup
	pub recover_on_startup: bool,
}

impl Default for StateConfig {
	fn default() -> Self {
		Self {
			max_queue_size: 10_000,
			storage_backend: StorageBackend::File {
				path: PathBuf::from("./data/solver-state"),
			},
			recover_on_startup: true,
		}
	}
}

/// Unified state manager
pub struct StateManager {
	queue: Arc<OrderQueue>,
	storage: StorageImpl,
	_config: StateConfig,
	classifier: OrderClassifier,
}

impl StateManager {
	/// Create new state manager
	pub async fn new(config: StateConfig) -> Result<Self> {
		let storage = create_storage(config.storage_backend.clone()).await?;
		let queue = Arc::new(OrderQueue::new(config.max_queue_size));

		let manager = Self {
			queue: queue.clone(),
			storage,
			_config: config.clone(),
			classifier: OrderClassifier::new(),
		};

		// Recover state if configured
		if config.recover_on_startup {
			if let StorageBackend::File { .. } = &config.storage_backend {
				manager.recover_state().await?;
			}
		}

		Ok(manager)
	}

	/// Recover state from storage
	async fn recover_state(&self) -> Result<()> {
		info!("Recovering state from storage");

		// Get all orders that should be in queue
		let ready_orders = self
			.storage
			.get_orders_by_status(OrderStatus::Ready)
			.await?;
		let discovered_orders = self
			.storage
			.get_orders_by_status(OrderStatus::Discovered)
			.await?;

		let mut recovered = 0;

		// Re-queue ready orders first (higher priority)
		for order_id in ready_orders {
			if let Some(state) = self.storage.get_order_state(&order_id).await? {
				if self.queue.enqueue(order_id, state.priority).is_ok() {
					recovered += 1;
				}
			}
		}

		// Then discovered orders
		for order_id in discovered_orders {
			if let Some(state) = self.storage.get_order_state(&order_id).await? {
				if self.queue.enqueue(order_id, state.priority).is_ok() {
					recovered += 1;
				}
			}
		}

		info!("Recovered {} orders to queue", recovered);
		Ok(())
	}

	/// Add discovered intent to state
	pub async fn add_discovered_intent(&self, intent: DiscoveredIntent) -> Result<()> {
		let order_id = intent.order.id();

		// Use the raw order data from discovery
		let order_data = intent.raw_order_data;

		// Determine urgency using the classifier
		let classification = self.classifier.classify(&intent.order).await?;
		let urgency = classification.urgency;

		let priority = OrderPriority::calculate(urgency, None, 0);

		let state = OrderState {
			id: order_id,
			order_data,
			status: OrderStatus::Discovered,
			priority,
			discovered_at: intent.metadata.discovered_at,
			queued_at: None,
			processed_at: None,
			completed_at: None,
			attempts: 0,
			last_error: None,
		};

		// Store in persistent storage
		self.storage.store_order_state(&state).await?;

		// Add to queue
		self.queue.enqueue(order_id, priority)?;

		debug!("Added discovered order {} to state", order_id);
		Ok(())
	}

	/// Get next order for processing
	pub async fn get_next_order(&self) -> Result<Option<OrderState>> {
		if let Some(order_id) = self.queue.dequeue() {
			if let Some(mut state) = self.storage.get_order_state(&order_id).await? {
				// Update status
				state.status = OrderStatus::Filling;
				state.processed_at = Some(chrono::Utc::now().timestamp() as u64);
				state.attempts += 1;

				// Update storage
				self.storage.store_order_state(&state).await?;

				return Ok(Some(state));
			}
		}

		Ok(None)
	}

	/// Update order status
	pub async fn update_order_status(
		&self,
		order_id: &OrderId,
		status: OrderStatus,
		error: Option<String>,
	) -> Result<()> {
		if let Some(mut state) = self.storage.get_order_state(order_id).await? {
			state.status = status;

			if let Some(err) = error {
				state.last_error = Some(err);
			}

			if matches!(status, OrderStatus::Settled | OrderStatus::Abandoned) {
				state.completed_at = Some(chrono::Utc::now().timestamp() as u64);
			}

			// Update storage
			self.storage.store_order_state(&state).await?;

			// Remove from queue if terminal state
			if matches!(
				status,
				OrderStatus::Settled | OrderStatus::Abandoned | OrderStatus::Invalid
			) {
				self.queue.remove(order_id);
			}

			Ok(())
		} else {
			Err(StateError::OrderNotFound(*order_id).into())
		}
	}

	/// Re-queue failed order
	pub async fn requeue_order(&self, order_id: &OrderId) -> Result<()> {
		if let Some(mut state) = self.storage.get_order_state(order_id).await? {
			// Update priority based on age
			let now = chrono::Utc::now().timestamp() as u64;
			let age_seconds = if now >= state.discovered_at {
				now - state.discovered_at
			} else {
				0 // If discovered_at is in the future, treat as 0 age
			};
			// For requeue, preserve the original urgency from the priority's urgency_modifier
			// but update the age component to reflect the additional time that has passed
			let original_urgency = match state.priority.urgency_modifier {
				10 => Urgency::High,
				0 => Urgency::Normal,
				-10 => Urgency::Low,
				_ => Urgency::Normal, // Default fallback
			};
			let priority = OrderPriority::calculate(original_urgency, None, age_seconds);

			state.priority = priority;
			state.status = OrderStatus::Ready;
			state.queued_at = Some(chrono::Utc::now().timestamp() as u64);

			// Update storage
			self.storage.store_order_state(&state).await?;

			// Re-add to queue
			self.queue.enqueue(*order_id, priority)?;

			Ok(())
		} else {
			Err(StateError::OrderNotFound(*order_id).into())
		}
	}

	/// Get order state
	pub async fn get_order_state(&self, order_id: &OrderId) -> Result<Option<OrderState>> {
		self.storage.get_order_state(order_id).await
	}

	/// Get state statistics
	pub async fn get_stats(&self) -> Result<StateStats> {
		let counts = self.storage.count_by_status().await?;
		let queue_size = self.queue.len();

		Ok(StateStats {
			total_orders: counts.values().sum(),
			by_status: counts,
			queue_size,
		})
	}

	/// Clean up old completed orders
	pub async fn cleanup_old_orders(&self, max_age_seconds: u64) -> Result<usize> {
		let cutoff = chrono::Utc::now().timestamp() as u64 - max_age_seconds;
		let mut cleaned = 0;

		// Get completed orders
		let statuses = vec![OrderStatus::Settled, OrderStatus::Abandoned];

		for status in statuses {
			let orders = self.storage.get_orders_by_status(status).await?;

			for order_id in orders {
				if let Some(state) = self.storage.get_order_state(&order_id).await? {
					if let Some(completed_at) = state.completed_at {
						if completed_at < cutoff {
							self.storage.delete_order_state(&order_id).await?;
							cleaned += 1;
						}
					}
				}
			}
		}

		info!("Cleaned up {} old orders", cleaned);
		Ok(cleaned)
	}
}

/// State statistics
#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct StateStats {
	pub total_orders: usize,
	pub by_status: HashMap<OrderStatus, usize>,
	pub queue_size: usize,
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::common::Bytes32;

	#[tokio::test]
	async fn test_state_manager_memory() {
		let config = StateConfig {
			max_queue_size: 100,
			storage_backend: StorageBackend::Memory,
			recover_on_startup: false,
		};

		let manager = StateManager::new(config).await.unwrap();

		// Create test order state
		let order_id = Bytes32::from([1u8; 32]);
		let state = OrderState {
			id: order_id,
			order_data: vec![1, 2, 3],
			status: OrderStatus::Discovered,
			priority: OrderPriority::calculate(Urgency::Normal, None, 0),
			discovered_at: 1000,
			queued_at: Some(1100),
			processed_at: None,
			completed_at: None,
			attempts: 0,
			last_error: None,
		};

		// Store and enqueue
		manager.storage.store_order_state(&state).await.unwrap();
		manager.queue.enqueue(order_id, state.priority).unwrap();

		// Get next order
		let next = manager.get_next_order().await.unwrap();
		assert!(next.is_some());
		assert_eq!(next.unwrap().id, order_id);

		// Check stats
		let stats = manager.get_stats().await.unwrap();
		assert_eq!(stats.total_orders, 1);
	}
}
