// solver-plugins/src/settlement/direct.rs

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solver_types::*;
use std::any::Any;

/// Direct settlement plugin - settles immediately on origin chain
#[derive(Debug, Default)]
pub struct DirectSettlementPlugin {
	config: DirectSettlementConfig,
	is_initialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectSettlementConfig {
	pub oracle_address: Address,
	pub min_confirmations: u32,
	pub dispute_period_seconds: u64,
	pub claim_window_seconds: u64,
}

impl Default for DirectSettlementConfig {
	fn default() -> Self {
		Self {
			oracle_address: "0x0000000000000000000000000000000000000000".to_string(),
			min_confirmations: 1,
			dispute_period_seconds: 300, // 5 minutes
			claim_window_seconds: 86400, // 24 hours
		}
	}
}

impl DirectSettlementPlugin {
	pub fn new() -> Self {
		Self {
			config: DirectSettlementConfig::default(),
			is_initialized: false,
		}
	}

	pub fn with_config(config: DirectSettlementConfig) -> Self {
		Self {
			config,
			is_initialized: false,
		}
	}
}

#[async_trait]
impl BasePlugin for DirectSettlementPlugin {
	fn plugin_type(&self) -> &'static str {
		"direct_settlement"
	}

	fn name(&self) -> String {
		"Direct Settlement Plugin".to_string()
	}

	fn version(&self) -> &'static str {
		"1.0.0"
	}

	fn description(&self) -> &'static str {
		"Direct settlement plugin that settles orders immediately on the origin chain"
	}

	async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()> {
		// Parse configuration
		if let Some(ConfigValue::String(oracle)) = config.config.get("oracle_address") {
			self.config.oracle_address = oracle.clone();
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
		if !config.config.contains_key("oracle_address") {
			return Err(PluginError::InvalidConfiguration(
				"oracle_address is required".to_string(),
			));
		}

		Ok(())
	}

	async fn health_check(&self) -> PluginResult<PluginHealth> {
		if !self.is_initialized {
			return Ok(PluginHealth::unhealthy("Plugin not initialized"));
		}

		Ok(PluginHealth::healthy(
			"Direct settlement plugin is operational",
		))
	}

	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		Ok(PluginMetrics::new())
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		self.is_initialized = false;
		Ok(())
	}

	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new()
			.required(
				"oracle_address",
				ConfigFieldType::String,
				"Oracle contract address for settlement verification",
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

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

#[async_trait]
impl SettlementPlugin for DirectSettlementPlugin {
	async fn can_handle(&self, _chain_id: ChainId, _order_type: &str) -> PluginResult<bool> {
		// Can handle any chain since we only check oracle attestations
		Ok(true)
	}

	async fn check_oracle_attestation(&self, fill: &FillData) -> PluginResult<AttestationStatus> {
		// For direct settlement, check if oracle has attested the fill
		// In a real implementation, this would query the oracle contract

		// Simulate oracle attestation after some time
		let current_time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let time_since_fill = current_time.saturating_sub(fill.fill_timestamp);
		let is_attested = time_since_fill > 30; // Assume attestation after 30 seconds

		tracing::info!(
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

	fn supported_settlement_types(&self) -> Vec<SettlementType> {
		vec![SettlementType::Direct]
	}
}
