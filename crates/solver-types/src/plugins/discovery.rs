// solver-types/src/plugins/discovery.rs

use crate::PluginConfig;

use super::{Address, BasePlugin, ChainId, PluginResult, Timestamp, TxHash};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use tokio::sync::mpsc;

/// Discovery event representing a newly found order or relevant blockchain event
#[derive(Debug, Clone)]
pub struct DiscoveryEvent {
	pub id: String,
	pub event_type: EventType,
	pub source: String,
	pub chain_id: ChainId,
	pub block_number: Option<u64>,
	pub transaction_hash: Option<TxHash>,
	pub timestamp: Timestamp,
	pub raw_data: Bytes,
	pub parsed_data: Option<ParsedEventData>,
	pub metadata: EventMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventType {
	OrderCreated,
	OrderCancelled,
	OrderFilled,
	OrderExpired,
	OrderUpdated,
	BlockCreated,
	Custom(String),
}

#[derive(Debug, Clone)]
pub struct ParsedEventData {
	pub order_id: Option<String>,
	pub user: Option<Address>,
	pub contract_address: Option<Address>,
	pub method_signature: Option<String>,
	pub decoded_params: HashMap<String, EventParam>,
}

#[derive(Debug, Clone)]
pub enum EventParam {
	Address(Address),
	Uint256(String), // Store as string to avoid overflow
	Bytes(Bytes),
	String(String),
	Bool(bool),
	Array(Vec<EventParam>),
}

#[derive(Debug, Clone)]
pub struct EventMetadata {
	pub source_specific: HashMap<String, String>,
	pub confidence_score: f64,         // 0.0 to 1.0
	pub processing_delay: Option<u64>, // milliseconds from event to discovery
	pub retry_count: u32,
}

/// Event sink for plugins to send discovered events
#[derive(Debug, Clone)]
pub struct EventSink<T = crate::Event> {
	sender: mpsc::UnboundedSender<T>,
}

impl<T: Send + 'static> EventSink<T> {
	pub fn new(sender: mpsc::UnboundedSender<T>) -> Self {
		Self { sender }
	}

	pub fn send(&self, event: T) -> PluginResult<()> {
		self.sender
			.send(event)
			.map_err(|_| super::PluginError::ExecutionFailed("Event sink closed".to_string()))
	}
}

impl EventSink<crate::Event> {
	/// Convenience method for sending discovery events
	pub fn send_discovery(&self, event: DiscoveryEvent) -> PluginResult<()> {
		self.send(crate::Event::Discovery(event))
	}
}

/// Event filter for targeted discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventFilter {
	pub contract_addresses: Option<Vec<Address>>,
	pub event_signatures: Option<Vec<String>>,
	pub topics: Option<Vec<Option<String>>>, // None for wildcard topics
	pub from_block: Option<u64>,
	pub to_block: Option<u64>,
}

/// Discovery status and statistics
#[derive(Debug, Clone)]
pub struct DiscoveryStatus {
	pub is_running: bool,
	pub current_block: Option<u64>,
	pub target_block: Option<u64>,
	pub events_discovered: u64,
	pub last_event_timestamp: Option<Timestamp>,
	pub errors_count: u64,
	pub average_processing_time_ms: f64,
}

/// Plugin interface for event discovery
#[async_trait]
pub trait DiscoveryPlugin: BasePlugin {
	/// Start monitoring for events and send them to the provided sink
	async fn start_monitoring(&mut self, sink: EventSink<crate::Event>) -> PluginResult<()>;

	/// Stop monitoring
	async fn stop_monitoring(&mut self) -> PluginResult<()>;

	/// Get current discovery status
	async fn get_status(&self) -> PluginResult<DiscoveryStatus>;

	/// Manually trigger discovery for a specific block range
	async fn discover_range(
		&self,
		from_block: u64,
		to_block: u64,
		sink: EventSink<crate::Event>,
	) -> PluginResult<u64>;

	/// Get supported event types this plugin can discover
	fn supported_event_types(&self) -> Vec<EventType>;

	/// Get the chain this plugin monitors
	fn chain_id(&self) -> ChainId;

	/// Check if plugin can handle the given contract address
	async fn can_monitor_contract(&self, contract_address: &Address) -> PluginResult<bool>;

	/// Subscribe to specific events (if supported)
	async fn subscribe_to_events(&mut self, filters: Vec<EventFilter>) -> PluginResult<()>;

	/// Unsubscribe from events
	async fn unsubscribe_from_events(&mut self, filters: Vec<EventFilter>) -> PluginResult<()>;
}

/// Real-time discovery for live monitoring
#[async_trait]
pub trait RealtimeDiscovery: DiscoveryPlugin {
	/// Subscribe to real-time events (WebSocket, etc.)
	async fn subscribe_realtime(&mut self, sink: EventSink<crate::Event>) -> PluginResult<()>;

	/// Check if real-time subscription is active
	fn is_subscribed(&self) -> bool;

	/// Get average latency from event to discovery
	async fn get_discovery_latency(&self) -> PluginResult<f64>; // milliseconds
}

/// Discovery source types
#[derive(Debug, Clone, PartialEq, Hash, Eq)]
pub enum DiscoverySourceType {
	OnchainPolling,   // Regular RPC polling
	OnchainStreaming, // WebSocket or similar
	Webhook,          // HTTP webhook endpoint
	MessageQueue,     // Kafka, RabbitMQ, etc.
	Database,         // Direct database monitoring
	API,              // Third-party API
	Custom(String),
}

/// Factory trait for creating discovery plugins
pub trait DiscoveryPluginFactory: Send + Sync {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn DiscoveryPlugin>>;
	fn plugin_type(&self) -> &'static str;
	fn supported_chains(&self) -> Vec<ChainId>;
	fn source_type(&self) -> DiscoverySourceType;
	fn supports_historical(&self) -> bool;
	fn supports_realtime(&self) -> bool;
}
