//! # Orchestrator Engine
//!
//! The central orchestration engine that coordinates all solver operations.
//!
//! This module provides the main `Orchestrator` struct that manages the lifecycle
//! of the solver system, coordinates between different services (discovery, delivery,
//! settlement, state), and processes events throughout the order lifecycle.
//!
//! ## Key Components
//!
//! - **Orchestrator**: The main coordinator that manages all solver operations
//! - **OrchestratorBuilder**: Builder pattern for creating orchestrator instances
//! - **Event Processing**: Handles all events in the order lifecycle
//! - **Health Monitoring**: Monitors the health of all system components
//! - **Transaction Monitoring**: Tracks pending transactions and settlements

use crate::{
	error::CoreError,
	lifecycle::{LifecycleManager, LifecycleState},
	utils::truncate_hash,
};
use serde::{Deserialize, Serialize};
use solver_config::{ConfigError, ConfigLoader};
use solver_delivery::{DeliveryService, DeliveryServiceBuilder};
use solver_discovery::{DiscoveryService, DiscoveryServiceBuilder};
use solver_plugin::factory::global_plugin_factory;
use solver_settlement::{SettlementService, SettlementServiceBuilder};
use solver_state::{StateService, StateServiceBuilder};
use solver_types::plugins::*;
use solver_types::{Event, EventType, SettlementReadyEvent, *};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, Mutex, RwLock};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

/// Create a state plugin from configuration.
///
/// Creates and validates a state plugin instance based on the provided configuration.
/// The plugin is validated before being returned to ensure it meets the required interface.
///
/// # Arguments
/// * `config` - The plugin configuration specifying type and settings
///
/// # Returns
/// A configured and validated state plugin instance
///
/// # Errors
/// Returns `CoreError::Plugin` if plugin creation or validation fails
fn create_state_plugin(config: &PluginConfig) -> Result<Arc<dyn StatePlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_state_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create a delivery plugin from configuration.
///
/// Creates and validates a delivery plugin instance based on the provided configuration.
/// The plugin is validated before being returned to ensure it meets the required interface.
///
/// # Arguments
/// * `config` - The plugin configuration specifying type and settings
///
/// # Returns
/// A configured and validated delivery plugin instance
///
/// # Errors
/// Returns `CoreError::Plugin` if plugin creation or validation fails
fn create_delivery_plugin(config: &PluginConfig) -> Result<Box<dyn DeliveryPlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_delivery_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create a discovery plugin from configuration.
///
/// Creates and validates a discovery plugin instance based on the provided configuration.
/// The plugin is validated before being returned to ensure it meets the required interface.
///
/// # Arguments
/// * `config` - The plugin configuration specifying type and settings
///
/// # Returns
/// A configured and validated discovery plugin instance
///
/// # Errors
/// Returns `CoreError::Plugin` if plugin creation or validation fails
fn create_discovery_plugin(config: &PluginConfig) -> Result<Box<dyn DiscoveryPlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_discovery_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create a settlement plugin from configuration.
///
/// Creates and validates a settlement plugin instance based on the provided configuration.
/// The plugin is validated before being returned to ensure it meets the required interface.
///
/// # Arguments
/// * `config` - The plugin configuration specifying type and settings
///
/// # Returns
/// A configured and validated settlement plugin instance
///
/// # Errors
/// Returns `CoreError::Plugin` if plugin creation or validation fails
fn create_settlement_plugin(config: &PluginConfig) -> Result<Box<dyn SettlementPlugin>, CoreError> {
	let factory = global_plugin_factory();

	let plugin = factory
		.create_settlement_plugin(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	// Validate config before using the plugin
	BasePlugin::validate_config(plugin.as_ref(), config).map_err(CoreError::Plugin)?;

	Ok(plugin)
}

/// Create an order processor from configuration.
///
/// Creates an order processor instance based on the provided configuration.
/// Order processors handle the transformation of orders into executable transactions.
///
/// # Arguments
/// * `config` - The plugin configuration specifying type and settings
///
/// # Returns
/// A configured order processor instance
///
/// # Errors
/// Returns `CoreError::Plugin` if processor creation fails
fn create_order_processor(config: &PluginConfig) -> Result<Arc<dyn OrderProcessor>, CoreError> {
	let factory = global_plugin_factory();

	let processor = factory
		.create_order_processor(&config.plugin_type, config.clone())
		.map_err(CoreError::Plugin)?;

	Ok(processor)
}

/// Event sender type alias for clarity.
///
/// Used throughout the system to send events between components.
/// This unbounded sender allows components to emit events without blocking.
pub type EventSender = mpsc::UnboundedSender<Event>;

/// Health status report for all solver services.
///
/// Provides a comprehensive view of the health status of all major components
/// in the solver system, including individual service health and overall status.
#[derive(Debug, Clone)]
pub struct HealthReport {
	/// Health status of the discovery service
	pub discovery_healthy: bool,
	/// Health status of the delivery service
	pub delivery_healthy: bool,
	/// Health status of the settlement service
	pub settlement_healthy: bool,
	/// Health status of the state service
	pub state_healthy: bool,
	/// Health status of the event processing system
	pub event_processor_healthy: bool,
	/// Overall system health status
	pub overall_status: ServiceStatus,
}

/// Order information with current status and settlement details.
///
/// Contains the complete order data along with its current processing status
/// and any associated settlement information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderInfo {
	/// The original order event data
	pub order: OrderEvent,
	/// Current processing status of the order
	pub status: String,
	/// Settlement ID if the order has been settled
	pub settlement_id: Option<String>,
}

