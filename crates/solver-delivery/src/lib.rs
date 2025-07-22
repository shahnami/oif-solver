//! # Delivery Service
//!
//! Manages order execution and transaction delivery across multiple blockchain networks.
//!
//! This crate provides the delivery service that coordinates between order processors
//! and delivery plugins to execute orders and settlements on target blockchains.
//! It supports multiple delivery strategies, fallback mechanisms, and plugin-based
//! extensibility for different blockchain protocols and transaction types.
//!
//! ## Architecture
//!
//! The delivery service operates through two main components:
//! - **Order Processors**: Transform order events into executable transactions
//! - **Delivery Plugins**: Execute transactions on specific blockchain networks
//!
//! ## Supported Operations
//!
//! - Order-to-transaction conversion and execution
//! - Fill-to-settlement transaction processing
//! - Transaction status monitoring and retrieval
//! - Multi-plugin orchestration with fallback strategies

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

/// Delivery service that orchestrates transaction execution across multiple plugins.
///
/// The delivery service acts as a coordinator between order processors and delivery
/// plugins, managing the complete lifecycle from order discovery to transaction
/// execution. It supports configurable delivery strategies, fallback mechanisms,
/// and real-time transaction monitoring.
pub struct DeliveryService {
	/// Registry of available delivery plugins by name
	delivery_plugins: Arc<RwLock<HashMap<String, Arc<dyn DeliveryPlugin>>>>,
	/// Registry of order processors that convert orders to transactions
	order_processors: Arc<RwLock<HashMap<String, Arc<dyn OrderProcessor>>>>,
	/// Configuration for delivery behavior and strategies
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
	/// Create a new delivery service with default configuration.
	pub fn new() -> Self {
		Self::default()
	}

	/// Create a delivery service with the specified configuration.
	///
	/// # Arguments
	/// * `config` - Delivery service configuration including strategy and limits
	pub fn with_config(config: DeliveryConfig) -> Self {
		Self {
			delivery_plugins: Arc::new(RwLock::new(HashMap::new())),
			order_processors: Arc::new(RwLock::new(HashMap::new())),
			config,
		}
	}

	/// Enable or disable fallback delivery mechanisms.
	///
	/// # Arguments
	/// * `enabled` - Whether to enable fallback when primary delivery fails
	pub fn with_fallback(mut self, enabled: bool) -> Self {
		self.config.fallback_enabled = enabled;
		self
	}

	/// Set the maximum number of parallel delivery attempts.
	///
	/// # Arguments
	/// * `max` - Maximum concurrent delivery operations
	pub fn with_max_parallel_attempts(mut self, max: usize) -> Self {
		self.config.max_parallel_attempts = max;
		self
	}

	/// Execute a transaction request using available delivery plugins.
	///
	/// This is the primary entry point for transaction execution, handling both
	/// order fills and settlement transactions by converting the request format
	/// and delegating to the appropriate delivery strategy.
	///
	/// # Arguments
	/// * `request` - Transaction request containing transaction data and metadata
	///
	/// # Returns
	/// Delivery response with transaction hash and status information
	pub async fn execute_transaction(
		&self,
		request: TransactionRequest,
	) -> PluginResult<DeliveryResponse> {
		let delivery_request = self.convert_to_delivery_request(request);
		self.deliver(delivery_request).await
	}

	/// Convert transaction request to delivery request format.
	///
	/// Transforms the transaction request format used by the orchestrator
	/// into the delivery request format expected by delivery plugins.
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

