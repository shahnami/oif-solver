//! Order standard processors for different intent formats.
//!
//! This module provides a pluggable architecture for processing orders
//! from different standards (e.g., EIP-7683, custom protocols). Each
//! processor implements the `OrderStandardProcessor` trait to handle
//! its specific order format.
//!
//! # Architecture
//!
//! The module is designed to be extensible:
//! - Define new processors by implementing `OrderStandardProcessor`
//! - Register processors in the `StandardProcessorRegistry`
//! - The discovery service automatically routes intents to the appropriate processor

pub mod eip7683;

use crate::types::RawIntent;
use async_trait::async_trait;
use solver_orders::OrderRegistry;
use solver_types::errors::Result;
use std::sync::Arc;

/// Trait for processing orders of different standards.
///
/// Each order standard (e.g., EIP-7683, custom protocols) should implement
/// this trait to provide its specific processing logic. The trait enables
/// the discovery service to handle multiple order formats in a pluggable way.
#[async_trait]
pub trait OrderStandardProcessor: Send + Sync {
	/// Returns the name of the standard this processor handles.
	///
	/// This is used for logging and debugging purposes.
	fn standard_name(&self) -> &str;

	/// Checks if this processor can handle the given intent.
	///
	/// # Arguments
	///
	/// * `order_type_hint` - Optional hint about the order type. If `None`,
	///   the processor may choose to accept it as a default handler.
	///
	/// # Returns
	///
	/// `true` if this processor can handle the intent, `false` otherwise.
	fn can_process(&self, order_type_hint: Option<&str>) -> bool;

	/// Processes a raw intent into an order implementation and formatted data.
	///
	/// This method takes a raw intent from a discovery source and transforms it
	/// into a structured order that the solver can work with.
	///
	/// # Arguments
	///
	/// * `raw_intent` - The raw intent data from a discovery source
	/// * `order_registry` - Registry for parsing and managing orders
	///
	/// # Returns
	///
	/// A tuple containing:
	/// - The parsed order implementation
	/// - The formatted order data as bytes
	///
	/// # Errors
	///
	/// Returns an error if the intent cannot be parsed or processed.
	async fn process_intent(
		&self,
		raw_intent: RawIntent,
		order_registry: &Arc<OrderRegistry>,
	) -> Result<(solver_orders::OrderImpl, Vec<u8>)>;
}

/// Registry of order standard processors.
///
/// The registry maintains a collection of processors for different order
/// standards and provides a way to route intents to the appropriate
/// processor based on order type hints.
///
/// # Default Processors
///
/// By default, the registry includes:
/// - EIP-7683 processor (handles both on-chain and off-chain EIP-7683 orders)
///
/// # Adding Custom Processors
///
/// To add support for new order standards, create a new processor implementing
/// `OrderStandardProcessor` and add it to the registry during initialization.
#[derive(Default)]
pub struct StandardProcessorRegistry {
	/// Collection of registered processors
	processors: Vec<Box<dyn OrderStandardProcessor>>,
}

impl StandardProcessorRegistry {
	/// Creates a new registry with default processors.
	///
	/// Initializes the registry with:
	/// - EIP-7683 processor
	pub fn new() -> Self {
		Self {
			processors: vec![Box::new(eip7683::EIP7683Processor::new())],
		}
	}

	/// Gets a processor for the given order type hint.
	///
	/// Searches through registered processors to find one that can handle
	/// the given order type. The first matching processor is returned.
	///
	/// # Arguments
	///
	/// * `order_type_hint` - Optional hint about the order type. If `None`,
	///   returns a processor that accepts untyped orders (e.g., default handler).
	///
	/// # Returns
	///
	/// * `Some(&dyn OrderStandardProcessor)` - A reference to the matching processor
	/// * `None` - If no processor can handle the given order type
	pub fn get_processor(
		&self,
		order_type_hint: Option<&str>,
	) -> Option<&dyn OrderStandardProcessor> {
		self.processors
			.iter()
			.find(|p| p.can_process(order_type_hint))
			.map(|p| p.as_ref())
	}
}
