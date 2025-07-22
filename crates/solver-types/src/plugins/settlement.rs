//! # Settlement Plugin Types
//!
//! Defines types and traits for order settlement and claim processing.
//!
//! This module provides the infrastructure for plugins that handle post-fill
//! settlement operations including reward claiming, dispute resolution, and
//! cross-chain settlement verification through various mechanisms.

use crate::{PluginConfig, RetryConfig};

use super::{delivery::Transaction, Address, BasePlugin, ChainId, PluginResult, Timestamp, TxHash};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;

/// Fill data representing order fulfillment.
///
/// Contains information about a completed order fill that needs
/// to be settled or have its rewards claimed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillData {
	/// Unique identifier of the filled order
	pub order_id: String,
	/// Transaction hash of the fill execution
	pub fill_tx_hash: TxHash,
	/// Timestamp when the fill was executed
	pub fill_timestamp: Timestamp,
	/// Chain where the fill occurred
	pub chain_id: ChainId,
	/// Raw order data that may contain filler address and other details
	pub order_data: Option<Bytes>, // Raw order data that may contain filler address
}

/// Settlement transaction that claims rewards or processes settlement.
///
/// Encapsulates the transaction data needed to claim rewards or
/// finalize settlement for a filled order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementTransaction {
	/// Transaction to execute for settlement
	pub transaction: Transaction,
	/// Type of settlement mechanism
	pub settlement_type: SettlementType,
	/// Expected reward amount to be claimed
	pub expected_reward: u64,
	/// Additional metadata for settlement tracking
	pub metadata: SettlementMetadata,
}

/// Settlement parameters returned by OrderProcessor.
///
/// Simplified structure used to bridge order processing and settlement,
/// containing all necessary information to initiate settlement.
#[derive(Debug, Clone)]
pub struct SettlementRequest {
	/// Settlement transaction details
	pub transaction: SettlementTransaction,
	/// Priority for settlement execution
	pub priority: SettlementPriority,
	/// Preferred settlement strategy to use
	pub preferred_strategy: Option<String>,
	/// Retry configuration for failed attempts
	pub retry_config: Option<RetryConfig>,
}

/// Type of settlement mechanism.
///
/// Categorizes different approaches to settling cross-chain orders
/// and claiming rewards.
#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum SettlementType {
	/// Direct settlement on origin chain
	Direct, // Direct settlement on origin chain
	/// Optimistic settlement with challenge period
	Optimistic, // Optimistic settlement with challenge period
	/// Zero-knowledge proof based settlement
	ZkProof, // Zero-knowledge proof settlement
	/// Oracle-based attestation settlement
	Oracle, // Oracle-based settlement
	/// Arbitrum-specific settlement mechanism
	Arbitrum, // Arbitrum-specific settlement
	/// Custom settlement mechanism
	Custom(String), // Custom settlement mechanism
}

/// Metadata for settlement transactions.
///
/// Contains tracking and configuration information specific
/// to the settlement process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementMetadata {
	/// Order identifier being settled
	pub order_id: String,
	/// Settlement strategy being used
	pub strategy: String,
	/// Number of confirmations required
	pub expected_confirmations: u32,
	/// Additional custom metadata fields
	pub custom_fields: HashMap<String, String>,
}

/// Status of a settlement transaction.
///
/// Tracks the lifecycle of settlement from initiation through
/// confirmation or failure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SettlementStatus {
	/// Settlement is pending execution
	Pending,
	/// Settlement has been confirmed on-chain
	Confirmed,
	/// Settlement failed to execute
	Failed,
	/// Settlement is being challenged (for optimistic settlements)
	Challenged, // For optimistic settlements
	/// Settlement window has expired
	Expired,
	/// Settlement was cancelled
	Cancelled,
}

/// Priority for settlement execution.
///
/// Determines when and how settlement should be executed based
/// on urgency and optimization preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SettlementPriority {
	/// Settle as soon as possible
	Immediate, // Settle as soon as possible
	/// Wait for batch settlement to reduce costs
	Batched, // Wait for batch settlement
	/// Wait for optimal gas conditions
	Optimized, // Wait for optimal gas conditions
	/// Settle at a specific scheduled time
	Scheduled(Timestamp), // Settle at specific time
}

