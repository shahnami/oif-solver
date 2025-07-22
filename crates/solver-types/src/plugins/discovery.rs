//! # Discovery Plugin Types
//!
//! Defines types and traits for order discovery and blockchain event monitoring.
//!
//! This module provides the infrastructure for plugins that discover orders and
//! relevant events from various sources including blockchain networks, APIs,
//! message queues, and databases. It supports both historical and real-time
//! event discovery with filtering, parsing, and metadata collection.

use crate::PluginConfig;

use super::{Address, BasePlugin, ChainId, PluginResult, Timestamp, TxHash};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use tokio::sync::mpsc;

/// Discovery event representing a newly found order or relevant blockchain event.
///
/// Contains comprehensive information about discovered events including raw and
/// parsed data, blockchain context, and metadata for processing and filtering.
/// Events flow through the system from discovery plugins to order processors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryEvent {
	/// Unique identifier for this discovery event
	pub id: String,
	/// Type categorization of the discovered event
	pub event_type: EventType,
	/// Source identifier for the discovery plugin that found this event
	pub source: String,
	/// Blockchain network identifier where the event occurred
	pub chain_id: ChainId,
	/// Block number containing this event (if from blockchain)
	pub block_number: Option<u64>,
	/// Transaction hash containing this event (if from blockchain)
	pub transaction_hash: Option<TxHash>,
	/// Unix timestamp when the event occurred
	pub timestamp: Timestamp,
	/// Raw event data as received from the source
	pub raw_data: Bytes,
	/// Parsed and structured event data (if parsing was successful)
	pub parsed_data: Option<ParsedEventData>,
	/// Additional metadata about the discovery and processing
	pub metadata: EventMetadata,
}

/// Event type classification for discovered events.
///
/// Categorizes events by their semantic meaning to enable appropriate
/// handling and routing through the processing pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
	/// New order has been created on-chain or off-chain
	OrderCreated,
	/// Existing order has been cancelled by user or system
	OrderCancelled,
	/// Order has been successfully filled or executed
	OrderFilled,
	/// Order has expired based on its time constraints
	OrderExpired,
	/// Order parameters have been updated or modified
	OrderUpdated,
	/// New block has been created on the blockchain
	BlockCreated,
	/// Custom event type for protocol-specific events
	Custom(String),
}

/// Parsed and structured representation of event data.
///
/// Contains decoded and interpreted information extracted from raw event
/// data, making it easier for downstream processors to handle the event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedEventData {
	/// Unique identifier of the order associated with this event
	pub order_id: Option<String>,
	/// Address of the user who initiated the transaction
	pub user: Option<Address>,
	/// Smart contract address where the event originated
	pub contract_address: Option<Address>,
	/// Function signature or event topic that generated this event
	pub method_signature: Option<String>,
	/// Decoded parameters from the event or function call
	pub decoded_params: HashMap<String, EventParam>,
}

/// Decoded parameter types from blockchain events and function calls.
///
/// Represents the various data types that can be extracted from
/// blockchain events, supporting nested structures through arrays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventParam {
	/// Blockchain address (20 bytes)
	Address(Address),
	/// 256-bit unsigned integer stored as string to prevent overflow
	Uint256(String), // Store as string to avoid overflow
	/// Raw byte data
	Bytes(Bytes),
	/// UTF-8 string data
	String(String),
	/// Boolean value
	Bool(bool),
	/// Array of parameters supporting nested structures
	Array(Vec<EventParam>),
}

/// Additional metadata about discovered events.
///
/// Provides context about event discovery quality, timing, and
/// source-specific information for processing decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
	/// Source-specific metadata fields (protocol-dependent)
	pub source_specific: HashMap<String, String>,
	/// Confidence score for event validity (0.0 to 1.0)
	pub confidence_score: f64, // 0.0 to 1.0
	/// Time delay from event occurrence to discovery in milliseconds
	pub processing_delay: Option<u64>, // milliseconds from event to discovery
	/// Number of discovery retry attempts for this event
	pub retry_count: u32,
}

/// Event sink for plugins to send discovered events.
///
/// Provides a channel for discovery plugins to emit events into the
/// solver's event processing pipeline. Generic over event type to
/// support different event systems.
#[derive(Debug, Clone)]
pub struct EventSink<T = crate::Event> {
	sender: mpsc::UnboundedSender<T>,
}

impl<T: Send + 'static> EventSink<T> {
	/// Creates a new event sink with the provided channel sender.
	pub fn new(sender: mpsc::UnboundedSender<T>) -> Self {
		Self { sender }
	}

	/// Sends an event through the sink to the processing pipeline.
	///
	/// Returns an error if the receiving end has been closed.
	pub fn send(&self, event: T) -> PluginResult<()> {
		self.sender
			.send(event)
			.map_err(|_| super::PluginError::ExecutionFailed("Event sink closed".to_string()))
	}
}

impl EventSink<crate::Event> {
	/// Convenience method for sending discovery events.
	///
	/// Wraps the discovery event in the appropriate event variant
	/// before sending through the sink.
	pub fn send_discovery(&self, event: DiscoveryEvent) -> PluginResult<()> {
		self.send(crate::Event::Discovery(event))
	}
}

/// Event filter for targeted discovery.
///
/// Specifies criteria for selecting which events to discover and process,
/// supporting filtering by contract, event type, and block range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
	/// Contract addresses to monitor for events
	pub contract_addresses: Option<Vec<Address>>,
	/// Event signatures or method IDs to filter for
	pub event_signatures: Option<Vec<String>>,
	/// Event log topics for filtering (None for wildcard matching)
	pub topics: Option<Vec<Option<String>>>, // None for wildcard topics
	/// Starting block number for historical discovery
	pub from_block: Option<u64>,
	/// Ending block number for historical discovery
	pub to_block: Option<u64>,
}

