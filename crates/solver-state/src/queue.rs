//! Priority queue for order processing.

use crate::types::{OrderPriority, StateError};
use priority_queue::PriorityQueue;
use solver_types::orders::OrderId;
use std::cmp::Reverse;
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

/// Thread-safe priority queue for orders
pub struct OrderQueue {
	/// Priority queue (higher score = higher priority)
	queue: Arc<Mutex<PriorityQueue<OrderId, i32>>>,
	/// Maximum queue size
	max_size: usize,
}

impl OrderQueue {
	/// Create new order queue
	pub fn new(max_size: usize) -> Self {
		Self {
			queue: Arc::new(Mutex::new(PriorityQueue::new())),
			max_size,
		}
	}

	/// Enqueue an order with priority
	pub fn enqueue(&self, order_id: OrderId, priority: OrderPriority) -> Result<(), StateError> {
		let mut queue = self.queue.lock().unwrap();

		if queue.len() >= self.max_size {
			return Err(StateError::QueueFull);
		}

		let score = priority.score();
		queue.push(order_id, score);

		debug!("Enqueued order {} with priority score {}", order_id, score);
		Ok(())
	}

	/// Dequeue highest priority order
	pub fn dequeue(&self) -> Option<OrderId> {
		let mut queue = self.queue.lock().unwrap();
		queue.pop().map(|(order_id, score)| {
			debug!("Dequeued order {} with priority score {}", order_id, score);
			order_id
		})
	}

	/// Peek at highest priority order without removing
	pub fn peek(&self) -> Option<OrderId> {
		let queue = self.queue.lock().unwrap();
		queue.peek().map(|(order_id, _)| *order_id)
	}

	/// Update order priority
	pub fn update_priority(&self, order_id: OrderId, priority: OrderPriority) -> bool {
		let mut queue = self.queue.lock().unwrap();
		let score = priority.score();

		match queue.change_priority(&order_id, score) {
			Some(_) => {
				debug!("Updated priority for order {} to score {}", order_id, score);
				true
			}
			None => {
				warn!(
					"Failed to update priority for order {} - not in queue",
					order_id
				);
				false
			}
		}
	}

	/// Remove order from queue
	pub fn remove(&self, order_id: &OrderId) -> bool {
		let mut queue = self.queue.lock().unwrap();
		queue.remove(order_id).is_some()
	}

	/// Check if order is in queue
	pub fn contains(&self, order_id: &OrderId) -> bool {
		let queue = self.queue.lock().unwrap();
		queue.get(order_id).is_some()
	}

	/// Get current queue size
	pub fn len(&self) -> usize {
		let queue = self.queue.lock().unwrap();
		queue.len()
	}

	/// Check if queue is empty
	pub fn is_empty(&self) -> bool {
		let queue = self.queue.lock().unwrap();
		queue.is_empty()
	}

	/// Get all queued orders (sorted by priority)
	pub fn get_all(&self) -> Vec<(OrderId, i32)> {
		let queue = self.queue.lock().unwrap();
		let mut items: Vec<_> = queue.iter().map(|(id, score)| (*id, *score)).collect();
		items.sort_by_key(|&(_, score)| Reverse(score));
		items
	}

	/// Clear the queue
	pub fn clear(&self) {
		let mut queue = self.queue.lock().unwrap();
		queue.clear();
		debug!("Cleared order queue");
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_orders::classification::Urgency;
	use solver_types::common::Bytes32;

	#[test]
	fn test_priority_queue_ordering() {
		let queue = OrderQueue::new(10);

		// Add orders with different priorities
		let order1 = Bytes32::from([1u8; 32]);
		let order2 = Bytes32::from([2u8; 32]);
		let order3 = Bytes32::from([3u8; 32]);

		let priority1 = OrderPriority::calculate(Urgency::Low, None, 0);
		let priority2 = OrderPriority::calculate(Urgency::High, Some(1_000_000), 0);
		let priority3 = OrderPriority::calculate(Urgency::Normal, None, 0);

		queue.enqueue(order1, priority1).unwrap();
		queue.enqueue(order2, priority2).unwrap();
		queue.enqueue(order3, priority3).unwrap();

		// Should dequeue in priority order (highest first)
		assert_eq!(queue.dequeue(), Some(order2)); // High urgency + high value
		assert_eq!(queue.dequeue(), Some(order3)); // Normal
		assert_eq!(queue.dequeue(), Some(order1)); // Low
	}

	#[test]
	fn test_queue_capacity() {
		let queue = OrderQueue::new(2);

		let order1 = Bytes32::from([1u8; 32]);
		let order2 = Bytes32::from([2u8; 32]);
		let order3 = Bytes32::from([3u8; 32]);

		let priority = OrderPriority::calculate(Urgency::Normal, None, 0);

		assert!(queue.enqueue(order1, priority).is_ok());
		assert!(queue.enqueue(order2, priority).is_ok());
		assert!(matches!(
			queue.enqueue(order3, priority),
			Err(StateError::QueueFull)
		));
	}

	#[test]
	fn test_priority_update() {
		let queue = OrderQueue::new(10);

		let order = Bytes32::from([1u8; 32]);
		let initial_priority = OrderPriority::calculate(Urgency::Low, None, 0);
		let updated_priority = OrderPriority::calculate(Urgency::High, Some(1_000_000), 3600);

		queue.enqueue(order, initial_priority).unwrap();

		assert!(queue.update_priority(order, updated_priority));

		let all = queue.get_all();
		assert_eq!(all.len(), 1);
		assert!(all[0].1 > initial_priority.score());
	}
}