/// Core orchestrator that coordinates all solver components.
///
/// The orchestrator is the central coordinator of the solver system. It manages
/// the lifecycle of all services, processes events, monitors transactions,
/// and maintains the overall state of the system.
///
/// ## Architecture
///
/// The orchestrator follows an event-driven architecture where:
/// - Discovery services emit order discovery events
/// - Order events trigger delivery processing
/// - Successful deliveries trigger settlement monitoring
/// - All state changes are tracked and persisted
///
/// ## Event Flow
///
/// 1. Discovery plugins find new orders and emit `DiscoveryEvent`
/// 2. Orders are converted to `OrderEvent` and processed
/// 3. Delivery service processes orders and emits `FillEvent`
/// 4. Settlement service monitors fills and emits `SettlementEvent`
/// 5. All events are persisted in the state service
pub struct Orchestrator {
	/// System configuration with plugin settings
	config: Arc<RwLock<SolverConfig>>,

	/// Core service instances
	discovery_service: Arc<DiscoveryService>,
	delivery_service: Arc<DeliveryService>,
	settlement_service: Arc<SettlementService>,
	state_service: Arc<StateService>,

	/// Event coordination channels owned by the orchestrator
	event_tx: EventSender,
	event_rx: Arc<Mutex<mpsc::UnboundedReceiver<Event>>>,

	/// Manages orchestrator lifecycle and shutdown coordination
	lifecycle_manager: Arc<LifecycleManager>,

	/// Broadcast channel for shutdown coordination
	shutdown_tx: broadcast::Sender<()>,

	/// Background task management
	tasks: Arc<Mutex<JoinSet<Result<(), CoreError>>>>,

	/// Pending fills being monitored for confirmation
	pending_fills: Arc<RwLock<HashMap<String, FillEvent>>>,

	/// Pending settlement transactions being monitored
	pending_settlements: Arc<RwLock<HashMap<String, SettlementEvent>>>,
}

impl Orchestrator {
	/// Start the orchestrator and all services.
	///
	/// Initializes and starts all services in the correct order, begins event processing,
	/// and sets up monitoring systems. This is the main entry point for starting
	/// the solver system.
	///
	/// # Service Startup Order
	///
	/// 1. State service initialization
	/// 2. Discovery service startup
	/// 3. Delivery service preparation
	/// 4. Settlement service monitoring
	/// 5. Event processing loop
	/// 6. Health monitoring
	/// 7. Transaction monitoring
	///
	/// # Returns
	/// Returns `Ok(())` if all services start successfully
	///
	/// # Errors
	/// Returns `CoreError` if any service fails to start or initialize
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

