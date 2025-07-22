// solver-types/src/events.rs

use bytes::Bytes;
use std::collections::HashMap;

use crate::plugins::discovery::DiscoveryEvent;
use crate::plugins::settlement::SettlementStatus;
use crate::plugins::{Address, ChainId, Timestamp, TxHash};

/// Unified event type for all services
#[derive(Debug, Clone)]
pub enum Event {
	Discovery(DiscoveryEvent),
	OrderCreated(OrderEvent),
	OrderFill(FillEvent),
	SettlementReady(SettlementReadyEvent),
	Settlement(SettlementEvent),
	ServiceStatus(StatusEvent),
}

#[derive(Debug, Clone)]
pub struct OrderEvent {
	pub order_id: String,
	pub chain_id: ChainId,
	pub user: Address,
	pub timestamp: Timestamp,
	pub metadata: HashMap<String, String>,
	pub source: String,                    // e.g., "eip7683_onchain"
	pub contract_address: Option<Address>, // Contract that emitted the event
	pub raw_data: Bytes,                   // Raw event data for parsing
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FillEvent {
	pub order_id: String,
	pub fill_id: String,
	pub chain_id: ChainId,
	pub tx_hash: TxHash,
	pub timestamp: Timestamp,
	pub status: FillStatus,
	pub source: String, // e.g., "eip7683_onchain", inherited from OrderEvent
	pub order_data: Option<Bytes>, // Raw order data for settlement
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum FillStatus {
	Pending,
	Confirmed,
	Failed(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettlementReadyEvent {
	pub fill_event: FillEvent, // Original fill event for processing
	pub oracle_attestation_id: Option<String>,
	pub claim_window_start: Timestamp,
	pub claim_window_end: Timestamp,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettlementEvent {
	pub order_id: String,
	pub settlement_id: String,
	pub source_chain: ChainId,
	pub destination_chain: ChainId,
	pub tx_hash: TxHash,
	pub timestamp: Timestamp,
	pub status: SettlementStatus,
}

#[derive(Debug, Clone)]
pub struct StatusEvent {
	pub service: String,
	pub status: ServiceStatus,
	pub timestamp: Timestamp,
	pub details: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ServiceStatus {
	Healthy,
	Degraded,
	Unhealthy,
	Starting,
	Stopping,
}

// Conversion implementations
impl From<DiscoveryEvent> for Event {
	fn from(event: DiscoveryEvent) -> Self {
		Event::Discovery(event)
	}
}