/// Settlement risk assessment.
///
/// Identifies potential risks in the settlement process with
/// severity ratings and mitigation strategies.
#[derive(Debug, Clone)]
pub struct SettlementRisk {
	/// Type of risk identified
	pub risk_type: String,
	/// Severity level of the risk
	pub severity: RiskSeverity,
	/// Detailed description of the risk
	pub description: String,
	/// Suggested mitigation strategy
	pub mitigation: Option<String>,
}

/// Risk severity levels.
///
/// Categorizes settlement risks by their potential impact
/// on successful settlement completion.
#[derive(Debug, Clone, PartialEq)]
pub enum RiskSeverity {
	/// Minor risk with minimal impact
	Low,
	/// Moderate risk requiring attention
	Medium,
	/// Significant risk that may prevent settlement
	High,
	/// Critical risk requiring immediate action
	Critical,
}

/// Plugin interface for settlement orchestration.
///
/// Core trait that all settlement plugins must implement to integrate with
/// the solver's settlement system. Handles reward claiming, dispute resolution,
/// and cross-chain settlement verification.
#[async_trait]
pub trait SettlementPlugin: BasePlugin {
	/// Check if this plugin can handle settlement for the given chain and order type.
	///
	/// Validates whether the plugin supports the specific chain and order
	/// protocol combination.
	async fn can_handle(&self, chain_id: ChainId, order_type: &str) -> PluginResult<bool>;

	/// Check oracle attestation status for a fill.
	///
	/// Queries oracle services to verify that a fill has been properly
	/// attested and is ready for settlement.
	async fn check_oracle_attestation(&self, fill: &FillData) -> PluginResult<AttestationStatus>;

	/// Get claim window for this order type.
	///
	/// Returns the time window during which settlement claims can be
	/// submitted for the given fill.
	async fn get_claim_window(
		&self,
		order_type: &str,
		fill: &FillData,
	) -> PluginResult<ClaimWindow>;

	/// Verify all settlement conditions are met.
	///
	/// Performs comprehensive checks to ensure the fill is ready for
	/// settlement including oracle attestations and timing constraints.
	async fn verify_settlement_conditions(
		&self,
		fill: &FillData,
	) -> PluginResult<SettlementReadiness>;

	/// Handle dispute or challenge if applicable.
	///
	/// Processes dispute claims against a settlement and determines
	/// the appropriate resolution.
	async fn handle_dispute(
		&self,
		fill: &FillData,
		dispute_data: &DisputeData,
	) -> PluginResult<DisputeResolution>;

	/// Get settlement requirements for this strategy.
	///
	/// Returns the technical requirements and dependencies for
	/// this settlement mechanism.
	fn get_settlement_requirements(&self) -> SettlementRequirements;

	/// Get supported settlement types.
	///
	/// Returns the list of settlement mechanisms this plugin can handle.
	fn supported_settlement_types(&self) -> Vec<SettlementType>;
}

/// Oracle attestation status.
///
/// Contains information about oracle verification of a fill,
/// including attestation details and dispute status.
#[derive(Debug, Clone)]
pub struct AttestationStatus {
	/// Whether the fill has been attested by the oracle
	pub is_attested: bool,
	/// Unique identifier of the attestation
	pub attestation_id: Option<String>,
	/// Address of the attesting oracle
	pub oracle_address: Option<Address>,
	/// Timestamp when attestation was recorded
	pub attestation_time: Option<Timestamp>,
	/// End time for the dispute period
	pub dispute_period_end: Option<Timestamp>,
	/// Whether the attestation is currently disputed
	pub is_disputed: bool,
}

/// Claim window information.
///
/// Defines the time period during which settlement claims
/// can be submitted for a filled order.
#[derive(Debug, Clone)]
pub struct ClaimWindow {
	/// Start time of the claim window
	pub start: Timestamp,
	/// End time of the claim window
	pub end: Timestamp,
	/// Whether the window is currently active
	pub is_active: bool,
	/// Remaining time in the window in seconds
	pub remaining_time: Option<u64>, // seconds
}