		info!("State service started successfully");
		Ok(())
	}

	async fn start_discovery_service(&self) -> Result<(), CoreError> {
		info!("Starting discovery service");

		// Start all registered discovery plugins
		self.discovery_service.start_all().await.map_err(|e| {
			CoreError::ServiceInit(format!("Failed to start discovery service: {}", e))
		})?;

		info!("Discovery service started successfully");
		Ok(())
	}

	async fn start_delivery_service(&self) -> Result<(), CoreError> {
		info!("Starting delivery service");

		// Delivery service doesn't have a specific start method
		// It's ready to use once created with plugins
		// TODO: Add health check to verify all configured plugins are ready

		info!("Delivery service started successfully");
		Ok(())
	}

	async fn start_settlement_service(&self) -> Result<(), CoreError> {
		info!("Starting settlement service");

		// Start the settlement monitoring loop
		self.settlement_service.start_monitoring().await;

		info!("Settlement service started successfully");
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

		let mut join_set = JoinSet::new();

		loop {
			tokio::select! {
				Some(event) = event_rx.recv() => {
					let orchestrator = self.clone();
					join_set.spawn(async move {
						if let Err(e) = orchestrator.handle_event(event).await {
							warn!("Error handling event: {}", e);
						}
					});
				}
				_ = shutdown_rx.recv() => {
					info!("Event processor received shutdown signal");
					break;
				}
			}
		}

		// Await all in-flight event handler tasks before returning
		while let Some(res) = join_set.join_next().await {
			if let Err(e) = res {
				warn!("Event handler task failed: {}", e);
			}
		}

		Ok(())
	}

	/// Handle a single event
	// State management helper functions
	async fn store_event(
		&self,
		event_type: &str,
		event_id: &str,
		event_data: &impl serde::Serialize,
		ttl_days: u64,
	) -> Result<(), CoreError> {
		let event_key = format!("events:{}:{}", event_type, event_id);
		let data = serde_json::to_vec(event_data)
			.map_err(|e| CoreError::Serialization(format!("Failed to serialize event: {}", e)))?;

		self.state_service
			.set_with_ttl(
				&event_key,
				data.into(),
				std::time::Duration::from_secs(ttl_days * 24 * 60 * 60),
			)
			.await
			.map_err(|e| CoreError::State(format!("Failed to store event: {}", e)))?;

		Ok(())
	}

	async fn store_order(
		&self,
		order_id: &str,
		order_data: &impl serde::Serialize,
	) -> Result<(), CoreError> {
		let order_key = format!("orders:{}", order_id);
		let data = serde_json::to_vec(order_data)
			.map_err(|e| CoreError::Serialization(format!("Failed to serialize order: {}", e)))?;

		self.state_service
			.set(&order_key, data.into())
			.await
			.map_err(|e| CoreError::State(format!("Failed to store order: {}", e)))?;

		Ok(())
	}

	async fn update_order_status(&self, order_id: &str, status: &str) -> Result<(), CoreError> {
		let status_key = format!("orders:status:{}", order_id);
		self.state_service
			.set(&status_key, status.as_bytes().to_vec().into())
			.await
			.map_err(|e| CoreError::State(format!("Failed to update order status: {}", e)))?;

		// Apply TTL based on status
		match status {
			"failed" | "cancelled" => {
				// Apply 24 hour TTL to failed/cancelled orders
				let order_key = format!("orders:{}", order_id);
				if let Ok(Some(order_data)) = self.state_service.get(&order_key).await {
					self.state_service
						.set_with_ttl(
							&order_key,
							order_data,
							std::time::Duration::from_secs(24 * 60 * 60),
						)
						.await
						.ok();
				}
			}
			"filled" => {
				// Apply 7 day TTL to completed orders
				let order_key = format!("orders:{}", order_id);
				if let Ok(Some(order_data)) = self.state_service.get(&order_key).await {
					self.state_service
						.set_with_ttl(
							&order_key,
							order_data,
							std::time::Duration::from_secs(7 * 24 * 60 * 60),
						)
						.await
						.ok();
				}
			}
			_ => {} // No TTL for active orders
		}

		Ok(())
	}

	async fn store_settlement(
		&self,
		settlement_id: &str,
		order_id: &str,
		settlement_data: &impl serde::Serialize,
	) -> Result<(), CoreError> {
		// Store settlement data
		let settlement_key = format!("settlements:{}", settlement_id);
		let data = serde_json::to_vec(settlement_data).map_err(|e| {
			CoreError::Serialization(format!("Failed to serialize settlement: {}", e))
		})?;

		self.state_service
			.set(&settlement_key, data.into())
			.await
			.map_err(|e| CoreError::State(format!("Failed to store settlement: {}", e)))?;

		// Map order to settlement
		let order_settlement_key = format!("settlements:by_order:{}", order_id);
		self.state_service
			.set(
				&order_settlement_key,
				settlement_id.as_bytes().to_vec().into(),
			)
			.await
			.ok();

		Ok(())
	}

	async fn handle_event(&self, event: Event) -> Result<(), CoreError> {
		match event {
			Event::Discovery(discovery_event) => self.handle_discovery_event(discovery_event).await,
			Event::OrderCreated(order_event) => self.handle_order_created_event(order_event).await,
			Event::OrderFill(fill_event) => self.handle_order_fill_event(fill_event).await,
			Event::SettlementReady(ready_event) => {
				self.handle_settlement_ready_event(ready_event).await
			}
			Event::Settlement(settlement_event) => {
				self.handle_settlement_event(settlement_event).await
			}
			Event::ServiceStatus(status_event) => {
				self.handle_service_status_event(status_event).await
			}
		}
	}

	async fn handle_discovery_event(&self, event: DiscoveryEvent) -> Result<(), CoreError> {
		debug!("Processing discovery event: {}", truncate_hash(&event.id));

		// Store event in state with 30 day TTL
		self.store_event("discovery", &event.id, &event, 30).await?;

		debug!("Stored discovery event {} in state", event.id);

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
		info!("Order created: {}", truncate_hash(&event.order_id));

		// Store order and set initial status
		self.store_order(&event.order_id, &event).await?;
		self.update_order_status(&event.order_id, "discovered")
			.await?;

		debug!("Stored order {} with status 'discovered'", event.order_id);

		// Use delivery service to process the order
		match self
			.delivery_service
			.process_order_to_transaction(&event)
			.await
		{
			Ok(Some(transaction_request)) => {
				info!(
					"Creating transaction request for order {}",
					truncate_hash(&event.order_id)
				);

				// Update order status to processing
				self.update_order_status(&event.order_id, "processing")
					.await?;

				// Submit the transaction request
				match self
					.delivery_service
					.execute_transaction(transaction_request)
					.await
				{
					Ok(response) => {
						info!(
							"Order {} delivered: tx_hash={}",
							truncate_hash(&event.order_id),
							truncate_hash(&response.tx_hash)
						);

						// Update order status based on delivery response
						let status = match response.status {
							DeliveryStatus::Confirmed => "filled",
							DeliveryStatus::Failed
							| DeliveryStatus::Dropped
							| DeliveryStatus::Replaced => "failed",
							_ => "processing", // Keep as processing for pending states
						};

						self.update_order_status(&event.order_id, status).await?;

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
						// Update order status to failed
						self.update_order_status(&event.order_id, "failed")
							.await
							.ok();

						warn!(
							"Failed to deliver order {}: {}",
							truncate_hash(&event.order_id),
							e
						);
					}
				}
			}
			Ok(None) => {
				warn!(
					"No delivery request created for order {}",
					truncate_hash(&event.order_id)
				);
				// Update status to indicate no action taken
				self.update_order_status(&event.order_id, "no_action")
					.await
					.ok();
			}
			Err(e) => {
				warn!(
					"Failed to process order {}: {}",
					truncate_hash(&event.order_id),
					e
				);
				// Update order status to failed
				self.update_order_status(&event.order_id, "failed")
					.await
					.ok();
			}
		}

		Ok(())
	}

	async fn handle_order_fill_event(&self, event: FillEvent) -> Result<(), CoreError> {
		match event.status {
			FillStatus::Pending => {
				// Store pending fill for monitoring
				info!(
					"Fill {} is pending, adding to monitoring queue",
					truncate_hash(&event.fill_id)
				);
				self.pending_fills
					.write()
					.await
					.insert(event.fill_id.clone(), event);
			}
			FillStatus::Confirmed => {
				info!(
					"Fill {} confirmed for order {}",
					truncate_hash(&event.fill_id),
					truncate_hash(&event.order_id)
				);

				// Remove from pending if it was there
				self.pending_fills.write().await.remove(&event.fill_id);

				// Send to settlement service for monitoring
				if let Err(e) = self.settlement_service.monitor_fill(event.clone()).await {
					warn!("Failed to start monitoring fill for settlement: {}", e);
				}
			}
			FillStatus::Failed(_) => {
				// Remove from pending if it was there
				self.pending_fills.write().await.remove(&event.fill_id);

				warn!(
					"Fill {} failed for order {}, not processing settlement",
					truncate_hash(&event.fill_id),
					truncate_hash(&event.order_id)
				);
			}
		}

		Ok(())
	}

	async fn handle_settlement_ready_event(
		&self,
		event: SettlementReadyEvent,
	) -> Result<(), CoreError> {
		info!(
			"Processing settlement ready event for order {} (fill: {})",
			truncate_hash(&event.fill_event.order_id),
			truncate_hash(&event.fill_event.fill_id)
		);

		// Use delivery service to process the fill
		match self
			.delivery_service
			.process_fill_to_transaction(&event.fill_event)
			.await
		{
			Ok(Some(transaction_request)) => {
				info!(
					"Creating settlement transaction for order {}",
					truncate_hash(&event.fill_event.order_id)
				);
				// Submit the transaction request
				match self
					.delivery_service
					.execute_transaction(transaction_request.clone())
					.await
				{
					Ok(response) => {
						info!(
							"Settlement transaction submitted for order {} with tx hash: {}",
							truncate_hash(&event.fill_event.order_id),
							&response.tx_hash
						);

						// Create and send Settlement event to track the execution
						let settlement_event = SettlementEvent {
							order_id: event.fill_event.order_id.clone(),
							settlement_id: format!(
								"{}:{}",
								event.fill_event.order_id, event.fill_event.fill_id
							),
							source_chain: transaction_request.transaction.chain_id, // Origin chain
							destination_chain: event.fill_event.chain_id, // chain where fill happened (destination chain)
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

						// Add to pending settlements if status is pending
						if matches!(settlement_event.status, SettlementStatus::Pending) {
							self.pending_settlements
								.write()
								.await
								.insert(settlement_event.tx_hash.clone(), settlement_event.clone());
						}

						if let Err(e) = self.event_tx.send(Event::Settlement(settlement_event)) {
							warn!("Failed to send Settlement event: {}", e);
						}
					}
					Err(e) => {
						warn!(
							"Failed to execute settlement transaction for order {}: {}",
							truncate_hash(&event.fill_event.order_id),
							e
						);
					}
				}
			}
			Ok(None) => {
				warn!(
					"No settlement transaction created for order {}",
					truncate_hash(&event.fill_event.order_id)
				);
			}
			Err(e) => {
				warn!(
					"Failed to process settlement for order {}: {}",
					truncate_hash(&event.fill_event.order_id),
					e
				);
			}
		}

		Ok(())
	}

	async fn handle_settlement_event(&self, event: SettlementEvent) -> Result<(), CoreError> {
		// Store settlement data
		self.store_settlement(&event.settlement_id, &event.order_id, &event)
			.await?;

		// Update order status based on settlement status
		let order_status = match event.status {
			SettlementStatus::Confirmed => "settled",
			SettlementStatus::Failed => "settlement_failed",
			SettlementStatus::Challenged => "challenged",
			SettlementStatus::Expired => "expired",
			_ => "settling",
		};

		self.update_order_status(&event.order_id, order_status)
			.await?;

		// Only log significant status changes
		match event.status {
			SettlementStatus::Confirmed => {
				info!(
					"Settlement {} confirmed for order {}",
					truncate_hash(&event.settlement_id),
					truncate_hash(&event.order_id)
				);
			}
			SettlementStatus::Failed => {
				warn!(
					"Settlement {} failed for order {}",
					truncate_hash(&event.settlement_id),
					truncate_hash(&event.order_id)
				);
			}
			SettlementStatus::Challenged => {
				warn!(
					"Settlement {} challenged for order {}",
					truncate_hash(&event.settlement_id),
					truncate_hash(&event.order_id)
				);
			}
			SettlementStatus::Expired => {
				warn!(
					"Settlement {} expired for order {}",
					truncate_hash(&event.settlement_id),
					truncate_hash(&event.order_id)
				);
			}
			SettlementStatus::Pending => {
				// Add to pending settlements for monitoring
				self.pending_settlements
					.write()
					.await
					.insert(event.tx_hash.clone(), event.clone());

				debug!(
					"Settlement {} pending for order {}",
					truncate_hash(&event.settlement_id),
					truncate_hash(&event.order_id)
				);
			}
			SettlementStatus::Cancelled => {
				info!(
					"Settlement {} cancelled for order {}",
					truncate_hash(&event.settlement_id),
					truncate_hash(&event.order_id)
				);
			}
		}

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

					// Get all pending settlements
					let pending_settlements: Vec<SettlementEvent> = {
						let settlements = self.pending_settlements.read().await;
						settlements.values().cloned().collect()
					};

					if !pending_fills.is_empty() || !pending_settlements.is_empty() {
						debug!("Monitoring {} pending fills and {} pending settlements",
							pending_fills.len(), pending_settlements.len());
					}

					// Check status of each pending fill
					for fill in pending_fills {
						match self.check_fill_status(&fill).await {
							Ok(Some(updated_status)) => {
								if updated_status != fill.status {
									info!(
										"Fill {} status changed from {:?} to {:?}",
										truncate_hash(&fill.fill_id), fill.status, updated_status
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
								warn!("Error checking fill status for {}: {}", truncate_hash(&fill.fill_id), e);
							}
						}
					}

					// Check status of each pending settlement
					for settlement in pending_settlements {
						match self.check_settlement_status(&settlement).await {
							Ok(Some(updated_status)) => {
								if updated_status != settlement.status {
									info!(
										"Settlement {} status changed from {:?} to {:?}",
										truncate_hash(&settlement.tx_hash), settlement.status, updated_status
									);

									// Remove from pending if confirmed or failed
									if matches!(updated_status, SettlementStatus::Confirmed | SettlementStatus::Failed) {
										self.pending_settlements.write().await.remove(&settlement.tx_hash);
									}

									// Create updated settlement event
									let mut updated_settlement = settlement.clone();
									updated_settlement.status = updated_status;

									// Send updated event
									if let Err(e) = self.event_tx.send(Event::Settlement(updated_settlement)) {
										warn!("Failed to send updated settlement event: {}", e);
									}
								}
							}
							Ok(None) => {
								// No update yet, continue monitoring
							}
							Err(e) => {
								warn!("Error checking settlement status for {}: {}", truncate_hash(&settlement.tx_hash), e);
							}
						}
					}
				}
				_ = shutdown_rx.recv() => {
					info!("Transaction monitor received shutdown signal");
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

	async fn check_settlement_status(
		&self,
		settlement: &SettlementEvent,
	) -> Result<Option<SettlementStatus>, CoreError> {
		// Use delivery service to check transaction status
		match self
			.delivery_service
			.get_transaction_status(&settlement.tx_hash, settlement.source_chain)
			.await
		{
			Ok(Some(response)) => {
				// Convert delivery status to settlement status
				let settlement_status = match response.status {
					DeliveryStatus::Submitted => SettlementStatus::Pending,
					DeliveryStatus::Pending => SettlementStatus::Pending,
					DeliveryStatus::Confirmed => SettlementStatus::Confirmed,
					DeliveryStatus::Failed => SettlementStatus::Failed,
					DeliveryStatus::Dropped => SettlementStatus::Failed,
					DeliveryStatus::Replaced => SettlementStatus::Failed,
				};
				Ok(Some(settlement_status))
			}
			Ok(None) => {
				// No status update yet
				Ok(None)
			}
			Err(e) => Err(CoreError::Delivery(format!(
				"Failed to check settlement transaction status: {}",
				e
			))),
		}
	}

	/// Get current health status of all system components.
	///
	/// Performs health checks on all services and returns a comprehensive
	/// health report. Currently returns basic status based on lifecycle state,
	/// but can be extended to perform detailed health checks.
	///
	/// # Returns
	/// A `HealthReport` containing the health status of all components
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

	/// Gracefully shutdown the orchestrator and all services.
	///
	/// Initiates a graceful shutdown sequence by signaling all services to stop,
	/// waiting for background tasks to complete, and cleaning up resources.
	///
	/// # Shutdown Process
	///
	/// 1. Signal lifecycle manager to begin shutdown
	/// 2. Send shutdown signal to all background tasks
	/// 3. Wait for all tasks to complete gracefully
	/// 4. Clean up resources
	///
	/// # Returns
	/// Returns `Ok(())` when shutdown is complete
	///
	/// # Errors
	/// Returns `CoreError` if shutdown process encounters errors
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

	/// Get order by ID
	pub async fn get_order(&self, order_id: &str) -> Result<Option<OrderInfo>, CoreError> {
		// Get order data
		let order_key = format!("orders:{}", order_id);
		let order_data = self
			.state_service
			.get(&order_key)
			.await
			.map_err(|e| CoreError::State(format!("Failed to retrieve order: {}", e)))?;

		if let Some(data) = order_data {
			// Deserialize order
			let order: OrderEvent = serde_json::from_slice(&data).map_err(|e| {
				CoreError::Serialization(format!("Failed to deserialize order: {}", e))
			})?;

			// Get order status
			let status_key = format!("orders:status:{}", order_id);
			let status_data =
				self.state_service.get(&status_key).await.map_err(|e| {
					CoreError::State(format!("Failed to retrieve order status: {}", e))
				})?;

			let status = status_data
				.and_then(|data| String::from_utf8(data.to_vec()).ok())
				.unwrap_or_else(|| "unknown".to_string());

			// Get settlement if exists
			let settlement_key = format!("settlements:by_order:{}", order_id);
			let settlement_id = self
				.state_service
				.get(&settlement_key)
				.await
				.ok()
				.flatten()
				.and_then(|data| String::from_utf8(data.to_vec()).ok());

			Ok(Some(OrderInfo {
				order,
				status,
				settlement_id,
			}))
		} else {
			Ok(None)
		}
	}

	/// Get settlement by ID
	pub async fn get_settlement(
		&self,
		settlement_id: &str,
	) -> Result<Option<SettlementEvent>, CoreError> {
		let settlement_key = format!("settlements:{}", settlement_id);
		let settlement_data = self
			.state_service
			.get(&settlement_key)
			.await
			.map_err(|e| CoreError::State(format!("Failed to retrieve settlement: {}", e)))?;

		if let Some(data) = settlement_data {
			let settlement: SettlementEvent = serde_json::from_slice(&data).map_err(|e| {
				CoreError::Serialization(format!("Failed to deserialize settlement: {}", e))
			})?;
			Ok(Some(settlement))
		} else {
			Ok(None)
		}
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
			pending_settlements: self.pending_settlements.clone(),
		}
	}
}

/// Builder for creating an Orchestrator instance.
///
/// Provides a fluent interface for configuring and building an orchestrator.
/// Supports loading configuration from either a direct config object or
/// from a configuration file.
pub struct OrchestratorBuilder {
	/// Direct configuration object
	config: Option<SolverConfig>,
	/// Path to configuration file
	config_path: Option<String>,
}

impl OrchestratorBuilder {
	/// Create a new orchestrator builder.
	pub fn new() -> Self {
		Self {
			config: None,
			config_path: None,
		}
	}

	/// Set the configuration directly.
	///
	/// # Arguments
	/// * `config` - The solver configuration to use
	pub fn with_config(mut self, config: SolverConfig) -> Self {
		self.config = Some(config);
		self
	}

	/// Set the path to a configuration file.
	///
	/// The configuration will be loaded from this file during build.
	///
	/// # Arguments
	/// * `path` - Path to the configuration file
	pub fn with_config_file(mut self, path: impl Into<String>) -> Self {
		self.config_path = Some(path.into());
		self
	}

	/// Build the orchestrator instance.
	///
	/// Creates and configures all services, plugins, and sets up the event system.
	/// This method loads configuration (either from direct config or file),
	/// creates all service instances with their configured plugins, and
	/// establishes the event coordination system.
	///
	/// # Returns
	/// A fully configured `Orchestrator` instance ready to start
	///
	/// # Errors
	/// Returns `CoreError` if:
	/// - No configuration is provided
	/// - Configuration file cannot be loaded or parsed
	/// - Plugin creation fails
	/// - Service initialization fails
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

		// Create SettlementService with event sink
		// Read values from config with defaults
		let mut settlement_builder = SettlementServiceBuilder::new()
			.with_config(config_ref.settlement.clone())
			.with_event_sink(EventSink::new(event_tx.clone()));

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
			pending_settlements: Arc::new(RwLock::new(HashMap::new())),
		})
	}
}

impl Default for OrchestratorBuilder {
	fn default() -> Self {
		Self::new()
	}
}
