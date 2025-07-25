//! # Delivery Plugin Types
//!
//! Defines types and traits for transaction delivery and execution.
//!
//! This module provides the infrastructure for plugins that handle the submission
//! and monitoring of blockchain transactions. It supports various delivery mechanisms
//! including standard RPC submission, MEV-protected relays, and private mempools.

use crate::{PluginConfig, RetryConfig};

use super::{Address, BasePlugin, ChainId, PluginResult, Timestamp, TxHash};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

/// Transaction data for delivery.
///
/// Contains all necessary fields for constructing and submitting
/// a blockchain transaction, supporting both legacy and EIP-1559 formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
	/// Recipient address of the transaction
	pub to: Address,
	/// Amount of native currency to transfer in wei
	pub value: u64,
	/// Encoded function call data
	pub data: Bytes,
	/// Maximum gas units this transaction can consume
	pub gas_limit: u64,
	/// Gas price for legacy transactions
	pub gas_price: Option<u64>,
	/// Maximum total fee per gas for EIP-1559 transactions
	pub max_fee_per_gas: Option<u64>,
	/// Maximum priority fee per gas for EIP-1559 transactions
	pub max_priority_fee_per_gas: Option<u64>,
	/// Transaction nonce (if manually specified)
	pub nonce: Option<u64>,
	/// Chain ID for replay protection
	pub chain_id: ChainId,
}

/// Unified transaction request that can represent both fills and settlements.
///
/// Encapsulates a transaction along with its priority, type classification,
/// and metadata for comprehensive transaction management.
#[derive(Debug, Clone)]
pub struct TransactionRequest {
	/// The transaction data to be delivered
	pub transaction: Transaction,
	/// Priority level for transaction submission
	pub priority: TransactionPriority,
	/// Classification of the transaction request
	pub request_type: TransactionRequestType,
	/// Additional metadata about the transaction
	pub metadata: TransactionMetadata,
	/// Optional retry configuration for failed deliveries
	pub retry_config: Option<RetryConfig>,
}

/// Type of transaction request.
///
/// Distinguishes between order fills and settlement transactions
/// to enable appropriate handling and tracking.
#[derive(Debug, Clone)]
pub enum TransactionRequestType {
	/// Transaction that fills an order
	Fill {
		/// Unique identifier of the order being filled
		order_id: String,
		/// Type of order protocol (e.g., "eip7683")
		order_type: String,
	},
	/// Transaction that settles a filled order
	Settlement {
		/// Unique identifier of the original order
		order_id: String,
		/// Unique identifier of the fill transaction
		fill_id: String,
		/// Type of settlement mechanism
		settlement_type: super::settlement::SettlementType,
		/// Expected reward amount for settlement
		expected_reward: u64,
	},
}

/// Delivery request with transaction and metadata.
///
/// Primary structure for requesting transaction delivery through
/// the delivery plugin system.
#[derive(Debug, Clone)]
pub struct DeliveryRequest {
	/// Transaction to be delivered
	pub transaction: Transaction,
	/// Priority level for delivery
	pub priority: DeliveryPriority,
	/// Additional metadata for tracking and routing
	pub metadata: DeliveryMetadata,
	/// Optional retry configuration
	pub retry_config: Option<RetryConfig>,
}

/// Unified priority type for all transactions.
///
/// Determines the urgency and fee strategy for transaction submission,
/// supporting both predefined levels and custom configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionPriority {
	/// Low priority with minimal fees
	Low,
	/// Standard priority for normal operations
	Normal,
	/// High priority for time-sensitive transactions
	High,
	/// Urgent priority for critical operations
	Urgent,
	/// Custom priority with explicit fee parameters
	Custom {
		/// Maximum fee willing to pay
		max_fee: u64,
		/// Priority fee for miners
		priority_fee: u64,
		/// Optional deadline for transaction inclusion
		deadline: Option<Timestamp>,
	},
}

/// Delivery priority for transaction submission.
///
/// Controls the urgency and resource allocation for delivering
/// transactions to the blockchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryPriority {
	/// Low priority with minimal fees
	Low,
	/// Standard priority for normal operations
	Normal,
	/// High priority for time-sensitive deliveries
	High,
	/// Urgent priority for critical deliveries
	Urgent,
	/// Custom priority with explicit parameters
	Custom {
		/// Maximum fee willing to pay
		max_fee: u64,
		/// Priority fee for miners
		priority_fee: u64,
		/// Optional deadline for inclusion
		deadline: Option<Timestamp>,
	},
}

