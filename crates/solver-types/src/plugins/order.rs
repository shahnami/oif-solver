// solver-types/src/plugins/order.rs

use crate::{DeliveryRequest, Event, FillEvent, OrderEvent, PluginConfig};

use super::delivery::TransactionRequest;
use super::settlement::{FillData, SettlementRequest};
use super::{Address, BasePlugin, ChainId, PluginResult, Timestamp};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// Core Order trait that all order types must implement
pub trait Order: Send + Sync + Debug + Clone {
	type Id: Send + Sync + Clone + Debug + PartialEq + Eq + std::hash::Hash;
	type Metadata: Send + Sync + Clone + Debug + Serialize + for<'de> Deserialize<'de>;

	/// Unique identifier for this order
	fn id(&self) -> Self::Id;

	/// Address of the user who created this order
	fn user(&self) -> Address;

	/// Chain where the order originates
	fn origin_chain(&self) -> ChainId;

	/// Chain where the order should be fulfilled
	fn destination_chain(&self) -> ChainId;

	/// When the order was created
	fn created_at(&self) -> Timestamp;

	/// When the order expires
	fn expires_at(&self) -> Timestamp;

	/// Order-specific metadata
	fn metadata(&self) -> &Self::Metadata;

	/// Serialize order to bytes for storage
	fn encode(&self) -> PluginResult<Bytes>;

	/// Deserialize order from bytes
	fn decode(data: &[u8]) -> PluginResult<Self>
	where
		Self: Sized;

	/// Check if order is expired
	fn is_expired(&self) -> bool {
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();
		now > self.expires_at()
	}

	/// Validate order signature and structure
	fn validate(&self) -> PluginResult<()>;
}

/// Plugin interface for handling specific order types
#[async_trait]
pub trait OrderPlugin: BasePlugin {
	type Order: Order;
	type OrderId: Send + Sync + Clone + Debug + PartialEq + Eq + std::hash::Hash;
	type ParseContext: Send + Sync + Clone + Debug;

	/// Parse raw bytes into a typed order
	async fn parse_order(
		&self,
		data: &[u8],
		context: Option<Self::ParseContext>,
	) -> PluginResult<Self::Order>;

	/// Validate order according to this standard's rules
	async fn validate_order(&self, order: &Self::Order) -> PluginResult<OrderValidation>;

	/// Extract searchable metadata from order
	async fn extract_metadata(&self, order: &Self::Order) -> PluginResult<OrderIndexMetadata>;

	/// Get order by ID if this plugin manages it
	async fn get_order(&self, id: &Self::OrderId) -> PluginResult<Option<Self::Order>>;

	/// Check if this plugin can handle the given order data
	fn can_handle(&self, data: &[u8]) -> bool;

	/// Get the order type identifier
	fn order_type(&self) -> &'static str;

	/// Estimate order parameters (fees, gas, etc.)
	async fn estimate_order(&self, order: &Self::Order) -> PluginResult<OrderEstimate>;

	/// Create a delivery request for filling this order
	/// Returns the transaction data needed to fill the order on the destination chain
	async fn create_fill_request(&self, order: &Self::Order) -> PluginResult<DeliveryRequest>;

	/// Create a settlement request for claiming funds after fill
	/// Returns None if this order type doesn't support settlement
	async fn create_settlement_request(
		&self,
		order: &Self::Order,
		fill_timestamp: Timestamp,
	) -> PluginResult<Option<SettlementRequest>>;
}

/// Order validation result
#[derive(Debug, Clone)]
pub struct OrderValidation {
	pub is_valid: bool,
	pub errors: Vec<ValidationError>,
}

impl OrderValidation {
	pub fn valid() -> Self {
		Self {
			is_valid: true,
			errors: Vec::new(),
		}
	}

	pub fn invalid(errors: Vec<ValidationError>) -> Self {
		Self {
			is_valid: false,
			errors,
		}
	}
}

