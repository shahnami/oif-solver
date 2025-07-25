use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
	pub id: String,
	pub source: String,
	pub standard: String,
	pub metadata: IntentMetadata,
	pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentMetadata {
	pub requires_auction: bool,
	pub exclusive_until: Option<u64>,
	pub discovered_at: u64,
}
