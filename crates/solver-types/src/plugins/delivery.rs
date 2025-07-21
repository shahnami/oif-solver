// solver-types/src/plugins/delivery.rs

use crate::PluginConfig;

use super::{Address, BasePlugin, ChainId, PluginResult, Timestamp, TxHash};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

/// Transaction data for delivery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
	pub to: Address,
	pub value: u64,
	pub data: Bytes,
	pub gas_limit: u64,
	pub gas_price: Option<u64>,
	pub max_fee_per_gas: Option<u64>,
	pub max_priority_fee_per_gas: Option<u64>,
	pub nonce: Option<u64>,
	pub chain_id: ChainId,
}

/// Delivery request with transaction and metadata
#[derive(Debug, Clone)]
pub struct DeliveryRequest {
	pub transaction: Transaction,
	pub priority: DeliveryPriority,
	pub metadata: DeliveryMetadata,
	pub retry_config: Option<RetryConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryPriority {
	Low,
	Normal,
	High,
	Urgent,
	Custom {
		max_fee: u64,
		priority_fee: u64,
		deadline: Option<Timestamp>,
	},
}

#[derive(Debug, Clone, Default)]
pub struct DeliveryMetadata {
	pub order_id: String,
	pub user: Address,
	pub tags: Vec<String>,
	pub custom_fields: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
	pub max_attempts: u32,
	pub initial_delay_ms: u64,
	pub max_delay_ms: u64,
	pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
	fn default() -> Self {
		Self {
			max_attempts: 3,
			initial_delay_ms: 1000,
			max_delay_ms: 30000,
			backoff_multiplier: 2.0,
		}
	}
}

/// Delivery response with transaction hash and status
#[derive(Debug, Clone)]
pub struct DeliveryResponse {
	pub tx_hash: TxHash,
	pub chain_id: ChainId,
	pub submitted_at: Timestamp,
	pub status: DeliveryStatus,
	pub receipt: Option<TransactionReceipt>,
	pub cost: DeliveryCost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryStatus {
	Submitted,
	Pending,
	Confirmed,
	Failed,
	Dropped,
	Replaced,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
	pub block_number: u64,
	pub block_hash: String,
	pub transaction_index: u32,
	pub gas_used: u64,
	pub effective_gas_price: u64,
	pub status: bool, // true for success, false for failure
	pub logs: Vec<TransactionLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLog {
	pub address: Address,
	pub topics: Vec<String>,
	pub data: Bytes,
}

/// Delivery cost breakdown
#[derive(Debug, Clone)]
pub struct DeliveryCost {
	pub gas_used: u64,
	pub gas_price: u64,
	pub total_cost: u64,
	pub fee_breakdown: HashMap<String, u64>,
}

/// Delivery estimation result
#[derive(Debug, Clone)]
pub struct DeliveryEstimate {
	pub gas_limit: u64,
	pub gas_price: u64,
	pub estimated_cost: u64,
	pub estimated_time: Option<u64>, // seconds until confirmation
	pub confidence_score: f64,       // 0.0 to 1.0
	pub recommendations: Vec<String>,
}

/// Plugin interface for transaction delivery mechanisms
#[async_trait]
pub trait DeliveryPlugin: BasePlugin {
	/// Get the chain ID this plugin operates on
	fn chain_id(&self) -> ChainId;

	/// Check if this plugin can handle the delivery request
	async fn can_deliver(&self, request: &DeliveryRequest) -> PluginResult<bool>;

	/// Estimate delivery cost and time
	async fn estimate(&self, request: &DeliveryRequest) -> PluginResult<DeliveryEstimate>;

	/// Execute delivery and return immediate response
	async fn deliver(&self, request: DeliveryRequest) -> PluginResult<DeliveryResponse>;

	/// Get transaction status by hash
	async fn get_transaction_status(
		&self,
		tx_hash: &TxHash,
	) -> PluginResult<Option<DeliveryResponse>>;

	/// Cancel a pending transaction if possible
	async fn cancel_transaction(&self, tx_hash: &TxHash) -> PluginResult<bool>;

	/// Replace a pending transaction with higher gas price
	async fn replace_transaction(
		&self,
		original_tx_hash: &TxHash,
		new_request: DeliveryRequest,
	) -> PluginResult<DeliveryResponse>;

	/// Get supported delivery features
	fn supported_features(&self) -> Vec<DeliveryFeature>;

	/// Get current network conditions
	async fn get_network_status(&self) -> PluginResult<NetworkStatus>;
}

/// Features supported by delivery plugins
#[derive(Debug, Clone, PartialEq)]
pub enum DeliveryFeature {
	EIP1559, // Type 2 transactions
	Cancellation,
	Replacement,
	BatchDelivery,
	PrivateMempool,
	FlashbotsRelay,
	MEVProtection,
	GasEstimation,
	NonceManagement,
}

/// Current network status information
#[derive(Debug, Clone)]
pub struct NetworkStatus {
	pub chain_id: ChainId,
	pub block_number: u64,
	pub gas_price: u64,
	pub base_fee: Option<u64>,
	pub priority_fee: Option<u64>,
	pub network_congestion: CongestionLevel,
	pub pending_tx_count: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CongestionLevel {
	Low,
	Medium,
	High,
	Critical,
}

/// Delivery strategy for handling multiple plugins
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryStrategy {
	RoundRobin,
}

/// Factory trait for creating delivery plugins
pub trait DeliveryPluginFactory: Send + Sync {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn DeliveryPlugin>>;
	fn plugin_type(&self) -> &'static str;
	fn supported_chains(&self) -> Vec<ChainId>;
}
