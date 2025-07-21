// solver-core/src/engine.rs

use crate::{
	error::CoreError,
	lifecycle::{LifecycleManager, LifecycleState},
};
use solver_config::{ConfigError, ConfigLoader};
use solver_delivery::{DeliveryService, DeliveryServiceBuilder};
use solver_discovery::{DiscoveryService, DiscoveryServiceBuilder};
use solver_plugin::factory::global_plugin_factory;
use solver_state::{StateService, StateServiceBuilder};
use solver_types::plugins::*;
use solver_types::{Event, EventType, *};
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

/// Create a delivery plugin from configuration
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

		// Start event processing
		self.start_event_processing().await?;

		// Start health monitoring
		self.start_health_monitor().await?;

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
			Event::OrderCreated(order_event) => self.handle_order_created(order_event).await,
			Event::OrderFilled(fill_event) => self.handle_order_filled(fill_event).await,
			Event::SettlementComplete(settlement_event) => {
				self.handle_settlement_complete(settlement_event).await
			}
			Event::ServiceStatus(status_event) => self.handle_service_status(status_event).await,
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

	async fn handle_order_created(&self, event: OrderEvent) -> Result<(), CoreError> {
		info!("Processing order created: {}", event.order_id);

		// Use delivery service to process the order
		match self.delivery_service.process_order(&event).await {
			Ok(Some(delivery_request)) => {
				info!("Creating delivery request for order {}", event.order_id);
				// Submit the delivery request
				match self.delivery_service.deliver(delivery_request).await {
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
						};

						if let Err(e) = self.event_tx.send(Event::OrderFilled(fill_event)) {
							warn!("Failed to send OrderFilled event: {}", e);
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

	async fn handle_order_filled(&self, event: FillEvent) -> Result<(), CoreError> {
		info!(
			"Processing order filled: {} (fill: {})",
			event.order_id, event.fill_id
		);

		// TODO: Implement fill status updates in state service

		Ok(())
	}

	async fn handle_settlement_complete(&self, event: SettlementEvent) -> Result<(), CoreError> {
		info!(
			"Processing settlement complete: {} (settlement: {})",
			event.order_id, event.settlement_id
		);

		// TODO: Implement settlement status updates in state service

		Ok(())
	}

	async fn handle_service_status(&self, event: StatusEvent) -> Result<(), CoreError> {
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

	/// Get current health status
	pub async fn get_health(&self) -> HealthReport {
		// TODO: Implement health checks for each service
		// For now, assume all services are healthy if lifecycle is running
		let is_running = self.lifecycle_manager.is_running().await;

		let discovery_healthy = is_running;
		let delivery_healthy = is_running;
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
			state_service: self.state_service.clone(),
			event_tx: self.event_tx.clone(),
			event_rx: self.event_rx.clone(),
			lifecycle_manager: self.lifecycle_manager.clone(),
			shutdown_tx: self.shutdown_tx.clone(),
			tasks: self.tasks.clone(),
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

		drop(config_ref); // Release the read lock

		// Create lifecycle manager
		let lifecycle_manager = Arc::new(LifecycleManager::new());

		// Create shutdown channel
		let (shutdown_tx, _) = broadcast::channel(16);

		Ok(Orchestrator {
			config,
			discovery_service,
			delivery_service,
			state_service,
			event_tx,
			event_rx: Arc::new(Mutex::new(event_rx)),
			lifecycle_manager,
			shutdown_tx,
			tasks: Arc::new(Mutex::new(JoinSet::new())),
		})
	}
}

impl Default for OrchestratorBuilder {
	fn default() -> Self {
		Self::new()
	}
}
