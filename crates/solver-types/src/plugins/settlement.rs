// solver-types/src/plugins/settlement.rs

use crate::{PluginConfig, RetryConfig};

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
	pub chain_id: ChainId,
	pub order_data: Option<Bytes>, // Raw order data that may contain filler address
}

/// Settlement transaction that claims rewards or processes settlement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementTransaction {
	pub transaction: Transaction,
	pub settlement_type: SettlementType,
	pub expected_reward: u64,
	pub metadata: SettlementMetadata,
}

/// Settlement parameters returned by OrderProcessor
/// This is a simplified version used to bridge order processing and settlement
#[derive(Debug, Clone)]
pub struct SettlementRequest {
	pub transaction: SettlementTransaction,
	pub priority: SettlementPriority,
	pub preferred_strategy: Option<String>,
	pub retry_config: Option<RetryConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementMetadata {
	pub order_id: String,
	pub strategy: String,
	pub expected_confirmations: u32,
	pub custom_fields: HashMap<String, String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SettlementPriority {
	Immediate,            // Settle as soon as possible
	Batched,              // Wait for batch settlement
	Optimized,            // Wait for optimal gas conditions
	Scheduled(Timestamp), // Settle at specific time
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

/// Plugin interface for settlement orchestration
#[async_trait]
pub trait SettlementPlugin: BasePlugin {
	/// Check if this plugin can handle settlement for the given chain and order type
	async fn can_handle(&self, chain_id: ChainId, order_type: &str) -> PluginResult<bool>;

	/// Check oracle attestation status for a fill
	async fn check_oracle_attestation(&self, fill: &FillData) -> PluginResult<AttestationStatus>;

	/// Get claim window for this order type
	async fn get_claim_window(
		&self,
		order_type: &str,
		fill: &FillData,
	) -> PluginResult<ClaimWindow>;

	/// Verify all settlement conditions are met
	async fn verify_settlement_conditions(
		&self,
		fill: &FillData,
	) -> PluginResult<SettlementReadiness>;

	/// Handle dispute or challenge if applicable
	async fn handle_dispute(
		&self,
		fill: &FillData,
		dispute_data: &DisputeData,
	) -> PluginResult<DisputeResolution>;

	/// Get settlement requirements for this strategy
	fn get_settlement_requirements(&self) -> SettlementRequirements;

	/// Get supported settlement types
	fn supported_settlement_types(&self) -> Vec<SettlementType>;
}

/// Oracle attestation status
#[derive(Debug, Clone)]
pub struct AttestationStatus {
	pub is_attested: bool,
	pub attestation_id: Option<String>,
	pub oracle_address: Option<Address>,
	pub attestation_time: Option<Timestamp>,
	pub dispute_period_end: Option<Timestamp>,
	pub is_disputed: bool,
}

/// Claim window information
#[derive(Debug, Clone)]
pub struct ClaimWindow {
	pub start: Timestamp,
	pub end: Timestamp,
	pub is_active: bool,
	pub remaining_time: Option<u64>, // seconds
}

/// Settlement readiness status
#[derive(Debug, Clone)]
pub struct SettlementReadiness {
	pub is_ready: bool,
	pub reasons: Vec<String>,
	pub oracle_status: AttestationStatus,
	pub claim_window: ClaimWindow,
	pub estimated_profit: i64,
	pub risks: Vec<SettlementRisk>,
}

/// Dispute data
#[derive(Debug, Clone)]
pub struct DisputeData {
	pub disputer: Address,
	pub dispute_reason: String,
	pub dispute_time: Timestamp,
	pub evidence: Option<Bytes>,
}

/// Dispute resolution
#[derive(Debug, Clone)]
pub struct DisputeResolution {
	pub resolution_type: DisputeResolutionType,
	pub outcome: String,
	pub refund_amount: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum DisputeResolutionType {
	Accepted,
	Rejected,
	Pending,
	Escalated,
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
