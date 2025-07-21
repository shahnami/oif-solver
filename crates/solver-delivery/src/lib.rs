// solver-delivery/src/lib.rs

use solver_types::configs::DeliveryConfig;
use solver_types::events::OrderEvent;
use solver_types::plugins::{
	ChainId, DeliveryPlugin, DeliveryRequest, DeliveryResponse, DeliveryStrategy, OrderProcessor,
	PluginError, PluginResult, TxHash,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

type DeliveryPluginMap = HashMap<String, Arc<dyn DeliveryPlugin>>;
type DeliveryPluginsType = Arc<RwLock<DeliveryPluginMap>>;

type OrderProcessorMap = HashMap<String, Arc<dyn OrderProcessor>>;
type OrderProcessorsType = Arc<RwLock<OrderProcessorMap>>;

/// Delivery service that orchestrates multiple delivery plugins
pub struct DeliveryService {
	delivery_plugins: DeliveryPluginsType,
	order_processors: OrderProcessorsType,
	active_deliveries: Arc<RwLock<HashMap<String, DeliveryTracker>>>,
	config: DeliveryConfig,
}

impl Default for DeliveryService {
	fn default() -> Self {
		Self {
			delivery_plugins: Arc::new(RwLock::new(HashMap::new())),
			order_processors: Arc::new(RwLock::new(HashMap::new())),
			active_deliveries: Arc::new(RwLock::new(HashMap::new())),
			config: DeliveryConfig {
				strategy: DeliveryStrategy::RoundRobin,
				fallback_enabled: false,
				max_parallel_attempts: 1,
			},
		}
	}
}

#[derive(Debug, Clone)]
pub struct DeliveryTracker {
	pub request: DeliveryRequest,
	pub attempts: Vec<DeliveryAttempt>,
	pub started_at: u64,
	pub status: DeliveryTrackingStatus,
}

#[derive(Debug, Clone)]
pub struct DeliveryAttempt {
	pub plugin_name: String,
	pub started_at: u64,
	pub response: Option<DeliveryResponse>,
	pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DeliveryTrackingStatus {
	InProgress,
	Completed(DeliveryResponse),
	Failed(String),
}

impl DeliveryService {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn with_config(config: DeliveryConfig) -> Self {
		Self {
			delivery_plugins: Arc::new(RwLock::new(HashMap::new())),
			order_processors: Arc::new(RwLock::new(HashMap::new())),
			active_deliveries: Arc::new(RwLock::new(HashMap::new())),
			config,
		}
	}

	pub fn with_fallback(mut self, enabled: bool) -> Self {
		self.config.fallback_enabled = enabled;
		self
	}

	pub fn with_max_parallel_attempts(mut self, max: usize) -> Self {
		self.config.max_parallel_attempts = max;
		self
	}

	/// Main delivery function - orchestrates plugin selection and execution
	pub async fn deliver(&self, request: DeliveryRequest) -> PluginResult<DeliveryResponse> {
		let delivery_id = format!(
			"delivery_{}",
			std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_nanos()
		);
		info!(
			"Starting delivery {} for chain {}",
			delivery_id, request.transaction.chain_id
		);

		// Create delivery tracker
		let tracker = DeliveryTracker {
			request: request.clone(),
			attempts: Vec::new(),
			started_at: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			status: DeliveryTrackingStatus::InProgress,
		};

		self.active_deliveries
			.write()
			.await
			.insert(delivery_id.clone(), tracker);

		// Get suitable plugins for this delivery
		let plugins = self.get_suitable_plugins(&request).await?;
		if plugins.is_empty() {
			return Err(PluginError::NotFound(
				"No suitable delivery plugins found".to_string(),
			));
		}

		// Execute delivery strategy
		let result = match self.config.strategy {
			DeliveryStrategy::RoundRobin => {
				self.deliver_round_robin(&delivery_id, request, plugins)
					.await
			}
		};

		// Update tracker with final result
		let mut deliveries = self.active_deliveries.write().await;
		if let Some(tracker) = deliveries.get_mut(&delivery_id) {
			match &result {
				Ok(response) => {
					tracker.status = DeliveryTrackingStatus::Completed(response.clone());
					info!("Delivery {} completed successfully", delivery_id);
				}
				Err(error) => {
					tracker.status = DeliveryTrackingStatus::Failed(error.to_string());
					error!("Delivery {} failed: {}", delivery_id, error);
				}
			}
		}

		result
	}

	/// Get plugins that can handle this delivery request
	async fn get_suitable_plugins(
		&self,
		request: &DeliveryRequest,
	) -> PluginResult<Vec<(String, Arc<dyn DeliveryPlugin>)>> {
		let all_plugins = self.delivery_plugins.read().await.clone();
		let mut suitable = Vec::new();

		for (name, plugin) in all_plugins {
			if plugin.can_deliver(request).await.unwrap_or(false) {
				suitable.push((name, plugin));
			}
		}

		debug!(
			"Found {} suitable plugins for chain {}",
			suitable.len(),
			request.transaction.chain_id
		);
		Ok(suitable)
	}

	/// Register a new delivery plugin
	pub async fn register_plugin(&self, name: String, plugin: Arc<dyn DeliveryPlugin>) {
		self.delivery_plugins.write().await.insert(name, plugin);
	}

	/// Strategy: Round-robin between plugins (for load distribution)
	async fn deliver_round_robin(
		&self,
		_delivery_id: &str,
		request: DeliveryRequest,
		plugins: Vec<(String, Arc<dyn DeliveryPlugin>)>,
	) -> PluginResult<DeliveryResponse> {
		// In real implementation, you'd track which plugin was used last
		// For now, just use the first plugin
		if let Some((plugin_name, plugin)) = plugins.first() {
			info!("Using round-robin plugin: {}", plugin_name);
			plugin.deliver(request).await
		} else {
			Err(PluginError::NotFound("No plugins available".to_string()))
		}
	}

	/// Record a delivery attempt
	async fn record_attempt(&self, delivery_id: &str, attempt: DeliveryAttempt) {
		if let Some(tracker) = self.active_deliveries.write().await.get_mut(delivery_id) {
			tracker.attempts.push(attempt);
		}
	}

	/// Get status of a delivery
	pub async fn get_delivery_status(&self, delivery_id: &str) -> Option<DeliveryTracker> {
		self.active_deliveries
			.read()
			.await
			.get(delivery_id)
			.cloned()
	}

	/// Get transaction status by hash (delegates to appropriate plugin)
	pub async fn get_transaction_status(
		&self,
		tx_hash: &TxHash,
		chain_id: ChainId,
	) -> PluginResult<Option<DeliveryResponse>> {
		let all_plugins = self.delivery_plugins.read().await;

		for (plugin_name, plugin) in all_plugins.iter() {
			if plugin.chain_id() == chain_id {
				debug!("Checking transaction status with plugin: {}", plugin_name);
				return plugin.get_transaction_status(tx_hash).await;
			}
		}

		Err(PluginError::NotFound(format!(
			"No delivery plugin found for chain {}",
			chain_id
		)))
	}

	/// Cancel a transaction (delegates to appropriate plugin)
	pub async fn cancel_transaction(
		&self,
		tx_hash: &TxHash,
		chain_id: ChainId,
	) -> PluginResult<bool> {
		let all_plugins = self.delivery_plugins.read().await;

		for (plugin_name, plugin) in all_plugins.iter() {
			if plugin.chain_id() == chain_id {
				info!(
					"Attempting to cancel transaction {} with plugin: {}",
					tx_hash, plugin_name
				);
				return plugin.cancel_transaction(tx_hash).await;
			}
		}

		Err(PluginError::NotFound(format!(
			"No delivery plugin found for chain {}",
			chain_id
		)))
	}

	/// Get network status for a chain
	pub async fn get_network_status(
		&self,
		chain_id: ChainId,
	) -> PluginResult<solver_types::plugins::NetworkStatus> {
		let all_plugins = self.delivery_plugins.read().await;

		for (plugin_name, plugin) in all_plugins.iter() {
			if plugin.chain_id() == chain_id {
				debug!("Getting network status from plugin: {}", plugin_name);
				return plugin.get_network_status().await;
			}
		}

		Err(PluginError::NotFound(format!(
			"No delivery plugin found for chain {}",
			chain_id
		)))
	}

	/// Health check all delivery plugins
	pub async fn health_check(
		&self,
	) -> PluginResult<HashMap<String, solver_types::plugins::PluginHealth>> {
		let all_plugins = self.delivery_plugins.read().await;
		let mut health_status = HashMap::new();

		for (plugin_name, plugin) in all_plugins.iter() {
			match plugin.health_check().await {
				Ok(health) => {
					health_status.insert(plugin_name.clone(), health);
				}
				Err(error) => {
					health_status.insert(
						plugin_name.clone(),
						solver_types::plugins::PluginHealth::unhealthy(format!(
							"Health check failed: {}",
							error
						)),
					);
				}
			}
		}

		Ok(health_status)
	}

	/// Process an order event by finding the appropriate order processor
	pub async fn process_order(&self, event: &OrderEvent) -> PluginResult<Option<DeliveryRequest>> {
		let processors = self.order_processors.read().await;

		// Find a processor that can handle this order source
		for (name, processor) in processors.iter() {
			if processor.can_handle_source(&event.source) {
				info!("Using order processor {} for source {}", name, event.source);
				return processor.process_order_event(event).await;
			}
		}

		warn!("No order processor found for source: {}", event.source);
		Ok(None)
	}

	/// Register an order processor
	pub async fn register_order_processor(&self, name: String, processor: Arc<dyn OrderProcessor>) {
		self.order_processors.write().await.insert(name, processor);
	}
}

/// Builder for DeliveryService
pub struct DeliveryServiceBuilder {
	plugins: Vec<(
		String,
		Box<dyn DeliveryPlugin>,
		solver_types::plugins::PluginConfig,
	)>,
	order_processors: Vec<(String, Arc<dyn OrderProcessor>)>,
	config: DeliveryConfig,
}

impl DeliveryServiceBuilder {
	pub fn new() -> Self {
		Self {
			plugins: Vec::new(),
			order_processors: Vec::new(),
			config: DeliveryConfig {
				strategy: DeliveryStrategy::RoundRobin,
				fallback_enabled: false,
				max_parallel_attempts: 1,
			},
		}
	}

	pub fn with_plugin(
		mut self,
		name: String,
		plugin: Box<dyn DeliveryPlugin>,
		config: solver_types::plugins::PluginConfig,
	) -> Self {
		self.plugins.push((name, plugin, config));
		self
	}

	pub fn with_config(mut self, config: DeliveryConfig) -> Self {
		self.config = config;
		self
	}

	pub fn with_fallback(mut self, enabled: bool) -> Self {
		self.config.fallback_enabled = enabled;
		self
	}

	pub fn with_max_parallel_attempts(mut self, max: usize) -> Self {
		self.config.max_parallel_attempts = max;
		self
	}

	pub fn with_order_processor(
		mut self,
		name: String,
		processor: Arc<dyn OrderProcessor>,
	) -> Self {
		self.order_processors.push((name, processor));
		self
	}

	pub async fn build(self) -> DeliveryService {
		let service = DeliveryService::with_config(self.config);

		// Initialize and register all plugins
		for (name, mut plugin, plugin_config) in self.plugins {
			// Initialize the plugin before registering
			match plugin.initialize(plugin_config).await {
				Ok(_) => {
					info!("Successfully initialized delivery plugin: {}", name);
					service.register_plugin(name, Arc::from(plugin)).await;
				}
				Err(e) => {
					error!("Failed to initialize delivery plugin {}: {}", name, e);
					// Skip registration if initialization fails
				}
			}
		}

		// Register all order processors
		for (name, processor) in self.order_processors {
			service
				.register_order_processor(name.clone(), processor)
				.await;
			info!("Registered order processor: {}", name);
		}

		service
	}
}

impl Default for DeliveryServiceBuilder {
	fn default() -> Self {
		Self::new()
	}
}
