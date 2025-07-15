//! Order type registry for multiple order standards.

use solver_types::errors::{Result, SolverError};
use solver_types::orders::OrderStandard;
use std::collections::HashMap;
use tracing::debug;

use crate::factory::OrderFactory;

/// Enum containing all supported order factory types
#[derive(Clone)]
pub enum OrderFactoryType {
	EIP7683(crate::implementations::eip7683::EIP7683OrderFactory),
	// Add more factory types here as needed
	// EIP1234(crate::implementations::eip1234::EIP1234OrderFactory),
}

impl OrderFactoryType {
	/// Get the standard this factory handles
	pub fn standard(&self) -> OrderStandard {
		match self {
			Self::EIP7683(_) => OrderStandard::EIP7683,
			// Add more standards here
		}
	}

	/// Parse order data using the appropriate factory
	pub async fn parse_order(&self, data: &[u8]) -> Result<Box<dyn crate::Order>> {
		match self {
			Self::EIP7683(factory) => factory.parse_order(data).await,
			// Add more factories here
		}
	}

	/// Validate format using the appropriate factory
	pub async fn validate_format(&self, data: &[u8]) -> Result<()> {
		match self {
			Self::EIP7683(factory) => factory.validate_format(data).await,
			// Add more factories here
		}
	}
}

/// Registry for managing multiple order factories
pub struct OrderRegistry {
	factories: HashMap<OrderStandard, OrderFactoryType>,
}

impl OrderRegistry {
	/// Create new registry with default factories
	pub fn new() -> Self {
		let mut factories = HashMap::new();

		// Register EIP-7683 factory by default
		let eip7683_factory =
			OrderFactoryType::EIP7683(crate::implementations::eip7683::EIP7683OrderFactory::new());
		factories.insert(OrderStandard::EIP7683, eip7683_factory);

		Self { factories }
	}

	/// Register a new factory for a specific order standard
	pub fn register_factory(&mut self, factory: OrderFactoryType) {
		let standard = factory.standard();
		debug!("Registering factory for standard: {}", standard);
		self.factories.insert(standard, factory);
	}

	/// Get available order standards
	pub fn supported_standards(&self) -> Vec<OrderStandard> {
		self.factories.keys().cloned().collect()
	}

	/// Parse raw order data into an Order instance
	/// Tries each registered factory until one succeeds
	pub async fn parse_order(&self, data: &[u8]) -> Result<crate::OrderImpl> {
		debug!("Parsing order data ({} bytes)", data.len());

		// Try to identify which standard this order belongs to
		// For now, we'll try each factory until one succeeds
		for (standard, factory) in &self.factories {
			match factory.parse_order(data).await {
				Ok(order_box) => {
					debug!("Successfully parsed order using {} factory", standard);
					return self.convert_to_order_impl(order_box, standard);
				}
				Err(_) => continue,
			}
		}

		Err(SolverError::Order(
			"No factory could parse the order data".to_string(),
		))
	}

	/// Parse order data with a specific standard
	pub async fn parse_order_with_standard(
		&self,
		standard: &OrderStandard,
		data: &[u8],
	) -> Result<crate::OrderImpl> {
		debug!(
			"Parsing order data ({} bytes) with standard: {}",
			data.len(),
			standard
		);

		let factory = self.factories.get(standard).ok_or_else(|| {
			SolverError::Order(format!("No factory registered for standard: {}", standard))
		})?;

		let order_box = factory.parse_order(data).await?;
		self.convert_to_order_impl(order_box, standard)
	}

	/// Convert Box<dyn Order> to OrderImpl using exhaustive matching
	fn convert_to_order_impl(
		&self,
		order_box: Box<dyn crate::Order>,
		standard: &OrderStandard,
	) -> Result<crate::OrderImpl> {
		match standard {
			OrderStandard::EIP7683 => {
				// Use match for EIP7683 order types
				if let Some(gasless_order) = order_box
					.as_any()
					.downcast_ref::<crate::implementations::eip7683::GaslessOrder>(
				) {
					Ok(crate::OrderImpl::GaslessOrder(gasless_order.clone()))
				} else if let Some(onchain_order) = order_box
					.as_any()
					.downcast_ref::<crate::implementations::eip7683::OnchainOrder>(
				) {
					Ok(crate::OrderImpl::OnchainOrder(onchain_order.clone()))
				} else {
					Err(SolverError::Order("Unknown EIP7683 order type".to_string()))
				}
			}
			OrderStandard::Custom(name) => Err(SolverError::Order(format!(
				"Custom standard '{}' not yet implemented",
				name
			))), // Add more standards here as they are implemented
		}
	}