#[derive(Debug, Clone)]
pub struct ValidationError {
	pub code: String,
	pub message: String,
	pub field: Option<String>,
}

/// Metadata for indexing and searching orders
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderIndexMetadata {
	pub order_type: String,
	pub user: Address,
	pub origin_chain: ChainId,
	pub destination_chain: ChainId,
	pub created_at: Timestamp,
	pub expires_at: Timestamp,
	pub status: OrderStatus,
	pub tags: Vec<String>,
	pub custom_fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrderStatus {
	Pending,
	Discovered,
	Validated,
	Processing,
	Filled,
	Settled,
	Expired,
	Cancelled,
	Failed,
}

/// Order cost and parameter estimates
#[derive(Debug, Clone)]
pub struct OrderEstimate {
	pub estimated_fill_time: Option<u64>, // seconds
	pub estimated_gas_cost: Option<u64>,
	pub estimated_fees: HashMap<String, u64>, // fee type -> amount
	pub feasibility_score: f64,               // 0.0 to 1.0
	pub recommendations: Vec<String>,
}

/// Factory trait for creating order plugins
pub trait OrderPluginFactory: Send + Sync {
	#[allow(clippy::type_complexity)]
	fn create_plugin(
		&self,
		config: super::PluginConfig,
	) -> PluginResult<
		Box<
			dyn OrderPlugin<
				Order = Self::Order,
				OrderId = Self::OrderId,
				ParseContext = Self::ParseContext,
			>,
		>,
	>;
	fn plugin_type(&self) -> &'static str;

	type Order: Order;
	type OrderId: Send + Sync + Clone + Debug + PartialEq + Eq + std::hash::Hash;
	type ParseContext: Send + Sync + Clone + Debug;
}

/// Registry for order plugins
#[derive(Default)]
pub struct OrderPluginRegistry {
	factories: HashMap<String, Box<dyn std::any::Any + Send + Sync>>,
}

impl OrderPluginRegistry {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn register<F>(&mut self, factory: F)
	where
		F: OrderPluginFactory + 'static,
	{
		self.factories
			.insert(factory.plugin_type().to_string(), Box::new(factory));
	}

	pub fn create_plugin(
		&self,
		plugin_type: &str,
		config: super::PluginConfig,
	) -> PluginResult<Box<dyn std::any::Any>> {
		let _factory = self
			.factories
			.get(plugin_type)
			.ok_or_else(|| super::PluginError::NotFound(plugin_type.to_string()))?;

		// This is a bit tricky with trait objects - in practice you'd need a more sophisticated approach
		Ok(Box::new(()) as Box<dyn std::any::Any>)
	}

	pub fn list_plugin_types(&self) -> Vec<&str> {
		self.factories.keys().map(|s| s.as_str()).collect()
	}
}

/// Trait for processing OrderEvents and creating TransactionRequests
/// This bridges the gap between order plugins (which are generic) and delivery service
#[async_trait]
pub trait OrderProcessor: Send + Sync {
	/// Process an OrderEvent and return a TransactionRequest for filling the order
	async fn process_order_event(
		&self,
		event: &OrderEvent,
	) -> PluginResult<Option<TransactionRequest>>;

	/// Create a settlement transaction for a filled order
	/// This is called when the order has been filled and needs to be settled
	async fn process_fill_event(
		&self,
		event: &FillEvent,
	) -> PluginResult<Option<TransactionRequest>>;

	/// Check if this processor can handle the given order source
	fn can_handle_source(&self, source: &str) -> bool;
}

/// Factory trait for creating order processors
pub trait OrderProcessorFactory: Send + Sync {
	/// Create an order processor instance (uninitialized)
	fn create_processor(&self, config: PluginConfig) -> PluginResult<Arc<dyn OrderProcessor>>;

	/// Get the plugin type identifier
	fn plugin_type(&self) -> &'static str;

	/// Get the source types this processor can handle
	fn source_types(&self) -> Vec<String>;
}
