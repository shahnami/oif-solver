// solver-plugins/src/settlement/direct.rs

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use solver_types::*;
use std::any::Any;
use std::collections::HashMap;

/// Direct settlement plugin - settles immediately on origin chain
#[derive(Debug)]
pub struct DirectSettlementPlugin {
	config: DirectSettlementConfig,
	metrics: PluginMetrics,
	is_initialized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectSettlementConfig {
	pub settler_contracts: HashMap<ChainId, Address>,
	pub oracle_address: Address,
	pub min_confirmations: u32,
	pub gas_multiplier: f64,
	pub max_gas_price: u64,
	pub settlement_timeout: u64, // seconds
}

impl Default for DirectSettlementConfig {
	fn default() -> Self {
		Self {
			settler_contracts: HashMap::new(),
			oracle_address: "0x0000000000000000000000000000000000000000".to_string(),
			min_confirmations: 1,
			gas_multiplier: 1.2,
			max_gas_price: 100_000_000_000, // 100 gwei
			settlement_timeout: 300,        // 5 minutes
		}
	}
}

impl DirectSettlementPlugin {
	pub fn new() -> Self {
		Self {
			config: DirectSettlementConfig::default(),
			metrics: PluginMetrics::new(),
			is_initialized: false,
		}
	}

	fn create_settlement_transaction(&self, fill: &FillData) -> PluginResult<Transaction> {
		let settler_address = self
			.config
			.settler_contracts
			.get(&fill.chain_id)
			.ok_or_else(|| {
				PluginError::InvalidConfiguration(format!(
					"No settler contract for chain {}",
					fill.chain_id
				))
			})?;

		// Create settlement call data
		let call_data = self.encode_settlement_call(fill)?;

		Ok(Transaction {
			to: settler_address.clone(),
			value: 0,
			data: call_data,
			gas_limit: self.estimate_gas_limit(fill),
			gas_price: Some(self.calculate_gas_price()?),
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
			nonce: None,
			chain_id: fill.chain_id,
		})
	}

	fn encode_settlement_call(&self, fill: &FillData) -> PluginResult<Bytes> {
		// This would encode the actual settlement function call
		// For example: settle(bytes32 orderId, bytes32 fillHash, address filler)
		let mut call_data = Vec::new();

		// Function selector for settle(bytes32,bytes32,address)
		call_data.extend_from_slice(&[0x12, 0x34, 0x56, 0x78]); // Example selector

		// Encode parameters (simplified)
		call_data.extend_from_slice(fill.order_id.as_bytes());
		call_data.extend_from_slice(fill.fill_tx_hash.as_bytes());
		call_data.extend_from_slice(fill.filler_address.as_bytes());

		Ok(call_data.into())
	}

	fn estimate_gas_limit(&self, _fill: &FillData) -> u64 {
		// Estimate gas for settlement transaction
		150_000 // Base estimate for settlement
	}

