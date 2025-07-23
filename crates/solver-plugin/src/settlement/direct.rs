//! # Direct Settlement Plugin
//!
//! Implements direct settlement mechanism for cross-chain orders.
//!
//! This plugin provides immediate settlement on the origin chain after
//! oracle attestation confirms successful order fills. It manages dispute
//! periods, claim windows, and verification of settlement conditions.

use alloy::network::Ethereum;
use alloy::providers::{Provider, ProviderBuilder};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solver_types::*;
use std::any::Any;
use std::fmt;
use tracing::debug;

/// Direct settlement plugin - settles immediately on origin chain.
///
/// Handles direct settlement workflow where orders are settled on the
/// origin chain immediately after oracle attestation confirms the fill
/// on the destination chain.
pub struct DirectSettlementPlugin {
	/// Plugin configuration settings
	config: DirectSettlementConfig,
	/// Whether the plugin has been initialized
	is_initialized: bool,
	/// Read-only provider for oracle interactions
	provider: Option<Box<dyn Provider<Ethereum>>>,
}

impl fmt::Debug for DirectSettlementPlugin {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("DirectSettlementPlugin")
			.field("config", &self.config)
			.field("is_initialized", &self.is_initialized)
			.field("provider", &"<Provider>")
			.finish()
	}
}

/// Configuration for the direct settlement plugin.
///
/// Contains settings for oracle verification, confirmation requirements,
/// dispute periods, and claim windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectSettlementConfig {
	/// Address of the oracle contract for fill verification
	pub oracle_address: Address,
	/// RPC URL for provider connection
	pub rpc_url: String,
	/// Minimum block confirmations required before settlement
	pub min_confirmations: u32,
	/// Duration of the dispute period in seconds
	pub dispute_period_seconds: u64,
	/// Duration of the claim window in seconds
	pub claim_window_seconds: u64,
}

impl Default for DirectSettlementConfig {
	fn default() -> Self {
		Self {
			oracle_address: "0x0000000000000000000000000000000000000000".to_string(),
			rpc_url: "http://localhost:8545".to_string(),
			min_confirmations: 1,
			dispute_period_seconds: 300, // 5 minutes
			claim_window_seconds: 86400, // 24 hours
		}
	}
}

#[allow(clippy::derivable_impls)]
impl Default for DirectSettlementPlugin {
	fn default() -> Self {
		Self {
			config: DirectSettlementConfig::default(),
			is_initialized: false,
			provider: None,
		}
	}
}

impl DirectSettlementPlugin {
	/// Creates a new direct settlement plugin with default configuration.
	pub fn new() -> Self {
		Self::default()
	}

	/// Creates a new direct settlement plugin with the specified configuration.
	pub fn with_config(config: DirectSettlementConfig) -> Self {
		Self {
			config,
			is_initialized: false,
			provider: None,
		}
	}
}

