//! Core types for state management.

use serde::{Deserialize, Serialize};
use solver_discovery::OrderStatus;
use solver_orders::classification::Urgency;
use solver_types::{errors::SolverError, orders::OrderId};

/// Order state representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderState {
	/// Order ID
	pub id: OrderId,
	/// Raw order data
	pub order_data: Vec<u8>,
	/// Current status
	pub status: OrderStatus,
	/// Priority for queue ordering
	pub priority: OrderPriority,
	/// Discovery timestamp
	pub discovered_at: u64,
	/// Queue entry timestamp
	pub queued_at: Option<u64>,
	/// Processing start timestamp
	pub processed_at: Option<u64>,
	/// Completion timestamp
	pub completed_at: Option<u64>,
	/// Number of processing attempts
	pub attempts: u32,
	/// Last error if any
	pub last_error: Option<String>,
}

/// Order priority for queue ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OrderPriority {
	/// Base priority (0-100)
	pub base: u8,
	/// Urgency modifier
	pub urgency_modifier: i8,
	/// Economic value modifier
	pub value_modifier: i8,
	/// Age modifier (increases over time)
	pub age_modifier: i8,
}

impl OrderPriority {
	/// Calculate priority based on various factors
	pub fn calculate(urgency: Urgency, economic_value: Option<u64>, age_seconds: u64) -> Self {
		let base = 50u8; // Default middle priority

		let urgency_modifier = match urgency {
			Urgency::High => 10,
			Urgency::Normal => 0,
			Urgency::Low => -10,
		};

		let value_modifier = if let Some(value) = economic_value {
			// Higher value = higher priority
			match value {
				v if v > 1_000_000 => 15,
				v if v > 100_000 => 10,
				v if v > 10_000 => 5,
				_ => 0,
			}
		} else {
			0
		};

		let age_modifier = match age_seconds {
			a if a > 3600 => 10, // Over 1 hour
			a if a > 1800 => 5,  // Over 30 minutes
			a if a > 600 => 2,   // Over 10 minutes
			_ => 0,
		};

		Self {
			base,
			urgency_modifier,
			value_modifier,
			age_modifier,
		}
	}

	/// Get total priority score
	pub fn score(&self) -> i32 {
		self.base as i32
			+ self.urgency_modifier as i32
			+ self.value_modifier as i32
			+ self.age_modifier as i32
	}
}

/// State management errors
#[derive(Debug, thiserror::Error)]
pub enum StateError {
	#[error("Queue is full")]
	QueueFull,

	#[error("Order not found: {0}")]
	OrderNotFound(OrderId),

	#[error("Invalid state transition: {0}")]
	InvalidTransition(String),

	#[error("Storage error: {0}")]
	StorageError(String),
}

impl From<StateError> for SolverError {
	fn from(err: StateError) -> Self {
		SolverError::State(err.to_string())
	}
}