/// Settlement readiness status.
///
/// Comprehensive assessment of whether a fill is ready for settlement,
/// including all relevant conditions and risk factors.
#[derive(Debug, Clone)]
pub struct SettlementReadiness {
	/// Whether all conditions for settlement are met
	pub is_ready: bool,
	/// Reasons why settlement may not be ready
	pub reasons: Vec<String>,
	/// Current oracle attestation status
	pub oracle_status: AttestationStatus,
	/// Claim window timing information
	pub claim_window: ClaimWindow,
	/// Estimated profit from settlement (can be negative)
	pub estimated_profit: i64,
	/// Identified risks with settlement
	pub risks: Vec<SettlementRisk>,
}

/// Dispute data.
///
/// Contains information about a dispute raised against
/// a settlement claim.
#[derive(Debug, Clone)]
pub struct DisputeData {
	/// Address of the entity raising the dispute
	pub disputer: Address,
	/// Reason for the dispute
	pub dispute_reason: String,
	/// Timestamp when dispute was raised
	pub dispute_time: Timestamp,
	/// Supporting evidence for the dispute
	pub evidence: Option<Bytes>,
}

/// Dispute resolution.
///
/// Contains the outcome of a dispute resolution process
/// including any refunds or penalties.
#[derive(Debug, Clone)]
pub struct DisputeResolution {
	/// Type of resolution reached
	pub resolution_type: DisputeResolutionType,
	/// Detailed outcome description
	pub outcome: String,
	/// Amount to be refunded if applicable
	pub refund_amount: Option<u64>,
}

/// Type of dispute resolution.
///
/// Categorizes the outcome of dispute resolution processes.
#[derive(Debug, Clone)]
pub enum DisputeResolutionType {
	/// Dispute was accepted and settlement reversed
	Accepted,
	/// Dispute was rejected and settlement stands
	Rejected,
	/// Resolution is still pending
	Pending,
	/// Dispute escalated to higher authority
	Escalated,
}

/// Settlement requirements for a strategy.
///
/// Defines the technical and operational requirements that must
/// be met for a settlement strategy to function properly.
#[derive(Debug, Clone)]
pub struct SettlementRequirements {
	/// Minimum block confirmations required
	pub min_confirmations: u32,
	/// Smart contracts required for settlement
	pub required_contracts: Vec<ContractRequirement>,
	/// Oracle services required for verification
	pub oracle_requirements: Vec<OracleRequirement>,
	/// Timeout constraints for settlement operations
	pub timeout_limits: Option<TimeoutLimits>,
}

/// Smart contract requirement for settlement.
///
/// Specifies a contract that must be available and compatible
/// for the settlement process to work.
#[derive(Debug, Clone)]
pub struct ContractRequirement {
	/// Address of the required contract
	pub contract_address: Address,
	/// Chain where the contract is deployed
	pub chain_id: ChainId,
	/// Type of contract (e.g., "SettlementContract")
	pub contract_type: String,
	/// Required contract version if applicable
	pub required_version: Option<String>,
}

/// Oracle service requirement.
///
/// Specifies an oracle service needed for settlement verification
/// or price feeds.
#[derive(Debug, Clone)]
pub struct OracleRequirement {
	/// Address of the oracle service
	pub oracle_address: Address,
	/// Type of oracle (e.g., "PriceFeed", "Attestation")
	pub oracle_type: String,
	/// Specific data feed required from the oracle
	pub required_feed: String,
}

/// Timeout limits for settlement operations.
///
/// Defines time constraints for various settlement phases to
/// prevent indefinite waiting periods.
#[derive(Debug, Clone)]
pub struct TimeoutLimits {
	/// Maximum time allowed for settlement completion in seconds
	pub max_settlement_time: u64, // seconds
	/// Challenge period duration for optimistic settlements in seconds
	pub challenge_period: Option<u64>, // seconds for optimistic settlements
}

/// Factory trait for creating settlement plugins.
///
/// Provides a standardized interface for instantiating settlement
/// plugins with configuration and capability reporting.
pub trait SettlementPluginFactory: Send + Sync {
	/// Create a new instance of the settlement plugin with configuration.
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn SettlementPlugin>>;

	/// Get the unique type identifier for this plugin factory.
	fn plugin_type(&self) -> &'static str;

	/// Get the list of blockchain networks this plugin supports.
	fn supported_chains(&self) -> Vec<ChainId>;

	/// Get the settlement types this plugin can handle.
	fn supported_settlement_types(&self) -> Vec<SettlementType>;
}
