//! # Event System Types
//!
//! Defines the event-driven architecture types used throughout the solver system.
//!
//! This module contains all event types that flow through the system, enabling
//! loose coupling between components and providing a clear data flow from order
//! discovery through settlement completion.

use bytes::Bytes;
use std::collections::HashMap;

use crate::plugins::discovery::DiscoveryEvent;
use crate::plugins::settlement::SettlementStatus;
use crate::plugins::{Address, ChainId, Timestamp, TxHash};

/// Unified event type for all solver services.
///
/// This enum represents all possible events that can occur in the solver system,
/// providing a type-safe way to handle different stages of order processing
/// from initial discovery through final settlement.
#[derive(Debug, Clone)]
pub enum Event {
	/// Raw discovery event from discovery plugins
	Discovery(DiscoveryEvent),
	/// Processed order ready for execution
	OrderCreated(OrderEvent),
	/// Order execution result and status
	OrderFill(FillEvent),
	/// Notification that settlement is ready to proceed
	SettlementReady(SettlementReadyEvent),
	/// Settlement execution result and status
	Settlement(SettlementEvent),
	/// Service health and status updates
	ServiceStatus(StatusEvent),
}

/// Represents a discovered order ready for processing.
///
/// Contains all necessary information about an order that has been discovered
/// by a discovery plugin and is ready to be processed by the delivery system.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrderEvent {
	/// Unique identifier for the order
	pub order_id: String,
	/// Blockchain network where the order exists
	pub chain_id: ChainId,
	/// Address of the user who created the order
	pub user: Address,
	/// Timestamp when the order was discovered
	pub timestamp: Timestamp,
	/// Additional metadata provided by the discovery source
	pub metadata: HashMap<String, String>,
	/// Source identifier of the discovery plugin
	pub source: String,
	/// Contract address that emitted the order event, if applicable
	pub contract_address: Option<Address>,
	/// Raw order data for processing by order processors
	pub raw_data: Bytes,
}

/// Represents the result of order execution.
///
/// Contains information about the execution of an order, including the
/// transaction details and current status. This event is used to track
/// the progress of order fulfillment.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FillEvent {
	/// The original order ID that was filled
	pub order_id: String,
	/// Unique identifier for this fill transaction
	pub fill_id: String,
	/// Blockchain network where the fill occurred
	pub chain_id: ChainId,
	/// Transaction hash of the fill execution
	pub tx_hash: TxHash,
	/// Timestamp when the fill was executed
	pub timestamp: Timestamp,
	/// Current status of the fill transaction
	pub status: FillStatus,
	/// Source plugin that processed the order
	pub source: String,
	/// Raw order data needed for settlement processing
	pub order_data: Option<Bytes>,
}

/// Status of an order fill transaction.
///
/// Tracks the lifecycle of a fill transaction from submission through
/// final confirmation or failure.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum FillStatus {
	/// Transaction has been submitted but not yet confirmed
	Pending,
	/// Transaction has been confirmed on the blockchain
	Confirmed,
	/// Transaction failed with error details
	Failed(String),
}

/// Notification that a fill is ready for settlement processing.
///
/// This event is emitted when the settlement service determines that
/// a confirmed fill is ready to proceed with settlement, including
/// any necessary oracle attestations or claim windows.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettlementReadyEvent {
	/// The confirmed fill event ready for settlement
	pub fill_event: FillEvent,
	/// Oracle attestation ID if required for settlement
	pub oracle_attestation_id: Option<String>,
	/// Start of the claim window for settlement
	pub claim_window_start: Timestamp,
	/// End of the claim window for settlement
	pub claim_window_end: Timestamp,
}

/// Represents the result of settlement execution.
///
/// Contains information about the settlement transaction and its current
/// status, enabling tracking of cross-chain settlement processes.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SettlementEvent {
	/// The original order ID being settled
	pub order_id: String,
	/// Unique identifier for this settlement
	pub settlement_id: String,
	/// Source blockchain where the order originated
	pub source_chain: ChainId,
	/// Destination blockchain where settlement occurs
	pub destination_chain: ChainId,
	/// Transaction hash of the settlement execution
	pub tx_hash: TxHash,
	/// Timestamp when settlement was executed
	pub timestamp: Timestamp,
	/// Current status of the settlement
	pub status: SettlementStatus,
}

/// Service health and status update event.
///
/// Used to communicate the health status of various system components
/// and services throughout the solver ecosystem.
#[derive(Debug, Clone)]
pub struct StatusEvent {
	/// Name of the service reporting status
	pub service: String,
	/// Current health status of the service
	pub status: ServiceStatus,
	/// Timestamp when the status was reported
	pub timestamp: Timestamp,
	/// Optional additional details about the status
	pub details: Option<String>,
}

/// Health status levels for system services.
///
/// Represents different levels of service health from fully operational
/// to completely unavailable, enabling appropriate response to service issues.
#[derive(Debug, Clone)]
pub enum ServiceStatus {
	/// Service is fully operational
	Healthy,
	/// Service is operational but with reduced performance
	Degraded,
	/// Service is not functioning properly
	Unhealthy,
	/// Service is in the process of starting up
	Starting,
	/// Service is in the process of shutting down
	Stopping,
}

// Conversion implementations
impl From<DiscoveryEvent> for Event {
	fn from(event: DiscoveryEvent) -> Self {
		Event::Discovery(event)
	}
}
