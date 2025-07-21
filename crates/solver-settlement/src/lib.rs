// solver-settlement/src/lib.rs

use solver_types::configs::SettlementConfig;
use solver_types::plugins::{
	FillData, PluginError, PluginHealth, PluginResult, SettlementPlugin, SettlementPriority,
	SettlementResult, SettlementStatus, SettlementTransaction, SettlementType,
};
use solver_types::{ChainId, FillEvent, PluginConfig, TxHash};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

type SettlementPluginMap = HashMap<String, Arc<dyn SettlementPlugin>>;
type SettlementPluginsType = Arc<RwLock<SettlementPluginMap>>;

/// Settlement request containing fill data and strategy preferences
#[derive(Debug, Clone)]
pub struct SettlementRequest {
	pub fill_event: FillEvent,
	pub fill_data: FillData,
	pub preferred_strategy: Option<String>,
	pub priority: SettlementPriority,
	pub order_type: String, // e.g., "eip7683_onchain"
	pub settlement_transaction: Option<SettlementTransaction>, // From order processor
}

/// Settlement response with transaction details
#[derive(Debug, Clone)]
pub struct SettlementResponse {
	pub settlement_id: String,
	pub tx_hash: TxHash,
	pub chain_id: ChainId,
	pub settlement_type: SettlementType,
	pub status: SettlementStatus,
	pub estimated_reward: u64,
	pub estimated_cost: u64,
	pub plugin_used: String,
}

/// Tracking settlement attempts
#[derive(Debug, Clone)]
pub struct SettlementTracker {
	pub request: SettlementRequest,
	pub attempts: Vec<SettlementAttempt>,
	pub started_at: u64,
	pub status: SettlementTrackingStatus,
}

#[derive(Debug, Clone)]
pub struct SettlementAttempt {
	pub plugin_name: String,
	pub started_at: u64,
	pub settlement_tx: Option<SettlementTransaction>,
	pub result: Option<SettlementResult>,
	pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SettlementTrackingStatus {
	Evaluating, // Checking profitability
	InProgress, // Settlement submitted
	Monitoring, // Monitoring confirmation
	Completed(SettlementResult),
	Failed(String),
}

/// Settlement service that orchestrates multiple settlement plugins
pub struct SettlementService {
	settlement_plugins: SettlementPluginsType,
	config: SettlementConfig,
	active_settlements: Arc<RwLock<HashMap<String, SettlementTracker>>>,
}

impl Default for SettlementService {
	fn default() -> Self {
		Self {
			settlement_plugins: Arc::new(RwLock::new(HashMap::new())),
			config: SettlementConfig {
				default_strategy: "direct_settlement".to_string(),
				fallback_strategies: vec![],
				profit_threshold_wei: "0".to_string(),
			},
			active_settlements: Arc::new(RwLock::new(HashMap::new())),
		}
	}
}

impl SettlementService {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn with_config(config: SettlementConfig) -> Self {
		Self {
			settlement_plugins: Arc::new(RwLock::new(HashMap::new())),
			config,
			active_settlements: Arc::new(RwLock::new(HashMap::new())),
		}
	}