#[async_trait]
impl BasePlugin for DirectSettlementPlugin {
	/// Returns the plugin type identifier.
	fn plugin_type(&self) -> &'static str {
		"direct_settlement"
	}

	/// Returns the human-readable plugin name.
	fn name(&self) -> String {
		"Direct Settlement Plugin".to_string()
	}

	/// Returns the plugin version.
	fn version(&self) -> &'static str {
		"1.0.0"
	}

	/// Returns a brief description of the plugin.
	fn description(&self) -> &'static str {
		"Direct settlement plugin that settles orders immediately on the origin chain"
	}

	/// Initializes the plugin with configuration parameters.
	async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()> {
		debug!("Initializing Direct settlement plugin");
		// Parse configuration
		if let Some(ConfigValue::String(oracle)) = config.config.get("oracle_address") {
			self.config.oracle_address = oracle.clone();
		}

		if let Some(ConfigValue::String(rpc_url)) = config.config.get("rpc_url") {
			self.config.rpc_url = rpc_url.clone();
		}

		if let Some(ConfigValue::Number(confirmations)) = config.config.get("min_confirmations") {
			self.config.min_confirmations = *confirmations as u32;
		}

		if let Some(ConfigValue::Number(dispute_period)) =
			config.config.get("dispute_period_seconds")
		{
			self.config.dispute_period_seconds = *dispute_period as u64;
		}

		if let Some(ConfigValue::Number(claim_window)) = config.config.get("claim_window_seconds") {
			self.config.claim_window_seconds = *claim_window as u64;
		}

		self.is_initialized = true;
		Ok(())
	}

	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()> {
		// Use schema validation
		let schema = self.config_schema();
		schema.validate(config)?;

		// Additional custom validation
		if let Some(min_confirmations) = config.get_number("min_confirmations") {
			if min_confirmations < 0 {
				return Err(PluginError::InvalidConfiguration(
					"min_confirmations cannot be negative".to_string(),
				));
			}
		}

		if let Some(dispute_period) = config.get_number("dispute_period_seconds") {
			if dispute_period < 0 {
				return Err(PluginError::InvalidConfiguration(
					"dispute_period_seconds cannot be negative".to_string(),
				));
			}
		}

		if let Some(claim_window) = config.get_number("claim_window_seconds") {
			if claim_window < 0 {
				return Err(PluginError::InvalidConfiguration(
					"claim_window_seconds cannot be negative".to_string(),
				));
			}
		}

		Ok(())
	}

	/// Performs health check on the plugin.
	async fn health_check(&self) -> PluginResult<PluginHealth> {
		if !self.is_initialized {
			return Ok(PluginHealth::unhealthy("Plugin not initialized"));
		}

		Ok(PluginHealth::healthy(
			"Direct settlement plugin is operational",
		))
	}

	/// Returns current plugin metrics.
	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		Ok(PluginMetrics::new())
	}

	/// Shuts down the plugin gracefully.
	async fn shutdown(&mut self) -> PluginResult<()> {
		debug!("Shutting down Direct settlement plugin");
		self.is_initialized = false;
		self.provider = None;
		Ok(())
	}

	/// Returns the configuration schema for the plugin.
	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new()
			.required(
				"oracle_address",
				ConfigFieldType::String,
				"Oracle contract address for settlement verification",
			)
			.required(
				"rpc_url",
				ConfigFieldType::String,
				"RPC URL for blockchain provider connection",
			)
			.optional(
				"min_confirmations",
				ConfigFieldType::Number,
				"Minimum confirmations required",
				Some(ConfigValue::Number(1)),
			)
			.optional(
				"dispute_period_seconds",
				ConfigFieldType::Number,
				"Dispute period in seconds",
				Some(ConfigValue::Number(300)),
			)
			.optional(
				"claim_window_seconds",
				ConfigFieldType::Number,
				"Claim window duration in seconds",
				Some(ConfigValue::Number(86400)),
			)
	}

	/// Returns self as Any reference for downcasting.
	fn as_any(&self) -> &dyn Any {
		self
	}

	/// Returns self as mutable Any reference for downcasting.
	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

#[async_trait]
impl SettlementPlugin for DirectSettlementPlugin {
	/// Sets up the provider connection.
	async fn setup_provider(&mut self) -> PluginResult<()> {
		debug!("Setting up provider with RPC URL: {}", self.config.rpc_url);

		// Create provider with the RPC URL
		let provider = ProviderBuilder::new()
			.connect(&self.config.rpc_url)
			.await
			.map_err(|e| {
				PluginError::InitializationFailed(format!("Failed to connect to provider: {}", e))
			})?;

		self.provider = Some(Box::new(provider));
		Ok(())
	}

	/// Checks if this plugin can handle settlement for the given chain and order type.
	///
	/// Direct settlement can handle any chain as it relies on oracle attestations
	/// rather than chain-specific mechanisms.
	async fn can_handle(&self, _chain_id: ChainId, _order_type: &str) -> PluginResult<bool> {
		// Can handle any chain since we only check oracle attestations
		Ok(true)
	}

	/// Checks oracle attestation status for a fill.
	///
	/// Queries the oracle to determine if the fill has been attested and
	/// whether it's past the dispute period.
	async fn check_oracle_attestation(&self, fill: &FillData) -> PluginResult<AttestationStatus> {
		// For direct settlement, check if oracle has attested the fill
		// In a real implementation, this would query the oracle contract

		// Simulate oracle attestation after some time
		let current_time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let time_since_fill = current_time.saturating_sub(fill.fill_timestamp);
		let is_attested = time_since_fill > 5; // Assume attestation after 5 seconds

		tracing::debug!(
			"Oracle attestation check: current_time={}, fill_timestamp={}, time_since_fill={}, is_attested={}",
			current_time, fill.fill_timestamp, time_since_fill, is_attested
		);

		Ok(AttestationStatus {
			is_attested,
			attestation_id: if is_attested {
				Some(format!("attestation_{}", fill.fill_tx_hash))
			} else {
				None
			},
			oracle_address: Some(self.config.oracle_address.clone()),
			attestation_time: if is_attested {
				Some(fill.fill_timestamp + 30)
			} else {
				None
			},
			dispute_period_end: if is_attested {
				Some(fill.fill_timestamp + self.config.dispute_period_seconds)
			} else {
				None
			},
			is_disputed: false,
		})
	}

