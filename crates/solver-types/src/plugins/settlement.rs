// solver-types/src/plugins/settlement.rs

use crate::PluginConfig;

use super::{delivery::Transaction, Address, BasePlugin, ChainId, PluginResult, Timestamp, TxHash};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

/// Fill data representing order fulfillment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillData {
	pub order_id: String,
	pub fill_tx_hash: TxHash,
	pub fill_timestamp: Timestamp,
	pub filler_address: Address,
	pub fill_amount: u64,
	pub chain_id: ChainId,
	pub block_number: u64,
	pub gas_used: u64,
	pub effective_gas_price: u64,
}

/// Settlement transaction that claims rewards or processes settlement
#[derive(Debug, Clone)]
pub struct SettlementTransaction {
	pub transaction: Transaction,
	pub settlement_type: SettlementType,
	pub expected_reward: u64,
	pub metadata: SettlementMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum SettlementType {
	Direct,         // Direct settlement on origin chain
	Optimistic,     // Optimistic settlement with challenge period
	ZkProof,        // Zero-knowledge proof settlement
	Oracle,         // Oracle-based settlement
	Arbitrum,       // Arbitrum-specific settlement
	Custom(String), // Custom settlement mechanism
}

#[derive(Debug, Clone)]
pub struct SettlementMetadata {
	pub order_id: String,
	pub fill_hash: TxHash,
	pub strategy: String,
	pub expected_confirmations: u32,
	pub timeout: Option<Timestamp>,
	pub custom_fields: HashMap<String, String>,
}

/// Settlement result after execution
#[derive(Debug, Clone)]
pub struct SettlementResult {
	pub settlement_tx_hash: TxHash,
	pub status: SettlementStatus,
	pub timestamp: Timestamp,
	pub actual_reward: u64,
	pub gas_used: u64,
	pub confirmation_data: Option<ConfirmationData>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SettlementStatus {
	Pending,
	Confirmed,
	Failed,
	Challenged, // For optimistic settlements
	Expired,
	Cancelled,
}

#[derive(Debug, Clone)]
pub struct ConfirmationData {
	pub block_number: u64,
	pub block_hash: String,
	pub confirmation_count: u32,
	pub proof_data: Option<Bytes>,
}

/// Settlement strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementStrategy {
	pub strategy_type: SettlementType,
	pub priority: SettlementPriority,
	pub conditions: SettlementConditions,
	pub retry_config: SettlementRetryConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SettlementPriority {
	Immediate,            // Settle as soon as possible
	Batched,              // Wait for batch settlement
	Optimized,            // Wait for optimal gas conditions
	Scheduled(Timestamp), // Settle at specific time
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementConditions {
	pub min_confirmations: u32,
	pub max_gas_price: Option<u64>,
	pub min_reward: Option<u64>,
	pub timeout: Option<u64>, // seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementRetryConfig {
	pub max_attempts: u32,
	pub delay_between_attempts: u64, // seconds
	pub exponential_backoff: bool,
}

/// Settlement estimation
#[derive(Debug, Clone)]
pub struct SettlementEstimate {
	pub estimated_gas: u64,
	pub estimated_cost: u64,
	pub estimated_reward: u64,
	pub net_profit: i64,                           // can be negative
	pub confidence_score: f64,                     // 0.0 to 1.0
	pub estimated_time_to_settlement: Option<u64>, // seconds
	pub risks: Vec<SettlementRisk>,
}

#[derive(Debug, Clone)]
pub struct SettlementRisk {
	pub risk_type: String,
	pub severity: RiskSeverity,
	pub description: String,
	pub mitigation: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RiskSeverity {
	Low,
	Medium,
	High,
	Critical,
}

/// Plugin interface for settlement strategies
#[async_trait]
pub trait SettlementPlugin: BasePlugin {
	/// Check if this plugin can handle settlement for the given chain and order type
	async fn can_settle(&self, chain_id: ChainId, order_type: &str) -> PluginResult<bool>;

	/// Prepare settlement transaction for a fill
	async fn prepare_settlement(&self, fill: &FillData) -> PluginResult<SettlementTransaction>;

	/// Execute settlement transaction
	async fn execute_settlement(
		&self,
		settlement: SettlementTransaction,
	) -> PluginResult<SettlementResult>;

	/// Monitor settlement status
	async fn monitor_settlement(&self, tx_hash: &TxHash) -> PluginResult<SettlementResult>;

	/// Estimate settlement costs and rewards
	async fn estimate_settlement(&self, fill: &FillData) -> PluginResult<SettlementEstimate>;

	/// Validate that a fill can be settled
	async fn validate_fill(&self, fill: &FillData) -> PluginResult<FillValidation>;

	/// Get settlement requirements for this strategy
	fn get_settlement_requirements(&self) -> SettlementRequirements;

	/// Check if settlement is profitable
	async fn is_profitable(&self, fill: &FillData) -> PluginResult<bool>;

	/// Get supported settlement types
	fn supported_settlement_types(&self) -> Vec<SettlementType>;

	/// Cancel a pending settlement if possible
	async fn cancel_settlement(&self, tx_hash: &TxHash) -> PluginResult<bool>;
}

/// Fill validation result
#[derive(Debug, Clone)]
pub struct FillValidation {
	pub is_valid: bool,
	pub errors: Vec<String>,
	pub warnings: Vec<String>,
	pub required_proofs: Vec<ProofRequirement>,
}

#[derive(Debug, Clone)]
pub struct ProofRequirement {
	pub proof_type: String,
	pub description: String,
	pub deadline: Option<Timestamp>,
}

/// Settlement requirements for a strategy
#[derive(Debug, Clone)]
pub struct SettlementRequirements {
	pub min_confirmations: u32,
	pub required_contracts: Vec<ContractRequirement>,
	pub oracle_requirements: Vec<OracleRequirement>,
	pub timeout_limits: Option<TimeoutLimits>,
}

#[derive(Debug, Clone)]
pub struct ContractRequirement {
	pub contract_address: Address,
	pub chain_id: ChainId,
	pub contract_type: String,
	pub required_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OracleRequirement {
	pub oracle_address: Address,
	pub oracle_type: String,
	pub required_feed: String,
}

#[derive(Debug, Clone)]
pub struct TimeoutLimits {
	pub max_settlement_time: u64,      // seconds
	pub challenge_period: Option<u64>, // seconds for optimistic settlements
}

/// Factory trait for creating settlement plugins
pub trait SettlementPluginFactory: Send + Sync {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn SettlementPlugin>>;
	fn plugin_type(&self) -> &'static str;
	fn supported_chains(&self) -> Vec<ChainId>;
	fn supported_settlement_types(&self) -> Vec<SettlementType>;
}
