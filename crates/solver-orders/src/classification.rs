//! Order classification for solver decision making.

use solver_types::{
	errors::Result,
	orders::{Order, OrderSemantics},
};

/// Order classification result
#[derive(Debug, Clone)]
pub struct OrderClassification {
	pub semantics: OrderSemantics,
	pub is_market_order: bool,
	pub urgency: Urgency,
	pub estimated_profit_bps: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Urgency {
	High,   // Near expiration
	Normal, // Standard timing
	Low,    // Plenty of time
}

/// Service for classifying orders
pub struct OrderClassifier {
	// In production, would have price feeds, etc.
}

impl Default for OrderClassifier {
	fn default() -> Self {
		Self::new()
	}
}

impl OrderClassifier {
	pub fn new() -> Self {
		Self {}
	}

	/// Classify an order for solver decision making
	pub async fn classify(&self, order: &dyn Order) -> Result<OrderClassification> {
		let semantics = order.semantics();

		// Determine urgency based on time to expiration
		let now = chrono::Utc::now().timestamp() as u64;
		let time_left = order.expires_at().saturating_sub(now);

		let urgency = match time_left {
			0..=60 => Urgency::High,     // Less than 1 minute
			61..=300 => Urgency::Normal, // 1-5 minutes
			_ => Urgency::Low,           // More than 5 minutes
		};

		// For EIP-7683, we'd need to analyze the orderData to determine
		// if it's a market order vs limit order
		let is_market_order = matches!(&semantics, OrderSemantics::Swap { .. });

		Ok(OrderClassification {
			semantics,
			is_market_order,
			urgency,
			estimated_profit_bps: None, // Would calculate based on current prices
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;
	use solver_types::common::{Address, Timestamp, U256};
	use solver_types::orders::{Input, OrderStandard, Output};
	use std::any::Any;

	// Mock order for testing
	#[derive(Debug)]
	struct MockOrder {
		expires_at: Timestamp,
		semantics: OrderSemantics,
	}

	#[async_trait]
	impl Order for MockOrder {
		fn id(&self) -> solver_types::orders::OrderId {
			solver_types::common::Bytes32::zero()
		}
		fn standard(&self) -> OrderStandard {
			OrderStandard::EIP7683
		}
		fn origin_chain(&self) -> solver_types::chains::ChainId {
			solver_types::chains::ChainId(1)
		}
		fn destination_chains(&self) -> Vec<solver_types::chains::ChainId> {
			vec![solver_types::chains::ChainId(1)]
		}
		fn created_at(&self) -> Timestamp {
			0
		}
		fn expires_at(&self) -> Timestamp {
			self.expires_at
		}
		async fn validate(&self) -> solver_types::errors::Result<()> {
			Ok(())
		}
		fn semantics(&self) -> OrderSemantics {
			self.semantics.clone()
		}
		async fn to_fill_instructions(
			&self,
		) -> solver_types::errors::Result<Vec<solver_types::orders::FillInstruction>> {
			Ok(vec![])
		}
		fn as_any(&self) -> &dyn Any {
			self
		}
		fn user(&self) -> Address {
			Address::from([1u8; 20])
		}
		fn inputs(&self) -> Result<Vec<Input>> {
			Ok(vec![Input {
				token: Address::from([2u8; 20]),
				amount: U256::from(1_000_000_000_000_000_000u64),
			}])
		}
		fn outputs(&self) -> Result<Vec<Output>> {
			Ok(vec![Output {
				token: Address::from([3u8; 20]),
				amount: U256::from(1_000_000_000_000_000_000u64),
				recipient: Address::from([4u8; 20]),
				chain_id: solver_types::chains::ChainId(1),
			}])
		}
	}

	#[tokio::test]
	async fn test_urgency_classification() {
		let classifier = OrderClassifier::new();
		let now = chrono::Utc::now().timestamp() as u64;

		// High urgency - expires in 30 seconds
		let order = MockOrder {
			expires_at: now + 30,
			semantics: OrderSemantics::Custom("test".to_string()),
		};
		let result = classifier.classify(&order).await.unwrap();
		assert_eq!(result.urgency, Urgency::High);

		// Normal urgency - expires in 3 minutes
		let order = MockOrder {
			expires_at: now + 180,
			semantics: OrderSemantics::Custom("test".to_string()),
		};
		let result = classifier.classify(&order).await.unwrap();
		assert_eq!(result.urgency, Urgency::Normal);

		// Low urgency - expires in 10 minutes
		let order = MockOrder {
			expires_at: now + 600,
			semantics: OrderSemantics::Custom("test".to_string()),
		};
		let result = classifier.classify(&order).await.unwrap();
		assert_eq!(result.urgency, Urgency::Low);
	}
}
