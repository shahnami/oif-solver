//! # Order Plugin Types
//!
//! Defines types and traits for order processing and validation.
//!
//! This module provides the infrastructure for plugins that handle different
//! order formats and protocols. It supports parsing, validation, estimation,
//! and transaction generation for various cross-chain order types.

use crate::{DeliveryRequest, FillEvent, OrderEvent, PluginConfig};

use super::delivery::TransactionRequest;
use super::settlement::SettlementRequest;
use super::{Address, BasePlugin, ChainId, PluginResult, Timestamp};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

/// Core Order trait that all order types must implement.
///
/// Defines the essential interface for cross-chain orders, providing
/// methods for identification, validation, and serialization.
pub trait Order: Send + Sync + Debug + Clone {
	/// Type used for order identification
	type Id: Send + Sync + Clone + Debug + PartialEq + Eq + std::hash::Hash;
	/// Type for order-specific metadata
	type Metadata: Send + Sync + Clone + Debug + Serialize + for<'de> Deserialize<'de>;

	/// Unique identifier for this order.
	///
	/// Returns a unique ID that can be used to track and reference
	/// this order throughout its lifecycle.
	fn id(&self) -> Self::Id;

	/// Address of the user who created this order.
	///
	/// Returns the blockchain address of the order originator.
	fn user(&self) -> Address;

	/// Chain where the order originates.
	///
	/// Returns the chain ID where the order was created and
	/// where source funds are located.
	fn origin_chain(&self) -> ChainId;

	/// Chain where the order should be fulfilled.
	///
	/// Returns the chain ID where the order execution should
	/// take place and destination funds delivered.
	fn destination_chain(&self) -> ChainId;

	/// When the order was created.
	///
	/// Returns the timestamp of order creation.
	fn created_at(&self) -> Timestamp;

	/// When the order expires.
	///
	/// Returns the timestamp after which the order is no longer valid.
	fn expires_at(&self) -> Timestamp;

	/// Order-specific metadata.
	///
	/// Returns additional metadata specific to this order type.
	fn metadata(&self) -> &Self::Metadata;

	/// Serialize order to bytes for storage.
	///
	/// Encodes the order into a byte representation suitable for
	/// storage or transmission.
	fn encode(&self) -> PluginResult<Bytes>;

	/// Deserialize order from bytes.
	///
	/// Reconstructs an order instance from its byte representation.
	fn decode(data: &[u8]) -> PluginResult<Self>
	where
		Self: Sized;

	/// Check if order is expired.
	///
	/// Returns true if the current time is past the order's
	/// expiration timestamp.
	fn is_expired(&self) -> bool {
		let now = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();
		now > self.expires_at()
	}

	/// Validate order signature and structure.
	///
	/// Performs comprehensive validation of the order including
	/// signature verification and structural integrity checks.
	fn validate(&self) -> PluginResult<()>;
}

/// Plugin interface for handling specific order types.
///
/// Core trait that order plugins must implement to integrate with the
/// solver's order processing system. Provides methods for parsing,
/// validation, and transaction generation.
#[async_trait]
pub trait OrderPlugin: BasePlugin {
	/// The concrete order type this plugin handles
	type Order: Order;
	/// The order ID type used by this plugin
	type OrderId: Send + Sync + Clone + Debug + PartialEq + Eq + std::hash::Hash;
	/// Context type for order parsing
	type ParseContext: Send + Sync + Clone + Debug;

	/// Parse raw bytes into a typed order.
	///
	/// Converts raw order data into a strongly-typed order instance,
	/// optionally using provided context for additional information.
	async fn parse_order(
		&self,
		data: &[u8],
		context: Option<Self::ParseContext>,
	) -> PluginResult<Self::Order>;

	/// Validate order according to this standard's rules.
	///
	/// Performs protocol-specific validation checks on the order
	/// and returns detailed validation results.
	async fn validate_order(&self, order: &Self::Order) -> PluginResult<OrderValidation>;

	/// Extract searchable metadata from order.
	///
	/// Extracts indexable metadata from the order for efficient
	/// searching and filtering.
	async fn extract_metadata(&self, order: &Self::Order) -> PluginResult<OrderIndexMetadata>;

	/// Get order by ID if this plugin manages it.
	///
	/// Retrieves a previously stored order by its identifier.
	async fn get_order(&self, id: &Self::OrderId) -> PluginResult<Option<Self::Order>>;

	/// Check if this plugin can handle the given order data.
	///
	/// Quickly determines if the provided data represents an order
	/// type that this plugin can process.
	fn can_handle(&self, data: &[u8]) -> bool;

	/// Get the order type identifier.
	///
	/// Returns a unique identifier for the order protocol this
	/// plugin handles.
	fn order_type(&self) -> &'static str;

	/// Estimate order parameters (fees, gas, etc.).
	///
	/// Provides cost and feasibility estimates for order execution.
	async fn estimate_order(&self, order: &Self::Order) -> PluginResult<OrderEstimate>;

	/// Create a delivery request for filling this order.
	///
	/// Generates the transaction data needed to fill the order
	/// on the destination chain.
	async fn create_fill_request(&self, order: &Self::Order) -> PluginResult<DeliveryRequest>;

	/// Create a settlement request for claiming funds after fill.
	///
	/// Generates the settlement transaction for claiming rewards.
	/// Returns None if this order type doesn't support settlement.
	async fn create_settlement_request(
		&self,
		order: &Self::Order,
		fill_timestamp: Timestamp,
	) -> PluginResult<Option<SettlementRequest>>;
}