	/// Main settle function - orchestrates plugin selection and execution
	pub async fn settle(&self, request: SettlementRequest) -> PluginResult<SettlementResponse> {
		info!(
			"Processing settlement request for order {} (fill: {})",
			request.fill_event.order_id, request.fill_event.fill_id
		);

		// Select appropriate plugin
		let plugin_name = self.select_plugin(&request).await?;
		let plugins = self.settlement_plugins.read().await;
		let plugin = plugins.get(&plugin_name).ok_or_else(|| {
			PluginError::NotFound(format!("Settlement plugin '{}' not found", plugin_name))
		})?;

		// Validate the fill
		let validation = plugin.validate_fill(&request.fill_data).await?;
		if !validation.is_valid {
			return Err(PluginError::ExecutionFailed(format!(
				"Fill validation failed: {:?}",
				validation.errors
			)));
		}

		// Check profitability
		let profit_threshold = self.config.profit_threshold_wei.parse::<i64>().unwrap_or(0);
		let estimate = plugin.estimate_settlement(&request.fill_data).await?;

		if estimate.net_profit < profit_threshold {
			return Err(PluginError::ExecutionFailed(format!(
				"Settlement not profitable: expected profit {} < threshold {}",
				estimate.net_profit, profit_threshold
			)));
		}

		// Use provided settlement transaction or prepare one
		let settlement_tx = if let Some(tx) = request.settlement_transaction.clone() {
			// Use the transaction from the order processor
			tx
		} else {
			// Prepare settlement transaction using plugin
			plugin.prepare_settlement(&request.fill_data).await?
		};

		// Create settlement ID
		let settlement_id = format!(
			"{}:{}:{}",
			request.fill_event.order_id,
			request.fill_event.fill_id,
			chrono::Utc::now().timestamp()
		);

		// Track the settlement
		let mut tracker = SettlementTracker {
			request: request.clone(),
			attempts: vec![],
			started_at: chrono::Utc::now().timestamp() as u64,
			status: SettlementTrackingStatus::InProgress,
		};

		// Execute settlement
		let result = plugin.execute_settlement(settlement_tx.clone()).await?;

		tracker.attempts.push(SettlementAttempt {
			plugin_name: plugin_name.clone(),
			started_at: chrono::Utc::now().timestamp() as u64,
			settlement_tx: Some(settlement_tx.clone()),
			result: Some(result.clone()),
			error: None,
		});

		// Store active settlement for monitoring
		self.active_settlements
			.write()
			.await
			.insert(settlement_id.clone(), tracker);

		// Create response
		let response = SettlementResponse {
			settlement_id: settlement_id.clone(),
			tx_hash: result.settlement_tx_hash.clone(),
			chain_id: request.fill_event.chain_id,
			settlement_type: settlement_tx.settlement_type,
			status: result.status,
			estimated_reward: estimate.estimated_reward,
			estimated_cost: estimate.estimated_cost,
			plugin_used: plugin_name,
		};

		info!(
			"Settlement {} submitted with tx hash: {}",
			settlement_id, response.tx_hash
		);

		Ok(response)
	}

	/// Monitor an active settlement
	pub async fn monitor_settlement(&self, settlement_id: &str) -> PluginResult<SettlementResult> {
		let active = self.active_settlements.read().await;
		let tracker = active.get(settlement_id).ok_or_else(|| {
			PluginError::NotFound(format!("Settlement '{}' not found", settlement_id))
		})?;

		// Get the plugin that was used
		let last_attempt = tracker.attempts.last().ok_or_else(|| {
			PluginError::ExecutionFailed("No settlement attempts found".to_string())
		})?;

		let plugins = self.settlement_plugins.read().await;
		let plugin = plugins.get(&last_attempt.plugin_name).ok_or_else(|| {
			PluginError::NotFound(format!(
				"Settlement plugin '{}' not found",
				last_attempt.plugin_name
			))
		})?;

		// Monitor the settlement
		let tx_hash = &last_attempt
			.result
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("No result found".to_string()))?
			.settlement_tx_hash;

		plugin.monitor_settlement(tx_hash).await
	}