	/// Execute delivery request using configured strategy and available plugins.
	///
	/// Selects appropriate delivery plugins based on the request requirements
	/// and executes the delivery using the configured strategy (e.g., round-robin).
	/// Handles plugin selection, error handling, and strategy execution.
	///
	/// # Arguments
	/// * `request` - Delivery request with transaction data and preferences
	///
	/// # Returns
	/// Delivery response from the executing plugin
	///
	/// # Errors
	/// Returns error if no suitable plugins are found or delivery fails
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
			debug!("Using round-robin plugin: {}", plugin_name);
			plugin.deliver(request).await
		} else {
			Err(PluginError::NotFound("No plugins available".to_string()))
		}
	}

	/// Process an order event into an executable transaction request.
	///
	/// Uses registered order processors to transform order events into
	/// transaction requests that can be executed by delivery plugins.
	/// Selects the appropriate processor based on the order source.
	///
	/// # Arguments
	/// * `event` - Order event containing order data and metadata
	///
	/// # Returns
	/// Transaction request if a suitable processor is found, None otherwise
	pub async fn process_order_to_transaction(
		&self,
		event: &OrderEvent,
	) -> PluginResult<Option<TransactionRequest>> {
		let processors = self.order_processors.read().await;

		// Find a processor that can handle this order source
		for (name, processor) in processors.iter() {
			if processor.can_handle_source(&event.source) {
				debug!("Using order processor {} for source {}", name, event.source);
				return processor.process_order_event(event).await;
			}
		}

		warn!("No order processor found for source: {}", event.source);
		Ok(None)
	}

	/// Process a fill event into a settlement transaction request.
	///
	/// Transforms confirmed fill events into settlement transaction requests
	/// that can be executed to complete cross-chain settlement operations.
	/// Uses order processors to handle protocol-specific settlement logic.
	///
	/// # Arguments
	/// * `event` - Fill event containing transaction hash and order data
	///
	/// # Returns
	/// Settlement transaction request if processing succeeds, None otherwise
	pub async fn process_fill_to_transaction(
		&self,
		event: &FillEvent,
	) -> PluginResult<Option<TransactionRequest>> {
		let processors = self.order_processors.read().await;

		// Find a processor that can handle this order source
		for (name, processor) in processors.iter() {
			if processor.can_handle_source(&event.source) {
				debug!(
					"Using order processor {} for settlement of source {}",
					name, event.source
				);
				return processor.process_fill_event(event).await;
			}
		}

		warn!("No order processor found for source: {}", event.source);
		Ok(None)
	}

	/// Retrieve the current status of a transaction by hash.
	///
	/// Delegates to the appropriate delivery plugin based on chain ID to
	/// check the current status of a previously submitted transaction.
	/// Used for monitoring transaction confirmations and detecting failures.
	///
	/// # Arguments
	/// * `tx_hash` - Transaction hash to check
	/// * `chain_id` - Blockchain network where the transaction was submitted
	///
	/// # Returns
	/// Current transaction status if found, None if transaction not found
	///
	/// # Errors
	/// Returns error if no suitable plugin is found for the chain
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
		info!("Starting {}", name);
		self.delivery_plugins
			.write()
			.await
			.insert(name.clone(), plugin);
		info!("{} started successfully", name);
	}
}

/// Builder for constructing DeliveryService instances.
///
/// Provides a fluent interface for configuring and building delivery services
/// with plugins, processors, and configuration options. Handles plugin
/// initialization and registration during the build process.
pub struct DeliveryServiceBuilder {
	/// Plugins to register with their configurations
	plugins: Vec<(
		String,
		Box<dyn DeliveryPlugin>,
		solver_types::plugins::PluginConfig,
	)>,
	/// Order processors to register
	order_processors: Vec<(String, Arc<dyn OrderProcessor>)>,
	/// Service configuration
	config: DeliveryConfig,
}

impl DeliveryServiceBuilder {
	/// Create a new delivery service builder with default configuration.
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

	/// Add a delivery plugin to be registered with the service.
	///
	/// # Arguments
	/// * `name` - Unique name for the plugin
	/// * `plugin` - Plugin implementation
	/// * `config` - Plugin-specific configuration
	pub fn with_plugin(
		mut self,
		name: String,
		plugin: Box<dyn DeliveryPlugin>,
		config: solver_types::plugins::PluginConfig,
	) -> Self {
		self.plugins.push((name, plugin, config));
		self
	}

	/// Set the delivery service configuration.
	///
	/// # Arguments
	/// * `config` - Service configuration including strategy and limits
	pub fn with_config(mut self, config: DeliveryConfig) -> Self {
		self.config = config;
		self
	}

	/// Enable or disable fallback delivery mechanisms.
	///
	/// # Arguments
	/// * `enabled` - Whether to enable fallback when primary delivery fails
	pub fn with_fallback(mut self, enabled: bool) -> Self {
		self.config.fallback_enabled = enabled;
		self
	}

	/// Set maximum parallel delivery attempts.
	///
	/// # Arguments
	/// * `max` - Maximum concurrent delivery operations
	pub fn with_max_parallel_attempts(mut self, max: usize) -> Self {
		self.config.max_parallel_attempts = max;
		self
	}

	/// Add an order processor to be registered with the service.
	///
	/// # Arguments
	/// * `name` - Unique name for the processor
	/// * `processor` - Order processor implementation
	pub fn with_order_processor(
		mut self,
		name: String,
		processor: Arc<dyn OrderProcessor>,
	) -> Self {
		self.order_processors.push((name, processor));
		self
	}

	/// Build the delivery service with all configured plugins and processors.
	///
	/// Initializes all plugins, registers them with the service, and sets up
	/// order processors. Plugin initialization failures are logged but do not
	/// prevent service creation.
	///
	/// # Returns
	/// Configured delivery service ready for use
	pub async fn build(self) -> DeliveryService {
		let service = DeliveryService::with_config(self.config);

		// Initialize and register all plugins
		for (name, mut plugin, plugin_config) in self.plugins {
			// Initialize the plugin before registering
			match plugin.initialize(plugin_config).await {
				Ok(_) => {
					debug!("Successfully initialized delivery plugin: {}", name);
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
			info!("Registering order processor: {}", name);
			service
				.register_order_processor(name.clone(), processor)
				.await;
		}

		service
	}
}

impl Default for DeliveryServiceBuilder {
	fn default() -> Self {
		Self::new()
	}
}