	fn calculate_gas_price(&self) -> PluginResult<u64> {
		// This would query current network gas price
		// For now, return a reasonable default
		Ok(20_000_000_000) // 20 gwei
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
		if let Some(ConfigValue::Object(settler_contracts)) = config.config.get("settler_contracts")
		{
			for (chain_str, address_val) in settler_contracts {
				if let (Ok(chain_id), ConfigValue::String(address)) =
					(chain_str.parse::<ChainId>(), address_val)
				{
					self.config
						.settler_contracts
						.insert(chain_id, address.clone());
				}
			}
		}

		if let Some(ConfigValue::String(oracle)) = config.config.get("oracle_address") {
			self.config.oracle_address = oracle.clone();
		}

		if let Some(ConfigValue::Number(confirmations)) = config.config.get("min_confirmations") {
			self.config.min_confirmations = *confirmations as u32;
		}

		self.is_initialized = true;
		Ok(())
	}

	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()> {
		if !config.config.contains_key("settler_contracts") {
			return Err(PluginError::InvalidConfiguration(
				"settler_contracts is required".to_string(),
			));
		}

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

		if self.config.settler_contracts.is_empty() {
			return Ok(PluginHealth::unhealthy("No settler contracts configured"));
		}

		Ok(PluginHealth::healthy(
			"Direct settlement plugin is operational",
		))
	}

	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		Ok(self.metrics.clone())
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		self.is_initialized = false;
		Ok(())
	}

	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new()
			.required(
				"settler_contracts",
				ConfigFieldType::Object,
				"Mapping of chain ID to settler contract address",
			)
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
				"gas_multiplier",
				ConfigFieldType::Number,
				"Gas limit multiplier",
				Some(ConfigValue::Float(1.2)),
			)
			.optional(
				"max_gas_price",
				ConfigFieldType::Number,
				"Maximum gas price in wei",
				Some(ConfigValue::Number(100_000_000_000)),
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
	async fn can_settle(&self, chain_id: ChainId, _order_type: &str) -> PluginResult<bool> {
		Ok(self.config.settler_contracts.contains_key(&chain_id))
	}

	async fn prepare_settlement(&self, fill: &FillData) -> PluginResult<SettlementTransaction> {
		let transaction = self.create_settlement_transaction(fill)?;

		let metadata = solver_types::plugins::settlement::SettlementMetadata {
			order_id: fill.order_id.clone(),
			fill_hash: fill.fill_tx_hash.clone(),
			strategy: "direct".to_string(),
			expected_confirmations: self.config.min_confirmations,
			timeout: Some(
				std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap()
					.as_secs() + self.config.settlement_timeout,
			),
			custom_fields: HashMap::new(),
		};

		Ok(SettlementTransaction {
			transaction,
			settlement_type: SettlementType::Direct,
			expected_reward: self.calculate_expected_reward(fill)?,
			metadata,
		})
	}

	async fn execute_settlement(
		&self,
		settlement: SettlementTransaction,
	) -> PluginResult<SettlementResult> {
		// In a real implementation, this would submit the transaction via delivery plugin
		// For now, simulate successful execution

		let tx_hash = format!("0x{:x}", rand::random::<u64>());
		let timestamp = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		Ok(SettlementResult {
			settlement_tx_hash: tx_hash,
			status: SettlementStatus::Pending,
			timestamp,
			actual_reward: settlement.expected_reward,
			gas_used: settlement.transaction.gas_limit,
			confirmation_data: None,
		})
	}

	async fn monitor_settlement(&self, tx_hash: &TxHash) -> PluginResult<SettlementResult> {
		// In a real implementation, this would query the blockchain for transaction status
		// For now, simulate a confirmed settlement

		let timestamp = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		Ok(SettlementResult {
			settlement_tx_hash: tx_hash.clone(),
			status: SettlementStatus::Confirmed,
			timestamp,
			actual_reward: 1000000, // 1 USDC equivalent
			gas_used: 120000,
			confirmation_data: Some(solver_types::plugins::settlement::ConfirmationData {
				block_number: 12345678,
				block_hash: "0xabcdef...".to_string(),
				confirmation_count: self.config.min_confirmations,
				proof_data: None,
			}),
		})
	}

	async fn estimate_settlement(&self, fill: &FillData) -> PluginResult<SettlementEstimate> {
		let gas_estimate = self.estimate_gas_limit(fill);
		let gas_price = self.calculate_gas_price()?;
		let cost = (gas_estimate as f64 * gas_price as f64 * self.config.gas_multiplier) as u64;
		let reward = self.calculate_expected_reward(fill)?;

		Ok(SettlementEstimate {
			estimated_gas: gas_estimate,
			estimated_cost: cost,
			estimated_reward: reward,
			net_profit: reward as i64 - cost as i64,
			confidence_score: 0.95,
			estimated_time_to_settlement: Some(60), // 1 minute
			risks: vec![SettlementRisk {
				risk_type: "gas_price_volatility".to_string(),
				severity: RiskSeverity::Medium,
				description: "Gas prices may increase before settlement".to_string(),
				mitigation: Some("Use dynamic gas pricing".to_string()),
			}],
		})
	}

	async fn validate_fill(&self, fill: &FillData) -> PluginResult<FillValidation> {
		let mut errors = Vec::new();
		let mut warnings = Vec::new();

		// Check if we have a settler contract for this chain
		if !self.config.settler_contracts.contains_key(&fill.chain_id) {
			errors.push(format!(
				"No settler contract configured for chain {}",
				fill.chain_id
			));
		}

		// Check if fill is recent enough
		let current_time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		if current_time - fill.fill_timestamp > self.config.settlement_timeout {
			warnings.push("Fill is older than settlement timeout".to_string());
		}

		Ok(FillValidation {
			is_valid: errors.is_empty(),
			errors,
			warnings,
			required_proofs: vec![],
		})
	}

	fn get_settlement_requirements(&self) -> SettlementRequirements {
		let contract_requirements: Vec<ContractRequirement> = self
			.config
			.settler_contracts
			.iter()
			.map(|(chain_id, address)| ContractRequirement {
				contract_address: address.clone(),
				chain_id: *chain_id,
				contract_type: "InputSettler7683".to_string(),
				required_version: Some("1.0.0".to_string()),
			})
			.collect();

		SettlementRequirements {
			min_confirmations: self.config.min_confirmations,
			required_contracts: contract_requirements,
			oracle_requirements: vec![solver_types::plugins::settlement::OracleRequirement {
				oracle_address: self.config.oracle_address.clone(),
				oracle_type: "AlwaysYesOracle".to_string(),
				required_feed: "fill_verification".to_string(),
			}],
			timeout_limits: Some(TimeoutLimits {
				max_settlement_time: self.config.settlement_timeout,
				challenge_period: None,
			}),
		}
	}

	async fn is_profitable(&self, fill: &FillData) -> PluginResult<bool> {
		let estimate = self.estimate_settlement(fill).await?;
		Ok(estimate.net_profit > 0)
	}

	fn supported_settlement_types(&self) -> Vec<SettlementType> {
		vec![SettlementType::Direct]
	}

	async fn cancel_settlement(&self, _tx_hash: &TxHash) -> PluginResult<bool> {
		// Direct settlements cannot be cancelled once submitted
		Ok(false)
	}
}

impl DirectSettlementPlugin {
	fn calculate_expected_reward(&self, _fill: &FillData) -> PluginResult<u64> {
		// In a real implementation, this would calculate the actual reward
		// based on the order parameters and fill amount
		Ok(1000000) // 1 USDC equivalent
	}
}