/// Order validation result.
///
/// Contains the outcome of order validation including any
/// errors or warnings found during the validation process.
#[derive(Debug, Clone)]
pub struct OrderValidation {
	/// Whether the order passed all validation checks
	pub is_valid: bool,
	/// List of validation errors found
	pub errors: Vec<ValidationError>,
}

impl OrderValidation {
	/// Create a successful validation result.
	pub fn valid() -> Self {
		Self {
			is_valid: true,
			errors: Vec::new(),
		}
	}

	/// Create a failed validation result with errors.
	pub fn invalid(errors: Vec<ValidationError>) -> Self {
		Self {
			is_valid: false,
			errors,
		}
	}
}

/// Validation error details.
///
/// Provides detailed information about validation failures
/// including error codes and affected fields.
#[derive(Debug, Clone)]
pub struct ValidationError {
	/// Error code for programmatic handling
	pub code: String,
	/// Human-readable error message
	pub message: String,
	/// Field name that caused the error (if applicable)
	pub field: Option<String>,
}

/// Metadata for indexing and searching orders.
///
/// Contains searchable fields extracted from orders to enable
/// efficient querying and filtering in the order database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderIndexMetadata {
	/// Type identifier for the order protocol
	pub order_type: String,
	/// Address of the order creator
	pub user: Address,
	/// Chain where the order originates
	pub origin_chain: ChainId,
	/// Chain where the order should be executed
	pub destination_chain: ChainId,
	/// Order creation timestamp
	pub created_at: Timestamp,
	/// Order expiration timestamp
	pub expires_at: Timestamp,
	/// Current order status
	pub status: OrderStatus,
	/// Searchable tags for categorization
	pub tags: Vec<String>,
	/// Additional custom searchable fields
	pub custom_fields: HashMap<String, String>,
}

/// Order lifecycle status.
///
/// Tracks the current state of an order as it progresses
/// through the fulfillment pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrderStatus {
	/// Order is awaiting discovery
	Pending,
	/// Order has been discovered by the solver
	Discovered,
	/// Order has passed validation checks
	Validated,
	/// Order is being processed for execution
	Processing,
	/// Order has been successfully filled
	Filled,
	/// Order has been settled and rewards claimed
	Settled,
	/// Order expired before execution
	Expired,
	/// Order was cancelled by user
	Cancelled,
	/// Order processing failed
	Failed,
}

/// Order cost and parameter estimates.
///
/// Provides estimated costs and execution parameters for
/// an order to help with decision making.
#[derive(Debug, Clone)]
pub struct OrderEstimate {
	/// Estimated time to fill the order in seconds
	pub estimated_fill_time: Option<u64>, // seconds
	/// Estimated gas cost for execution
	pub estimated_gas_cost: Option<u64>,
	/// Breakdown of estimated fees by type
	pub estimated_fees: HashMap<String, u64>, // fee type -> amount
	/// Feasibility score from 0.0 (impossible) to 1.0 (highly feasible)
	pub feasibility_score: f64, // 0.0 to 1.0
	/// Recommendations for order optimization
	pub recommendations: Vec<String>,
}

/// Factory trait for creating order plugins.
///
/// Provides a standardized interface for instantiating order
/// plugins with proper type associations and configuration.
pub trait OrderPluginFactory: Send + Sync {
	#[allow(clippy::type_complexity)]
	/// Create a new instance of the order plugin with configuration.
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

	/// Get the unique type identifier for this plugin factory.
	fn plugin_type(&self) -> &'static str;

	/// The order type this factory produces plugins for
	type Order: Order;
	/// The order ID type used by produced plugins
	type OrderId: Send + Sync + Clone + Debug + PartialEq + Eq + std::hash::Hash;
	/// The parse context type used by produced plugins
	type ParseContext: Send + Sync + Clone + Debug;
}

/// Trait for processing OrderEvents and creating TransactionRequests.
///
/// Bridges the gap between generic order plugins and the concrete
/// delivery service by converting order events into executable transactions.
#[async_trait]
pub trait OrderProcessor: Send + Sync {
	/// Process an OrderEvent and return a TransactionRequest for filling the order.
	///
	/// Converts a discovered order event into a transaction request
	/// that can be submitted for execution.
	async fn process_order_event(
		&self,
		event: &OrderEvent,
	) -> PluginResult<Option<TransactionRequest>>;

	/// Create a settlement transaction for a filled order.
	///
	/// Generates a settlement transaction when an order has been
	/// successfully filled and rewards need to be claimed.
	async fn process_fill_event(
		&self,
		event: &FillEvent,
	) -> PluginResult<Option<TransactionRequest>>;

	/// Check if this processor can handle the given order source.
	///
	/// Determines if this processor supports orders from the
	/// specified source system or protocol.
	fn can_handle_source(&self, source: &str) -> bool;
}

/// Factory trait for creating order processors.
///
/// Provides a standardized interface for instantiating order
/// processors with configuration and capability reporting.
pub trait OrderProcessorFactory: Send + Sync {
	/// Create an order processor instance.
	///
	/// Instantiates a new order processor with the provided configuration.
	fn create_processor(&self, config: PluginConfig) -> PluginResult<Arc<dyn OrderProcessor>>;

	/// Get the plugin type identifier.
	///
	/// Returns a unique identifier for this processor type.
	fn plugin_type(&self) -> &'static str;

	/// Get the source types this processor can handle.
	///
	/// Returns a list of order source identifiers that this
	/// processor is capable of handling.
	fn source_types(&self) -> Vec<String>;
}
