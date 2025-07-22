// solver-delivery/src/lib.rs

use solver_types::configs::DeliveryConfig;
use solver_types::events::{FillEvent, OrderEvent};
use solver_types::plugins::{
	delivery::TransactionRequest, ChainId, DeliveryPlugin, DeliveryRequest, DeliveryResponse,
	DeliveryStrategy, OrderProcessor, PluginError, PluginResult, TxHash,
};
use solver_types::{DeliveryMetadata, DeliveryPriority, TransactionPriority};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Delivery service that orchestrates multiple delivery plugins
pub struct DeliveryService {
	delivery_plugins: Arc<RwLock<HashMap<String, Arc<dyn DeliveryPlugin>>>>,
	order_processors: Arc<RwLock<HashMap<String, Arc<dyn OrderProcessor>>>>,
	config: DeliveryConfig,
}

impl Default for DeliveryService {
	fn default() -> Self {
		Self {
			delivery_plugins: Arc::new(RwLock::new(HashMap::new())),
			order_processors: Arc::new(RwLock::new(HashMap::new())),
			config: DeliveryConfig {
				strategy: DeliveryStrategy::RoundRobin,
				fallback_enabled: false,
				max_parallel_attempts: 1,
			},
		}
	}
}

impl DeliveryService {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn with_config(config: DeliveryConfig) -> Self {
		Self {
			delivery_plugins: Arc::new(RwLock::new(HashMap::new())),
			order_processors: Arc::new(RwLock::new(HashMap::new())),
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

	/// Main transaction execution function - handles both fills and settlements
	pub async fn execute_transaction(
		&self,
		request: TransactionRequest,
	) -> PluginResult<DeliveryResponse> {
		let delivery_request = self.convert_to_delivery_request(request);
		self.deliver(delivery_request).await
	}

	/// Convert TransactionRequest to DeliveryRequest
	fn convert_to_delivery_request(&self, request: TransactionRequest) -> DeliveryRequest {
		DeliveryRequest {
			transaction: request.transaction,
			priority: match request.priority {
				TransactionPriority::Low => DeliveryPriority::Low,
				TransactionPriority::Normal => DeliveryPriority::Normal,
				TransactionPriority::High => DeliveryPriority::High,
				TransactionPriority::Urgent => DeliveryPriority::Urgent,
				TransactionPriority::Custom {
					max_fee,
					priority_fee,
					deadline,
				} => DeliveryPriority::Custom {
					max_fee,
					priority_fee,
					deadline,
				},
			},
			metadata: DeliveryMetadata {
				order_id: request.metadata.order_id,
				user: request.metadata.user,
				tags: request.metadata.tags,
				custom_fields: request.metadata.custom_fields,
			},
			retry_config: request.retry_config,
		}
	}

	/// Main delivery function - orchestrates plugin selection and execution
	pub async fn deliver(&self, request: DeliveryRequest) -> PluginResult<DeliveryResponse> {
		info!(
			"Starting delivery for chain {}",
			request.transaction.chain_id
		);

		// Get suitable plugins for this delivery
		let plugins = self.get_suitable_plugins(&request).await?;
		if plugins.is_empty() {
			return Err(PluginError::NotFound(
				"No suitable delivery plugins found".to_string(),
			));
		}

		// Execute delivery strategy
		match self.config.strategy {
			DeliveryStrategy::RoundRobin => self.deliver_round_robin(request, plugins).await,
		}
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

	/// Strategy: Round-robin between plugins (for load distribution)
	async fn deliver_round_robin(
		&self,
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

	/// Process an order event and return a transaction request
	pub async fn process_order_to_transaction(
		&self,
		event: &OrderEvent,
	) -> PluginResult<Option<TransactionRequest>> {
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

	/// Process a fill event and return a settlement transaction request
	pub async fn process_fill_to_transaction(
		&self,
		event: &FillEvent,
	) -> PluginResult<Option<TransactionRequest>> {
		let processors = self.order_processors.read().await;

		// Find a processor that can handle this order source
		for (name, processor) in processors.iter() {
			if processor.can_handle_source(&event.source) {
				info!(
					"Using order processor {} for settlement of source {}",
					name, event.source
				);
				return processor.process_fill_event(event).await;
			}
		}

		warn!("No order processor found for source: {}", event.source);
		Ok(None)
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

	/// Register an order processor
	pub async fn register_order_processor(&self, name: String, processor: Arc<dyn OrderProcessor>) {
		self.order_processors.write().await.insert(name, processor);
	}

	/// Register a new delivery plugin
	pub async fn register_plugin(&self, name: String, plugin: Arc<dyn DeliveryPlugin>) {
		self.delivery_plugins.write().await.insert(name, plugin);
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