	/// Select the best plugin for a settlement request
	async fn select_plugin(&self, request: &SettlementRequest) -> PluginResult<String> {
		// Use preferred strategy if specified
		if let Some(preferred) = &request.preferred_strategy {
			let plugins = self.settlement_plugins.read().await;
			if plugins.contains_key(preferred) {
				// Verify plugin can handle this chain
				let plugin = plugins.get(preferred).unwrap();
				if plugin
					.can_settle(request.fill_event.chain_id, &request.order_type)
					.await?
				{
					return Ok(preferred.clone());
				}
			}
		}

		// Otherwise use default strategy
		let plugins = self.settlement_plugins.read().await;

		// Try default strategy first
		if let Some(plugin) = plugins.get(&self.config.default_strategy) {
			if plugin
				.can_settle(request.fill_event.chain_id, &request.order_type)
				.await?
			{
				return Ok(self.config.default_strategy.clone());
			}
		}

		// Try fallback strategies
		for strategy in &self.config.fallback_strategies {
			if let Some(plugin) = plugins.get(strategy) {
				if plugin
					.can_settle(request.fill_event.chain_id, &request.order_type)
					.await?
				{
					return Ok(strategy.clone());
				}
			}
		}

		Err(PluginError::NotFound(format!(
			"No settlement plugin available for chain {}",
			request.fill_event.chain_id
		)))
	}

	/// Register a new settlement plugin
	pub async fn register_plugin(&self, name: String, plugin: Arc<dyn SettlementPlugin>) {
		info!("Registering settlement plugin: {}", name);
		self.settlement_plugins.write().await.insert(name, plugin);
	}

	/// Health check all settlement plugins
	pub async fn health_check(&self) -> PluginResult<HashMap<String, PluginHealth>> {
		let all_plugins = self.settlement_plugins.read().await;
		let mut health_status = HashMap::new();

		for (plugin_name, plugin) in all_plugins.iter() {
			match plugin.health_check().await {
				Ok(health) => {
					health_status.insert(plugin_name.clone(), health);
				}
				Err(error) => {
					health_status.insert(
						plugin_name.clone(),
						PluginHealth::unhealthy(format!("Health check failed: {}", error)),
					);
				}
			}
		}

		Ok(health_status)
	}

	/// Get active settlements
	pub async fn get_active_settlements(&self) -> Vec<String> {
		self.active_settlements
			.read()
			.await
			.keys()
			.cloned()
			.collect()
	}

	/// Clean up completed settlements
	pub async fn cleanup_completed_settlements(&self) {
		let mut active = self.active_settlements.write().await;
		let completed: Vec<String> = active
			.iter()
			.filter_map(|(id, tracker)| match &tracker.status {
				SettlementTrackingStatus::Completed(_) | SettlementTrackingStatus::Failed(_) => {
					Some(id.clone())
				}
				_ => None,
			})
			.collect();

		for id in completed {
			debug!("Removing completed settlement: {}", id);
			active.remove(&id);
		}
	}
}

/// Builder for SettlementService
pub struct SettlementServiceBuilder {
	plugins: Vec<(String, Box<dyn SettlementPlugin>, PluginConfig)>,
	config: SettlementConfig,
}

impl SettlementServiceBuilder {
	pub fn new() -> Self {
		Self {
			plugins: Vec::new(),
			config: SettlementConfig {
				default_strategy: "direct_settlement".to_string(),
				fallback_strategies: vec![],
				profit_threshold_wei: "0".to_string(),
			},
		}
	}

	pub fn with_plugin(
		mut self,
		name: String,
		plugin: Box<dyn SettlementPlugin>,
		config: PluginConfig,
	) -> Self {
		self.plugins.push((name, plugin, config));
		self
	}

	pub fn with_config(mut self, config: SettlementConfig) -> Self {
		self.config = config;
		self
	}

	pub async fn build(self) -> SettlementService {
		let service = SettlementService::with_config(self.config);

		// Initialize and register all plugins
		for (name, mut plugin, plugin_config) in self.plugins {
			// Initialize the plugin before registering
			match plugin.initialize(plugin_config).await {
				Ok(_) => {
					info!("Successfully initialized settlement plugin: {}", name);
					service.register_plugin(name, Arc::from(plugin)).await;
				}
				Err(e) => {
					error!("Failed to initialize settlement plugin {}: {}", name, e);
					// Skip registration if initialization fails
				}
			}
		}

		service
	}
}

impl Default for SettlementServiceBuilder {
	fn default() -> Self {
		Self::new()
	}
}