	/// Gets the claim window for settlement.
	///
	/// Calculates when claims can be submitted based on attestation time
	/// and dispute period configuration.
	async fn get_claim_window(
		&self,
		_order_type: &str,
		fill: &FillData,
	) -> PluginResult<ClaimWindow> {
		// For direct settlement, claim window starts after attestation + dispute period
		let current_time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let window_start = fill.fill_timestamp + self.config.dispute_period_seconds;
		let window_end = window_start + self.config.claim_window_seconds;

		let is_active = current_time >= window_start && current_time <= window_end;
		let remaining_time = if is_active && current_time < window_end {
			Some(window_end - current_time)
		} else {
			None
		};

		Ok(ClaimWindow {
			start: window_start,
			end: window_end,
			is_active,
			remaining_time,
		})
	}

	/// Verifies all conditions required for settlement.
	///
	/// Checks oracle attestation, dispute status, and claim window timing
	/// to determine if the fill is ready for settlement.
	async fn verify_settlement_conditions(
		&self,
		fill: &FillData,
	) -> PluginResult<SettlementReadiness> {
		let attestation_status = self.check_oracle_attestation(fill).await?;
		let claim_window = self.get_claim_window("direct", fill).await?;

		let mut reasons = Vec::new();
		let mut is_ready = true;

		// Check attestation
		if !attestation_status.is_attested {
			reasons.push("Waiting for oracle attestation".to_string());
			is_ready = false;
		}

		// Check dispute period
		if attestation_status.is_disputed {
			reasons.push("Settlement is disputed".to_string());
			is_ready = false;
		}

		// Check claim window
		if !claim_window.is_active {
			if claim_window.start
				> std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap()
					.as_secs()
			{
				reasons.push("Claim window not yet open".to_string());
			} else {
				reasons.push("Claim window has expired".to_string());
			}
			is_ready = false;
		}

		Ok(SettlementReadiness {
			is_ready,
			reasons,
			oracle_status: attestation_status,
			claim_window,
			estimated_profit: 0, // Not calculated by this plugin
			risks: vec![],
		})
	}

	/// Handles dispute claims against a settlement.
	///
	/// For direct settlement, disputes are forwarded to the oracle
	/// for resolution based on provided evidence.
	async fn handle_dispute(
		&self,
		_fill: &FillData,
		dispute_data: &DisputeData,
	) -> PluginResult<DisputeResolution> {
		// For direct settlement, disputes are handled by the oracle
		// This would typically involve checking the dispute evidence and making a determination

		Ok(DisputeResolution {
			resolution_type: DisputeResolutionType::Pending,
			outcome: format!(
				"Dispute {} is being reviewed by oracle",
				dispute_data.dispute_reason
			),
			refund_amount: None,
		})
	}

	/// Returns the requirements for this settlement mechanism.
	///
	/// Specifies oracle dependencies and confirmation requirements
	/// needed for direct settlement to function properly.
	fn get_settlement_requirements(&self) -> SettlementRequirements {
		// Since settlement execution is handled by delivery service,
		// we only specify oracle requirements for orchestration
		SettlementRequirements {
			min_confirmations: self.config.min_confirmations,
			required_contracts: vec![], // Contract execution handled by delivery service
			oracle_requirements: vec![solver_types::plugins::settlement::OracleRequirement {
				oracle_address: self.config.oracle_address.clone(),
				oracle_type: "AlwaysYesOracle".to_string(),
				required_feed: "fill_verification".to_string(),
			}],
			timeout_limits: None,
		}
	}

	/// Returns the settlement types supported by this plugin.
	fn supported_settlement_types(&self) -> Vec<SettlementType> {
		vec![SettlementType::Direct]
	}
}
