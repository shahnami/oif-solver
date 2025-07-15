//! Types for intent discovery.

use serde::{Deserialize, Serialize};
use solver_types::{
	chains::ChainId,
	common::{BlockNumber, Timestamp},
	orders::{OrderId, OrderStatus},
};

/// A discovered intent with metadata
#[derive(Debug)]
pub struct DiscoveredIntent {
	/// The parsed order
	pub order: solver_orders::OrderImpl,
	/// Raw encoded order data (for storage and re-parsing)
	pub raw_order_data: Vec<u8>,
	/// Where this intent was discovered
	pub source: IntentSourceType,
	/// Discovery metadata
	pub metadata: DiscoveryMetadata,
	/// Current status
	pub status: OrderStatus,
}

/// Source of an intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentSourceType {
	/// Discovered from on-chain event
	OnChain {
		chain: ChainId,
		block: BlockNumber,
		transaction_hash: [u8; 32],
		log_index: u64,
	},
	/// Discovered from off-chain source
	OffChain {
		source_name: String,
		endpoint: Option<String>,
	},
}

/// Metadata about when/how an intent was discovered
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryMetadata {
	/// When the intent was first discovered
	pub discovered_at: Timestamp,
	/// When the intent was last updated
	pub last_updated: Timestamp,
	/// Number of times we've seen this intent
	pub seen_count: u32,
	/// Any additional source-specific data
	pub extra: serde_json::Value,
}

/// Raw intent data before parsing
#[derive(Debug, Clone)]
pub struct RawIntent {
	/// Source of this raw intent
	pub source: IntentSourceType,
	/// Raw data (could be event data, API response, etc.)
	pub data: Vec<u8>,
	/// Hint about what type of order this might be
	pub order_type_hint: Option<String>,
	/// Additional context for parsing (e.g., event topics for on-chain sources)
	pub context: Option<serde_json::Value>,
}

/// Intent lifecycle event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleEvent {
	pub order_id: OrderId,
	pub timestamp: Timestamp,
	pub previous_status: OrderStatus,
	pub new_status: OrderStatus,
	pub reason: Option<String>,
}