/// Discovery status and statistics.
///
/// Provides real-time information about the discovery plugin's
/// operational state, progress, and performance metrics.
#[derive(Debug, Clone)]
pub struct DiscoveryStatus {
	/// Whether the discovery plugin is actively monitoring
	pub is_running: bool,
	/// Current block being processed (for blockchain sources)
	pub current_block: Option<u64>,
	/// Target block to sync up to (for historical sync)
	pub target_block: Option<u64>,
	/// Total number of events discovered since start
	pub events_discovered: u64,
	/// Timestamp of the most recently discovered event
	pub last_event_timestamp: Option<Timestamp>,
	/// Total number of errors encountered during discovery
	pub errors_count: u64,
	/// Average time to process each discovered event in milliseconds
	pub average_processing_time_ms: f64,
}

/// Plugin interface for event discovery.
///
/// Core trait that all discovery plugins must implement to integrate with
/// the solver's event discovery system. Supports both real-time monitoring
/// and historical event discovery across various data sources.
#[async_trait]
pub trait DiscoveryPlugin: BasePlugin {
	/// Start monitoring for events and send them to the provided sink.
	///
	/// Begins the discovery process, emitting found events through the
	/// provided sink for processing by the solver engine.
	async fn start_monitoring(&mut self, sink: EventSink<crate::Event>) -> PluginResult<()>;

	/// Stop monitoring for events.
	///
	/// Gracefully shuts down the discovery process and releases resources.
	async fn stop_monitoring(&mut self) -> PluginResult<()>;

	/// Get current discovery status and statistics.
	///
	/// Returns real-time information about the plugin's operational state
	/// and performance metrics.
	async fn get_status(&self) -> PluginResult<DiscoveryStatus>;

	/// Manually trigger discovery for a specific block range.
	///
	/// Performs historical discovery within the specified block range,
	/// returning the number of events discovered.
	async fn discover_range(
		&self,
		from_block: u64,
		to_block: u64,
		sink: EventSink<crate::Event>,
	) -> PluginResult<u64>;

	/// Get supported event types this plugin can discover.
	///
	/// Returns the list of event types this plugin is capable of
	/// discovering and processing.
	fn supported_event_types(&self) -> Vec<EventType>;

	/// Get the chain this plugin monitors.
	///
	/// Returns the blockchain network identifier for chain-specific plugins.
	fn chain_id(&self) -> ChainId;

	/// Check if plugin can handle the given contract address.
	///
	/// Validates whether this plugin supports monitoring events from
	/// the specified contract address.
	async fn can_monitor_contract(&self, contract_address: &Address) -> PluginResult<bool>;

	/// Subscribe to specific events using filters.
	///
	/// Configures the plugin to monitor only events matching the
	/// provided filter criteria.
	async fn subscribe_to_events(&mut self, filters: Vec<EventFilter>) -> PluginResult<()>;

	/// Unsubscribe from previously subscribed event filters.
	///
	/// Removes the specified filters from the active monitoring set.
	async fn unsubscribe_from_events(&mut self, filters: Vec<EventFilter>) -> PluginResult<()>;
}

/// Real-time discovery for live monitoring.
///
/// Extended trait for discovery plugins that support real-time event
/// streaming through persistent connections like WebSockets.
#[async_trait]
pub trait RealtimeDiscovery: DiscoveryPlugin {
	/// Subscribe to real-time events through streaming connections.
	///
	/// Establishes a persistent connection for receiving events as they
	/// occur with minimal latency.
	async fn subscribe_realtime(&mut self, sink: EventSink<crate::Event>) -> PluginResult<()>;

	/// Check if real-time subscription is active.
	///
	/// Returns true if the plugin has an active streaming connection.
	fn is_subscribed(&self) -> bool;

	/// Get average latency from event occurrence to discovery.
	///
	/// Returns the average delay in milliseconds between when an event
	/// occurs on-chain and when it's discovered by this plugin.
	async fn get_discovery_latency(&self) -> PluginResult<f64>; // milliseconds
}

/// Discovery source types.
///
/// Categorizes the technical mechanism used by discovery plugins
/// to find and retrieve events from various data sources.
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum DiscoverySourceType {
	/// Regular blockchain RPC polling for events
	OnchainPolling, // Regular RPC polling
	/// Real-time blockchain event streaming via WebSocket
	OnchainStreaming, // WebSocket or similar
	/// HTTP webhook endpoint for push notifications
	Webhook, // HTTP webhook endpoint
	/// Message queue systems like Kafka or RabbitMQ
	MessageQueue, // Kafka, RabbitMQ, etc.
	/// Direct database monitoring for event records
	Database, // Direct database monitoring
	/// Third-party API endpoints for event data
	API, // Third-party API
	/// Custom discovery mechanism not covered above
	Custom(String),
}

/// Factory trait for creating discovery plugins.
///
/// Provides a standardized interface for instantiating discovery plugins
/// with configuration validation and capability reporting.
pub trait DiscoveryPluginFactory: Send + Sync {
	/// Create a new instance of the discovery plugin with configuration.
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn DiscoveryPlugin>>;

	/// Get the unique type identifier for this plugin factory.
	fn plugin_type(&self) -> &'static str;

	/// Get the list of blockchain networks this plugin supports.
	fn supported_chains(&self) -> Vec<ChainId>;

	/// Get the technical mechanism this plugin uses for discovery.
	fn source_type(&self) -> DiscoverySourceType;

	/// Check if this plugin supports historical event discovery.
	fn supports_historical(&self) -> bool;

	/// Check if this plugin supports real-time event streaming.
	fn supports_realtime(&self) -> bool;
}
