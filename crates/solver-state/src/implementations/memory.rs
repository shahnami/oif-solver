//! In-memory storage implementation.

use crate::{storage::Storage, types::OrderState};
use async_trait::async_trait;
use solver_discovery::OrderStatus;
use solver_types::{errors::Result, orders::OrderId};
use std::collections::HashMap;

/// In-memory storage implementation
#[derive(Clone)]
pub struct MemoryStorage {
	data: dashmap::DashMap<OrderId, OrderState>,
}

impl MemoryStorage {
	pub fn new() -> Self {
		Self {
			data: dashmap::DashMap::new(),
		}
	}
}

impl Default for MemoryStorage {
	fn default() -> Self {
		Self::new()
	}
}

#[async_trait]
impl Storage for MemoryStorage {
	async fn store_order_state(&self, state: &OrderState) -> Result<()> {
		self.data.insert(state.id, state.clone());
		Ok(())
	}

	async fn get_order_state(&self, order_id: &OrderId) -> Result<Option<OrderState>> {
		Ok(self.data.get(order_id).map(|entry| entry.clone()))
	}

	async fn get_orders_by_status(&self, status: OrderStatus) -> Result<Vec<OrderId>> {
		Ok(self
			.data
			.iter()
			.filter(|entry| entry.status == status)
			.map(|entry| entry.id)
			.collect())
	}

	async fn count_by_status(&self) -> Result<HashMap<OrderStatus, usize>> {
		let mut counts = HashMap::new();

		for entry in self.data.iter() {
			*counts.entry(entry.status).or_insert(0) += 1;
		}

		Ok(counts)
	}

	async fn delete_order_state(&self, order_id: &OrderId) -> Result<()> {
		self.data.remove(order_id);
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::common::Bytes32;

	#[tokio::test]
	async fn test_memory_storage() {
		let storage = MemoryStorage::new();
		let order_id = Bytes32::from([1u8; 32]);

		// Create test state
		let state = OrderState {
			id: order_id,
			order_data: vec![1, 2, 3],
			status: OrderStatus::Discovered,
			priority: crate::types::OrderPriority::calculate(
				solver_orders::classification::Urgency::Normal,
				None,
				0,
			),
			discovered_at: 1000,
			queued_at: None,
			processed_at: None,
			completed_at: None,
			attempts: 0,
			last_error: None,
		};

		// Store and retrieve
		storage.store_order_state(&state).await.unwrap();
		let retrieved = storage.get_order_state(&order_id).await.unwrap();
		assert!(retrieved.is_some());
		assert_eq!(retrieved.unwrap().id, order_id);

		// Count by status
		let counts = storage.count_by_status().await.unwrap();
		assert_eq!(counts.get(&OrderStatus::Discovered), Some(&1));
	}
}