/// Unified metadata for all transaction types.
///
/// Provides contextual information about transactions for tracking,
/// debugging, and analytics purposes.
#[derive(Debug, Clone, Default)]
pub struct TransactionMetadata {
	/// Unique identifier of the associated order
	pub order_id: String,
	/// Address of the user initiating the transaction
	pub user: Address,
	/// Source system identifier (e.g., "eip7683_onchain")
	pub source: String, // e.g., "eip7683_onchain"
	/// Tags for categorization and filtering
	pub tags: Vec<String>,
	/// Additional custom metadata fields
	pub custom_fields: HashMap<String, String>,
}

/// Metadata specific to delivery requests.
///
/// Contains tracking and contextual information for
/// transaction delivery operations.
#[derive(Debug, Clone, Default)]
pub struct DeliveryMetadata {
	/// Unique identifier of the associated order
	pub order_id: String,
	/// Address of the user initiating the delivery
	pub user: Address,
	/// Tags for categorization and filtering
	pub tags: Vec<String>,
	/// Additional custom metadata fields
	pub custom_fields: HashMap<String, String>,
}

/// Delivery response with transaction hash and status.
///
/// Contains comprehensive information about a delivered transaction,
/// including its hash, status, receipt, and cost breakdown.
#[derive(Debug, Clone)]
pub struct DeliveryResponse {
	/// Transaction hash on the blockchain
	pub tx_hash: TxHash,
	/// Chain where the transaction was submitted
	pub chain_id: ChainId,
	/// Timestamp when the transaction was submitted
	pub submitted_at: Timestamp,
	/// Current status of the delivered transaction
	pub status: DeliveryStatus,
	/// Transaction receipt (if confirmed)
	pub receipt: Option<TransactionReceipt>,
	/// Cost breakdown for the delivery
	pub cost: DeliveryCost,
}

/// Status of a delivered transaction.
///
/// Tracks the lifecycle of a transaction from submission
/// through confirmation or failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryStatus {
	/// Transaction has been submitted to the network
	Submitted,
	/// Transaction is pending in the mempool
	Pending,
	/// Transaction has been confirmed on-chain
	Confirmed,
	/// Transaction failed during execution
	Failed,
	/// Transaction was dropped from the mempool
	Dropped,
	/// Transaction was replaced by another transaction
	Replaced,
}

/// Transaction receipt from blockchain confirmation.
///
/// Contains detailed information about a confirmed transaction
/// including block data, gas usage, and emitted logs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
	/// Block number where the transaction was included
	pub block_number: u64,
	/// Hash of the block containing the transaction
	pub block_hash: String,
	/// Index of the transaction within the block
	pub transaction_index: u32,
	/// Actual gas consumed by the transaction
	pub gas_used: u64,
	/// Effective gas price paid for the transaction
	pub effective_gas_price: u128,
	/// Transaction execution status (true for success, false for failure)
	pub status: bool, // true for success, false for failure
	/// Event logs emitted by the transaction
	pub logs: Vec<TransactionLog>,
}

/// Event log emitted during transaction execution.
///
/// Represents a single event log entry from a smart contract
/// during transaction execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLog {
	/// Contract address that emitted the log
	pub address: Address,
	/// Indexed event topics
	pub topics: Vec<String>,
	/// Non-indexed event data
	pub data: Bytes,
}

/// Delivery cost breakdown.
///
/// Provides detailed cost analysis for a delivered transaction
/// including gas usage and fee components.
#[derive(Debug, Clone)]
pub struct DeliveryCost {
	/// Gas units consumed by the transaction
	pub gas_used: u64,
	/// Gas price used for the transaction
	pub gas_price: u128,
	/// Total cost in wei (gas_used * gas_price)
	pub total_cost: u128,
	/// Breakdown of fees by component
	pub fee_breakdown: HashMap<String, u128>,
}

/// Delivery estimation result.
///
/// Provides cost and time estimates for transaction delivery
/// along with optimization recommendations.
#[derive(Debug, Clone)]
pub struct DeliveryEstimate {
	/// Estimated gas limit required
	pub gas_limit: u64,
	/// Recommended gas price
	pub gas_price: u64,
	/// Estimated total cost in wei
	pub estimated_cost: u64,
	/// Estimated time until confirmation in seconds
	pub estimated_time: Option<u64>, // seconds until confirmation
	/// Confidence score for the estimate (0.0 to 1.0)
	pub confidence_score: f64, // 0.0 to 1.0
	/// Optimization recommendations
	pub recommendations: Vec<String>,
}

/// Plugin interface for transaction delivery mechanisms.
///
/// Core trait that all delivery plugins must implement to integrate with
/// the solver's transaction submission system. Supports various delivery
/// methods including direct RPC, MEV relays, and private mempools.
#[async_trait]
pub trait DeliveryPlugin: BasePlugin {
	/// Get the chain ID this plugin operates on.
	///
	/// Returns the blockchain network identifier for this delivery plugin.
	fn chain_id(&self) -> ChainId;