	/// Validate order data format
	pub async fn validate_format(&self, data: &[u8]) -> Result<()> {
		// Try validation with each factory
		for factory in self.factories.values() {
			if factory.validate_format(data).await.is_ok() {
				return Ok(());
			}
		}

		Err(SolverError::Order(
			"Order data format not recognized by any factory".to_string(),
		))
	}

	/// Validate order data format with a specific standard
	pub async fn validate_format_with_standard(
		&self,
		standard: &OrderStandard,
		data: &[u8],
	) -> Result<()> {
		let factory = self.factories.get(standard).ok_or_else(|| {
			SolverError::Order(format!("No factory registered for standard: {}", standard))
		})?;

		factory.validate_format(data).await
	}

	/// Get all event signatures from registered factories
	pub fn get_event_signatures(&self) -> Vec<solver_types::common::Bytes32> {
		let mut signatures = Vec::new();

		for factory in self.factories.values() {
			match factory {
				OrderFactoryType::EIP7683(f) => {
					signatures.extend(f.event_signatures());
				} // Add more factory types here as they are implemented
			}
		}

		// Remove duplicates
		signatures.sort();
		signatures.dedup();

		signatures
	}
}

impl Default for OrderRegistry {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::errors::SolverError;

	#[test]
	fn test_registry_creation() {
		let registry = OrderRegistry::new();
		// Should have EIP7683 factory registered by default
		assert_eq!(registry.factories.len(), 1);
		assert!(registry.factories.contains_key(&OrderStandard::EIP7683));
	}

	#[test]
	fn test_supported_standards() {
		let registry = OrderRegistry::new();
		let standards = registry.supported_standards();
		assert_eq!(standards.len(), 1);
		assert!(standards.contains(&OrderStandard::EIP7683));
	}

	#[test]
	fn test_register_factory() {
		let mut registry = OrderRegistry::new();
		assert_eq!(registry.factories.len(), 1);

		// Register the same factory type again (simulating a different standard)
		let new_factory =
			OrderFactoryType::EIP7683(crate::implementations::eip7683::EIP7683OrderFactory::new());
		registry.register_factory(new_factory);

		// Should still have 1 factory since we're overwriting the same standard
		assert_eq!(registry.factories.len(), 1);
	}

	#[tokio::test]
	async fn test_validate_empty_data() {
		let registry = OrderRegistry::new();
		let result = registry.validate_format(&[]).await;
		assert!(result.is_err());

		if let Err(SolverError::Order(msg)) = result {
			assert!(msg.contains("Empty") || msg.contains("not recognized"));
		} else {
			panic!("Expected Order error");
		}
	}

	#[tokio::test]
	async fn test_validate_with_standard() {
		let registry = OrderRegistry::new();
		let result = registry
			.validate_format_with_standard(&OrderStandard::EIP7683, &[])
			.await;
		assert!(result.is_err());

		if let Err(SolverError::Order(msg)) = result {
			assert!(msg.contains("Empty"));
		} else {
			panic!("Expected Order error");
		}
	}

	#[tokio::test]
	async fn test_validate_unknown_standard() {
		let registry = OrderRegistry::new();
		let unknown_standard = OrderStandard::Custom("Unknown".to_string());
		let result = registry
			.validate_format_with_standard(&unknown_standard, &[1, 2, 3])
			.await;
		assert!(result.is_err());

		if let Err(SolverError::Order(msg)) = result {
			assert!(msg.contains("No factory registered"));
		} else {
			panic!("Expected Order error about missing factory");
		}
	}

	#[tokio::test]
	async fn test_validate_short_data() {
		let registry = OrderRegistry::new();
		let result = registry.validate_format(&[1, 2, 3]).await;
		assert!(result.is_err());
	}
}
