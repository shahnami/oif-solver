//! Order lifecycle tracking.

use dashmap::DashMap;
use solver_types::orders::{OrderId, OrderStatus};

/// Tracks order lifecycle
pub struct OrderLifecycle {
	statuses: DashMap<OrderId, OrderStatus>,
}

impl OrderLifecycle {
	pub fn new() -> Self {
		Self {
			statuses: DashMap::new(),
		}
	}

	/// Update order status
	pub async fn update_status(&self, order_id: OrderId, status: OrderStatus) {
		self.statuses.insert(order_id, status);
	}

	/// Get order status
	pub async fn get_status(&self, order_id: &OrderId) -> Option<OrderStatus> {
		self.statuses.get(order_id).map(|entry| *entry)
	}

	/// Remove order from tracking
	pub async fn remove(&self, order_id: &OrderId) {
		self.statuses.remove(order_id);
	}

	/// Get all orders with a specific status
	pub async fn get_orders_by_status(&self, status: OrderStatus) -> Vec<OrderId> {
		self.statuses
			.iter()
			.filter(|entry| *entry.value() == status)
			.map(|entry| *entry.key())
			.collect()
	}
}

impl Default for OrderLifecycle {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::common::Bytes32;

	#[tokio::test]
	async fn test_lifecycle_tracking() {
		let lifecycle = OrderLifecycle::new();
		let order_id = Bytes32::from([1u8; 32]);

		// Initially no status
		assert!(lifecycle.get_status(&order_id).await.is_none());

		// Update status
		lifecycle
			.update_status(order_id, OrderStatus::Discovered)
			.await;
		assert_eq!(
			lifecycle.get_status(&order_id).await,
			Some(OrderStatus::Discovered)
		);

		// Update to new status
		lifecycle
			.update_status(order_id, OrderStatus::Filling)
			.await;
		assert_eq!(
			lifecycle.get_status(&order_id).await,
			Some(OrderStatus::Filling)
		);

		// Remove
		lifecycle.remove(&order_id).await;
		assert!(lifecycle.get_status(&order_id).await.is_none());
	}

	#[tokio::test]
	async fn test_get_orders_by_status() {
		let lifecycle = OrderLifecycle::new();

		let order1 = Bytes32::from([1u8; 32]);
		let order2 = Bytes32::from([2u8; 32]);
		let order3 = Bytes32::from([3u8; 32]);

		lifecycle.update_status(order1, OrderStatus::Ready).await;
		lifecycle.update_status(order2, OrderStatus::Ready).await;
		lifecycle.update_status(order3, OrderStatus::Filling).await;

		let ready_orders = lifecycle.get_orders_by_status(OrderStatus::Ready).await;
		assert_eq!(ready_orders.len(), 2);
		assert!(ready_orders.contains(&order1));
		assert!(ready_orders.contains(&order2));

		let filling_orders = lifecycle.get_orders_by_status(OrderStatus::Filling).await;
		assert_eq!(filling_orders.len(), 1);
		assert!(filling_orders.contains(&order3));
	}
}