	/// Check if this plugin can handle the delivery request.
	///
	/// Validates whether the plugin supports the specific transaction
	/// and delivery requirements.
	async fn can_deliver(&self, request: &DeliveryRequest) -> PluginResult<bool>;

	/// Estimate delivery cost and time.
	///
	/// Provides gas estimates and timing predictions for the transaction
	/// delivery based on current network conditions.
	async fn estimate(&self, request: &DeliveryRequest) -> PluginResult<DeliveryEstimate>;

	/// Execute delivery and return immediate response.
	///
	/// Submits the transaction to the network and returns initial
	/// submission information.
	async fn deliver(&self, request: DeliveryRequest) -> PluginResult<DeliveryResponse>;

	/// Get transaction status by hash.
	///
	/// Queries the current status of a previously submitted transaction.
	async fn get_transaction_status(
		&self,
		tx_hash: &TxHash,
	) -> PluginResult<Option<DeliveryResponse>>;

	/// Cancel a pending transaction if possible.
	///
	/// Attempts to cancel a transaction that hasn't been confirmed yet.
	/// Returns true if cancellation was successful.
	async fn cancel_transaction(&self, tx_hash: &TxHash) -> PluginResult<bool>;

	/// Replace a pending transaction with higher gas price.
	///
	/// Submits a replacement transaction with updated parameters to
	/// override a pending transaction.
	async fn replace_transaction(
		&self,
		original_tx_hash: &TxHash,
		new_request: DeliveryRequest,
	) -> PluginResult<DeliveryResponse>;

	/// Get supported delivery features.
	///
	/// Returns the list of advanced features this plugin supports.
	fn supported_features(&self) -> Vec<DeliveryFeature>;

	/// Get current network conditions.
	///
	/// Provides real-time information about network status and congestion.
	async fn get_network_status(&self) -> PluginResult<NetworkStatus>;
}

/// Features supported by delivery plugins.
///
/// Enumerates advanced capabilities that delivery plugins
/// may support for enhanced transaction management.
#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryFeature {
	/// Support for EIP-1559 type 2 transactions
	EIP1559, // Type 2 transactions
	/// Ability to cancel pending transactions
	Cancellation,
	/// Ability to replace pending transactions
	Replacement,
	/// Support for batching multiple transactions
	BatchDelivery,
	/// Access to private mempool services
	PrivateMempool,
	/// Integration with Flashbots relay
	FlashbotsRelay,
	/// MEV protection mechanisms
	MEVProtection,
	/// Advanced gas estimation capabilities
	GasEstimation,
	/// Automatic nonce management
	NonceManagement,
}

/// Current network status information.
///
/// Provides real-time data about blockchain network conditions
/// for optimizing transaction delivery strategies.
#[derive(Debug, Clone)]
pub struct NetworkStatus {
	/// Chain identifier
	pub chain_id: ChainId,
	/// Current block number
	pub block_number: u64,
	/// Current gas price
	pub gas_price: u64,
	/// Base fee for EIP-1559 networks
	pub base_fee: Option<u64>,
	/// Recommended priority fee
	pub priority_fee: Option<u64>,
	/// Current network congestion level
	pub network_congestion: CongestionLevel,
	/// Number of pending transactions in mempool
	pub pending_tx_count: Option<u64>,
}

/// Network congestion level.
///
/// Indicates the current load on the blockchain network
/// to help with gas price and timing decisions.
#[derive(Debug, Clone, PartialEq)]
pub enum CongestionLevel {
	/// Network is operating normally with low load
	Low,
	/// Moderate network activity
	Medium,
	/// High network activity with delays expected
	High,
	/// Critical congestion with significant delays
	Critical,
}

/// Delivery strategy for handling multiple plugins.
///
/// Defines how the system selects between multiple available
/// delivery plugins for transaction submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryStrategy {
	/// Rotate through available plugins in order
	RoundRobin,
}

/// Factory trait for creating delivery plugins.
///
/// Provides a standardized interface for instantiating delivery
/// plugins with configuration and capability reporting.
pub trait DeliveryPluginFactory: Send + Sync {
	/// Create a new instance of the delivery plugin with configuration.
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn DeliveryPlugin>>;

	/// Get the unique type identifier for this plugin factory.
	fn plugin_type(&self) -> &'static str;

	/// Get the list of blockchain networks this plugin supports.
	fn supported_chains(&self) -> Vec<ChainId>;
}
