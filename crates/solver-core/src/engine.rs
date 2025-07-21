// solver-core/src/engine.rs

use crate::{
	error::CoreError,
	lifecycle::{LifecycleManager, LifecycleState},
};
use solver_config::{ConfigError, ConfigLoader};
use solver_delivery::{DeliveryService, DeliveryServiceBuilder};
use solver_discovery::{DiscoveryService, DiscoveryServiceBuilder};
use solver_plugin::factory::global_plugin_factory;
use solver_settlement::{SettlementService, SettlementServiceBuilder};
use solver_state::{StateService, StateServiceBuilder};
use solver_types::plugins::*;
use solver_types::{Event, EventType, *};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

/// Create a state plugin from configuration
fn create_state_plugin(config: &PluginConfig) -> Result<Arc<dyn StatePlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_state_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create a delivery plugin from configuration
fn create_delivery_plugin(config: &PluginConfig) -> Result<Box<dyn DeliveryPlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_delivery_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create a discovery plugin from configuration
fn create_discovery_plugin(config: &PluginConfig) -> Result<Box<dyn DiscoveryPlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_discovery_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create a settlement plugin from configuration
fn create_settlement_plugin(config: &PluginConfig) -> Result<Box<dyn SettlementPlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_settlement_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create a order processor from configuration
fn create_order_processor(config: &PluginConfig) -> Result<Arc<dyn OrderProcessor>, CoreError> {
	let factory = global_plugin_factory();

	let processor = factory
		.create_order_processor(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	Ok(processor)
}

// Event sender type alias for clarity
pub type EventSender = mpsc::UnboundedSender<Event>;

/// Health status for services
#[derive(Debug, Clone)]
pub struct HealthReport {
	pub discovery_healthy: bool,
	pub delivery_healthy: bool,
	pub settlement_healthy: bool,
	pub state_healthy: bool,
	pub event_processor_healthy: bool,
	pub overall_status: ServiceStatus,
}

/// Core orchestrator that coordinates all solver components
pub struct Orchestrator {
	/// Configuration
	config: Arc<RwLock<SolverConfig>>,

	/// Core services
	discovery_service: Arc<DiscoveryService>,
	delivery_service: Arc<DeliveryService>,
	settlement_service: Arc<SettlementService>,

	state_service: Arc<StateService>,

	/// Event coordination - owned directly by orchestrator
	event_tx: EventSender,
	event_rx: Arc<Mutex<mpsc::UnboundedReceiver<Event>>>,

	/// Lifecycle management
	lifecycle_manager: Arc<LifecycleManager>,

	/// Shutdown signal
	shutdown_tx: broadcast::Sender<()>,

	/// Background tasks
	tasks: Arc<Mutex<JoinSet<Result<(), CoreError>>>>,

	/// Pending fills that are being monitored
	pending_fills: Arc<RwLock<HashMap<String, FillEvent>>>,
}

impl Orchestrator {
	/// Start the orchestrator and all services
	pub async fn start(&self) -> Result<(), CoreError> {
		info!("Starting orchestrator");

		// Initialize lifecycle
		self.lifecycle_manager.initialize().await?;

		// Start services in order
		self.start_state_service().await?;
		self.start_discovery_service().await?;
		self.start_delivery_service().await?;
		self.start_settlement_service().await?;

		// Start event processing
		self.start_event_processing().await?;

		// Start health monitoring
		self.start_health_monitor().await?;

		// Start fill monitoring
		self.start_fill_monitor().await?;

		// Mark as running
		self.lifecycle_manager.start().await?;

		info!("Orchestrator started successfully");
		Ok(())
	}

	async fn start_state_service(&self) -> Result<(), CoreError> {
		info!("Starting state service");

		// Initialize the state service with its configured backend
		self.state_service.initialize().await.map_err(|e| {
			CoreError::ServiceInit(format!("Failed to initialize state service: {}", e))
		})?;

		// Start cleanup task
		self.state_service.start_cleanup_task().await;

		Ok(())
	}

	async fn start_discovery_service(&self) -> Result<(), CoreError> {
		info!("Starting discovery service");

		// Start all registered discovery plugins
		self.discovery_service.start_all().await.map_err(|e| {
			CoreError::ServiceInit(format!("Failed to start discovery service: {}", e))
		})?;

		Ok(())
	}

	async fn start_delivery_service(&self) -> Result<(), CoreError> {
		info!("Starting delivery service");

		// Delivery service doesn't have a specific start method
		// It's ready to use once created with plugins
		// TODO: Add health check to verify all configured plugins are ready

		Ok(())
	}

	async fn start_settlement_service(&self) -> Result<(), CoreError> {
		info!("Starting settlement service");

		// Settlement service doesn't have a specific start method
		// It's ready to use once created with plugins
		// TODO: Add health check to verify all configured plugins are ready

		Ok(())
	}

	async fn start_event_processing(&self) -> Result<(), CoreError> {
		info!("Starting event processing");

		let orchestrator = self.clone();
		let mut tasks = self.tasks.lock().await;

		tasks.spawn(async move { orchestrator.process_events().await });

		Ok(())
	}

	/// Main event processing loop
	async fn process_events(&self) -> Result<(), CoreError> {
		let mut event_rx = self.event_rx.lock().await;
		let mut shutdown_rx = self.lifecycle_manager.subscribe_shutdown();

		loop {
			tokio::select! {
				Some(event) = event_rx.recv() => {
					debug!("Processing event: {:?}", event);

					if let Err(e) = self.handle_event(event).await {
						warn!("Error handling event: {}", e);
					}
				}
				_ = shutdown_rx.recv() => {
					info!("Event processor received shutdown signal");
					break;
				}
			}
		}

		Ok(())
	}

	/// Handle a single event
	async fn handle_event(&self, event: Event) -> Result<(), CoreError> {
		match event {
			Event::Discovery(discovery_event) => self.handle_discovery_event(discovery_event).await,
			Event::OrderCreated(order_event) => self.handle_order_created_event(order_event).await,
			Event::OrderFill(fill_event) => self.handle_order_fill_event(fill_event).await,
			Event::Settlement(settlement_event) => {
				self.handle_settlement_event(settlement_event).await
			}
			Event::ServiceStatus(status_event) => {
				self.handle_service_status_event(status_event).await
			}
		}
	}

	async fn handle_discovery_event(&self, event: DiscoveryEvent) -> Result<(), CoreError> {
		info!("Processing discovery event: {}", event.id);

		// Store event in state
		// TODO: Implement state storage for discovery events

		// Convert to order if applicable
		if event.event_type == EventType::OrderCreated {
			let order_event = OrderEvent {
				order_id: event.id.clone(),
				chain_id: event.chain_id,
				user: event
					.parsed_data
					.as_ref()
					.and_then(|d| d.user.clone())
					.unwrap_or_default(),
				timestamp: event.timestamp,
				metadata: event.metadata.source_specific.clone(),
				source: event.source.clone(),
				contract_address: event
					.parsed_data
					.as_ref()
					.and_then(|d| d.contract_address.clone()),
				raw_data: event.raw_data.clone(),
			};

			self.event_tx
				.send(Event::OrderCreated(order_event))
				.map_err(|_| CoreError::Channel("Failed to send order event".to_string()))?;
		}

		Ok(())
	}

	async fn handle_order_created_event(&self, event: OrderEvent) -> Result<(), CoreError> {
		info!("Processing order created: {}", event.order_id);

		// Use delivery service to process the order
		match self
			.delivery_service
			.process_order_to_transaction(&event)
			.await
		{
			Ok(Some(transaction_request)) => {
				info!("Creating transaction request for order {}", event.order_id);
				// Submit the transaction request
				match self
					.delivery_service
					.execute_transaction(transaction_request)
					.await
				{
					Ok(response) => {
						info!("Order {} delivered: {:?}", event.order_id, response);
						let fill_event = FillEvent {
							order_id: event.order_id.clone(),
							fill_id: response.tx_hash.clone(),
							chain_id: response.chain_id,
							tx_hash: response.tx_hash.clone(),
							timestamp: response.submitted_at,
							status: match response.status {
								DeliveryStatus::Submitted => FillStatus::Pending,
								DeliveryStatus::Pending => FillStatus::Pending,
								DeliveryStatus::Confirmed => FillStatus::Confirmed,
								DeliveryStatus::Failed => {
									FillStatus::Failed("Delivery failed".to_string())
								}
								DeliveryStatus::Dropped => {
									FillStatus::Failed("Transaction dropped".to_string())
								}
								DeliveryStatus::Replaced => {
									FillStatus::Failed("Transaction replaced".to_string())
								}
							},
							source: event.source.clone(),
							order_data: Some(event.raw_data.clone()),
						};

						if let Err(e) = self.event_tx.send(Event::OrderFill(fill_event)) {
							warn!("Failed to send OrderFill event: {}", e);
						}
					}
					Err(e) => {
						CoreError::Delivery(format!(
							"Failed to deliver order {}: {}",
							event.order_id, e
						));
					}
				}
			}
			Ok(None) => {
				warn!("No delivery request created for order {}", event.order_id);
			}
			Err(e) => {
				CoreError::Delivery(format!("Failed to process order {}: {}", event.order_id, e));
			}
		}

		Ok(())
	}

	async fn handle_order_fill_event(&self, event: FillEvent) -> Result<(), CoreError> {
		info!(
			"Processing order fill: {} (fill: {}, status: {:?})",
			event.order_id, event.fill_id, event.status
		);

		match event.status {
			FillStatus::Pending => {
				// Store pending fill for monitoring
				info!(
					"Fill {} is pending, adding to monitoring queue",
					event.fill_id
				);
				self.pending_fills
					.write()
					.await
					.insert(event.fill_id.clone(), event);
			}
			FillStatus::Confirmed => {
				// Remove from pending if it was there
				self.pending_fills.write().await.remove(&event.fill_id);

				// Process settlement
				self.process_confirmed_fill(event).await?;
			}
			FillStatus::Failed(_) => {
				// Remove from pending if it was there
				self.pending_fills.write().await.remove(&event.fill_id);

				warn!(
					"Fill {} failed for order {}, not processing settlement",
					event.fill_id, event.order_id
				);
			}
		}

		Ok(())
	}

	async fn process_confirmed_fill(&self, event: FillEvent) -> Result<(), CoreError> {
		info!(
			"Fill confirmed, triggering settlement for order {}",
			event.order_id
		);

		// Process through delivery service order processors to get settlement transaction
		match self
			.delivery_service
			.process_fill_to_transaction(&event)
			.await
		{
			Ok(Some(transaction_request)) => {
				info!(
					"Creating settlement transaction for order {}",
					event.order_id
				);

				// Submit the settlement transaction through delivery service
				match self
					.delivery_service
					.execute_transaction(transaction_request)
					.await
				{
					Ok(response) => {
						// Create settlement ID
						let settlement_id = format!(
							"{}:{}:{}",
							event.order_id,
							event.fill_id,
							chrono::Utc::now().timestamp()
						);

						info!(
							"Settlement {} initiated for order {} with tx hash: {}",
							settlement_id, event.order_id, response.tx_hash
						);

						// Create and send Settlement event
						let settlement_event = SettlementEvent {
							order_id: event.order_id.clone(),
							settlement_id,
							source_chain: event.chain_id,
							destination_chain: event.chain_id, // TODO: Set to actual destination chain
							tx_hash: response.tx_hash,
							timestamp: chrono::Utc::now().timestamp() as u64,
							status: match response.status {
								DeliveryStatus::Submitted => SettlementStatus::Pending,
								DeliveryStatus::Pending => SettlementStatus::Pending,
								DeliveryStatus::Confirmed => SettlementStatus::Confirmed,
								DeliveryStatus::Failed => SettlementStatus::Failed,
								DeliveryStatus::Dropped => SettlementStatus::Failed,
								DeliveryStatus::Replaced => SettlementStatus::Failed,
							},
						};

						if let Err(e) = self.event_tx.send(Event::Settlement(settlement_event)) {
							warn!("Failed to send Settlement event: {}", e);
						}
					}
					Err(e) => {
						warn!(
							"Failed to initiate settlement for order {}: {}",
							event.order_id, e
						);
					}
				}
			}
			Ok(None) => {
				// No order processor created a settlement transaction
				info!(
					"No order processor created settlement transaction for order {}",
					event.order_id
				);
				// In the new architecture, if no processor handles settlement, we skip it
				// (settlements are optional and depend on the order type)
			}
			Err(e) => {
				warn!("Failed to process fill event: {}", e);
			}
		}

		Ok(())
	}

	async fn handle_settlement_event(&self, event: SettlementEvent) -> Result<(), CoreError> {
		info!(
			"Processing settlement complete: {} (settlement: {})",
			event.order_id, event.settlement_id
		);

		info!(
			"Settlement {} for order {} completed with status: {:?}",
			event.settlement_id, event.order_id, event.status
		);

		Ok(())
	}

	async fn handle_service_status_event(&self, event: StatusEvent) -> Result<(), CoreError> {
		match event.status {
			ServiceStatus::Unhealthy => {
				warn!(
					"Service {} is unhealthy: {:?}",
					event.service, event.details
				);
			}
			ServiceStatus::Degraded => {
				warn!("Service {} is degraded: {:?}", event.service, event.details);
			}
			_ => {
				info!("Service {} status: {:?}", event.service, event.status);
			}
		}

		Ok(())
	}

	async fn start_health_monitor(&self) -> Result<(), CoreError> {
		let orchestrator = self.clone();
		let mut tasks = self.tasks.lock().await;

		tasks.spawn(async move { orchestrator.monitor_health().await });

		Ok(())
	}

	async fn start_fill_monitor(&self) -> Result<(), CoreError> {
		let orchestrator = self.clone();
		let mut tasks = self.tasks.lock().await;

		tasks.spawn(async move { orchestrator.monitor_fills().await });

		Ok(())
	}

	async fn monitor_health(&self) -> Result<(), CoreError> {
		let mut interval = tokio::time::interval(Duration::from_secs(30));
		let mut shutdown_rx = self.lifecycle_manager.subscribe_shutdown();

		loop {
			tokio::select! {
				_ = interval.tick() => {
					let health = self.get_health().await;

					if !matches!(health.overall_status, ServiceStatus::Healthy) {
						warn!("System health degraded: {:?}", health);
					}
				}
				_ = shutdown_rx.recv() => {
					info!("Health monitor received shutdown signal");
					break;
				}
			}
		}

		Ok(())
	}

	async fn monitor_fills(&self) -> Result<(), CoreError> {
		let mut interval = tokio::time::interval(Duration::from_secs(5)); // Check every 5 seconds
		let mut shutdown_rx = self.lifecycle_manager.subscribe_shutdown();

		loop {
			tokio::select! {
				_ = interval.tick() => {
					// Get all pending fills
					let pending_fills: Vec<FillEvent> = {
						let fills = self.pending_fills.read().await;
						fills.values().cloned().collect()
					};

					if !pending_fills.is_empty() {
						debug!("Monitoring {} pending fills", pending_fills.len());
					}

					// Check status of each pending fill
					for fill in pending_fills {
						match self.check_fill_status(&fill).await {
							Ok(Some(updated_status)) => {
								if updated_status != fill.status {
									info!(
										"Fill {} status changed from {:?} to {:?}",
										fill.fill_id, fill.status, updated_status
									);

									// Create updated fill event
									let mut updated_fill = fill.clone();
									updated_fill.status = updated_status;

									// Send updated event
									if let Err(e) = self.event_tx.send(Event::OrderFill(updated_fill)) {
										warn!("Failed to send updated fill event: {}", e);
									}
								}
							}
							Ok(None) => {
								// No update yet, continue monitoring
							}
							Err(e) => {
								warn!("Error checking fill status for {}: {}", fill.fill_id, e);
							}
						}
					}
				}
				_ = shutdown_rx.recv() => {
					info!("Fill monitor received shutdown signal");
					break;
				}
			}
		}

		Ok(())
	}

	async fn check_fill_status(&self, fill: &FillEvent) -> Result<Option<FillStatus>, CoreError> {
		// Use delivery service to check transaction status
		match self
			.delivery_service
			.get_transaction_status(&fill.tx_hash, fill.chain_id)
			.await
		{
			Ok(Some(response)) => {
				// Convert delivery status to fill status
				let fill_status = match response.status {
					DeliveryStatus::Submitted => FillStatus::Pending,
					DeliveryStatus::Pending => FillStatus::Pending,
					DeliveryStatus::Confirmed => FillStatus::Confirmed,
					DeliveryStatus::Failed => FillStatus::Failed("Delivery failed".to_string()),
					DeliveryStatus::Dropped => {
						FillStatus::Failed("Transaction dropped".to_string())
					}
					DeliveryStatus::Replaced => {
						FillStatus::Failed("Transaction replaced".to_string())
					}
				};
				Ok(Some(fill_status))
			}
			Ok(None) => {
				// No status update yet
				Ok(None)
			}
			Err(e) => Err(CoreError::Delivery(format!(
				"Failed to check transaction status: {}",
				e
			))),
		}
	}

	/// Get current health status
	pub async fn get_health(&self) -> HealthReport {
		// TODO: Implement health checks for each service
		// For now, assume all services are healthy if lifecycle is running
		let is_running = self.lifecycle_manager.is_running().await;

		let discovery_healthy = is_running;
		let delivery_healthy = is_running;
		let settlement_healthy = is_running;

		let state_healthy = is_running;
		let event_processor_healthy = is_running;

		let overall_status = if is_running {
			ServiceStatus::Healthy
		} else {
			ServiceStatus::Unhealthy
		};

		HealthReport {
			discovery_healthy,
			delivery_healthy,
			settlement_healthy,
			state_healthy,
			event_processor_healthy,
			overall_status,
		}
	}

	/// Gracefully shutdown the orchestrator
	pub async fn shutdown(&self) -> Result<(), CoreError> {
		info!("Shutting down orchestrator");

		// Signal shutdown
		self.lifecycle_manager.shutdown().await?;
		let _ = self.shutdown_tx.send(());

		// Event processing will stop when shutdown signal is received
		// TODO: Implement graceful shutdown in services
		// Services should listen to shutdown signal from lifecycle manager

		// Wait for all tasks
		let mut tasks = self.tasks.lock().await;
		tasks.shutdown().await;

		info!("Orchestrator shutdown complete");
		Ok(())
	}

	/// Update configuration
	pub async fn update_config(&self, new_config: SolverConfig) -> Result<(), CoreError> {
		info!("Updating configuration");

		// Update config
		*self.config.write().await = new_config.clone();

		// TODO: Implement config update notification to services
		// Services should re-initialize plugins with new config

		Ok(())
	}

	/// Get current lifecycle state
	pub async fn get_state(&self) -> LifecycleState {
		self.lifecycle_manager.get_state().await
	}

	/// Get event sender for services to send events
	pub fn get_event_sender(&self) -> EventSender {
		self.event_tx.clone()
	}
}

impl Clone for Orchestrator {
	fn clone(&self) -> Self {
		Self {
			config: self.config.clone(),
			discovery_service: self.discovery_service.clone(),
			delivery_service: self.delivery_service.clone(),
			settlement_service: self.settlement_service.clone(),
			state_service: self.state_service.clone(),
			event_tx: self.event_tx.clone(),
			event_rx: self.event_rx.clone(),
			lifecycle_manager: self.lifecycle_manager.clone(),
			shutdown_tx: self.shutdown_tx.clone(),
			tasks: self.tasks.clone(),
			pending_fills: self.pending_fills.clone(),
		}
	}
}

/// Builder for creating an Orchestrator instance
pub struct OrchestratorBuilder {
	config: Option<SolverConfig>,
	config_path: Option<String>,
}

impl OrchestratorBuilder {
	pub fn new() -> Self {
		Self {
			config: None,
			config_path: None,
		}
	}

	pub fn with_config(mut self, config: SolverConfig) -> Self {
		self.config = Some(config);
		self
	}

	pub fn with_config_file(mut self, path: impl Into<String>) -> Self {
		self.config_path = Some(path.into());
		self
	}

	pub async fn build(self) -> Result<Orchestrator, CoreError> {
		// Load configuration either from provided config or from file
		let config = if let Some(config) = self.config {
			config
		} else if let Some(config_path) = self.config_path {
			// Load from file using ConfigLoader
			ConfigLoader::new()
				.with_file(&config_path)
				.load()
				.await
				.map_err(|e| match e {
					ConfigError::FileNotFound(msg) => {
						CoreError::Configuration(format!("Config file not found: {}", msg))
					}
					ConfigError::ParseError(msg) => {
						CoreError::Configuration(format!("Config parse error: {}", msg))
					}
					ConfigError::ValidationError(msg) => {
						CoreError::Configuration(format!("Config validation error: {}", msg))
					}
					ConfigError::EnvVarNotFound(var) => {
						CoreError::Configuration(format!("Environment variable not found: {}", var))
					}
					ConfigError::IoError(e) => {
						CoreError::Configuration(format!("IO error reading config: {}", e))
					}
				})?
		} else {
			return Err(CoreError::Configuration(
				"No configuration or config file path provided".to_string(),
			));
		};

		let config = Arc::new(RwLock::new(config));

		// Create services with their respective configurations
		// Note: Services will handle their own plugin initialization based on config

		// Create unified event channel
		let (event_tx, event_rx) = mpsc::unbounded_channel::<Event>();

		// Create services with their respective configurations
		let config_ref = config.read().await;

		// Create StateService with builder pattern
		let mut state_builder = StateServiceBuilder::new().with_config(config_ref.state.clone());

		// Register state plugins from config
		for (name, plugin_config) in &config_ref.plugins.state {
			if plugin_config.enabled {
				let plugin = create_state_plugin(plugin_config)?;
				state_builder =
					state_builder.with_plugin(name.clone(), plugin, plugin_config.clone());
			}
		}

		let state_service = Arc::new(state_builder.build().await);

		// Create EventSink for DiscoveryService using the main event channel
		let event_sink = EventSink::new(event_tx.clone());

		// Create DiscoveryService with event sender
		// Read values from config with defaults
		let mut discovery_builder =
			DiscoveryServiceBuilder::new().with_config(config_ref.discovery.clone());

		// Register discovery plugins from config
		for (name, plugin_config) in &config_ref.plugins.discovery {
			if plugin_config.enabled {
				let plugin = create_discovery_plugin(plugin_config)?;
				discovery_builder =
					discovery_builder.with_plugin(name.clone(), plugin, plugin_config.clone());
			}
		}

		let discovery_service = Arc::new(discovery_builder.build(event_sink).await);

		let mut delivery_builder =
			DeliveryServiceBuilder::new().with_config(config_ref.delivery.clone());

		// Register delivery plugins from config
		for (name, plugin_config) in &config_ref.plugins.delivery {
			if plugin_config.enabled {
				let plugin = create_delivery_plugin(plugin_config)?;
				delivery_builder =
					delivery_builder.with_plugin(name.clone(), plugin, plugin_config.clone());
			}
		}

		// Register order processors with delivery service
		for (name, plugin_config) in &config_ref.plugins.order {
			if plugin_config.enabled {
				let processor = create_order_processor(plugin_config)?;
				delivery_builder = delivery_builder.with_order_processor(name.clone(), processor);
			}
		}

		let delivery_service = Arc::new(delivery_builder.build().await);

		// Create SettlementService
		// Read values from config with defaults
		let mut settlement_builder =
			SettlementServiceBuilder::new().with_config(config_ref.settlement.clone());

		// Register settlement plugins from config
		for (name, plugin_config) in &config_ref.plugins.settlement {
			if plugin_config.enabled {
				let plugin = create_settlement_plugin(plugin_config)?;
				settlement_builder =
					settlement_builder.with_plugin(name.clone(), plugin, plugin_config.clone());
			}
		}

		let settlement_service = Arc::new(settlement_builder.build().await);

		drop(config_ref); // Release the read lock

		// Create lifecycle manager
		let lifecycle_manager = Arc::new(LifecycleManager::new());

		// Create shutdown channel
		let (shutdown_tx, _) = broadcast::channel(16);

		Ok(Orchestrator {
			config,
			discovery_service,
			delivery_service,
			settlement_service,
			state_service,
			event_tx,
			event_rx: Arc::new(Mutex::new(event_rx)),
			lifecycle_manager,
			shutdown_tx,
			tasks: Arc::new(Mutex::new(JoinSet::new())),
			pending_fills: Arc::new(RwLock::new(HashMap::new())),
		})
	}
}

impl Default for OrchestratorBuilder {
	fn default() -> Self {
		Self::new()
	}
}
