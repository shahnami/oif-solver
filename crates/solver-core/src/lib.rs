//! Core solver engine for the OIF solver system.
//!
//! This module provides the main orchestration logic for the solver, coordinating
//! between all the various services (discovery, order processing, delivery, settlement)
//! to execute the complete order lifecycle. It includes the event-driven architecture
//! and factory pattern for building solver instances.

use alloy_primitives::{hex, U256};
use solver_account::AccountService;
use solver_config::Config;
use solver_delivery::{DeliveryError, DeliveryService};
use solver_discovery::DiscoveryService;
use solver_order::OrderService;
use solver_settlement::SettlementService;
use solver_storage::StorageService;
use solver_types::{
	DeliveryEvent, DiscoveryEvent, EventBus, ExecutionContext, ExecutionDecision, Intent, Order,
	OrderEvent, SettlementEvent, SolverEvent, TransactionType,
};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::instrument;

pub mod event_bus;

/// Utility function to truncate a hex string for display purposes.
///
/// Shows only the first 8 characters followed by ".." for longer strings.
fn truncate_id(id: &str) -> String {
	if id.len() <= 8 {
		id.to_string()
	} else {
		format!("{}..", &id[..8])
	}
}

/// Errors that can occur during solver operations.
#[derive(Debug, Error)]
pub enum SolverError {
	/// Error related to configuration issues.
	#[error("Configuration error: {0}")]
	Config(String),
	/// Error from one of the solver services.
	#[error("Service error: {0}")]
	Service(String),
}

/// Main solver engine that orchestrates the order execution lifecycle.
///
/// The SolverEngine coordinates between multiple services:
/// - Discovery: Finds new orders to process
/// - Order: Validates and executes orders
/// - Delivery: Submits transactions to the blockchain
/// - Settlement: Monitors and claims settled orders
/// - Storage: Persists state and order information
pub struct SolverEngine {
	/// Solver configuration.
	config: Config,
	/// Storage service for persisting state.
	storage: Arc<StorageService>,
	/// Delivery service for blockchain transactions.
	delivery: Arc<DeliveryService>,
	/// Discovery service for finding new orders.
	discovery: Arc<DiscoveryService>,
	/// Order service for validation and execution.
	order: Arc<OrderService>,
	/// Settlement service for monitoring and claiming.
	settlement: Arc<SettlementService>,
	/// Event bus for inter-service communication.
	event_bus: EventBus,
}

/// Number of orders to batch together for claim operations.
static CLAIM_BATCH: usize = 1;

impl SolverEngine {
	/// Main execution loop for the solver engine.
	///
	/// This method:
	/// 1. Starts discovery monitoring to find new intents
	/// 2. Subscribes to the event bus for inter-service communication
	/// 3. Processes discovered intents and system events
	/// 4. Handles graceful shutdown on Ctrl+C
	pub async fn run(&self) -> Result<(), SolverError> {
		// Start discovery monitoring
		let (intent_tx, mut intent_rx) = mpsc::unbounded_channel();
		self.discovery
			.start_all(intent_tx)
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		// Subscribe to events
		let mut event_receiver = self.event_bus.subscribe();

		// Batch claim processing
		let mut claim_batch = Vec::new();
		loop {
			tokio::select! {
				// Handle discovered intents
				Some(intent) = intent_rx.recv() => {
					tracing::info!(
						order_id = %truncate_id(&intent.id),
						"Discovered intent"
					);
					self.handle_intent(intent).await?;
				}

				// Handle events
				Ok(event) = event_receiver.recv() => {
					match event {
						SolverEvent::Order(OrderEvent::Executing { order, params }) => {
							self.handle_order_execution(order, params).await?;
						}

						SolverEvent::Delivery(DeliveryEvent::TransactionPending { order_id, tx_hash, tx_type }) => {
							self.handle_transaction_pending(order_id, tx_hash, tx_type).await?;
						}

						SolverEvent::Delivery(DeliveryEvent::TransactionConfirmed { tx_hash, receipt, tx_type }) => {
							self.handle_transaction_confirmed(tx_hash, receipt, tx_type).await?;
						}

						SolverEvent::Settlement(SettlementEvent::ClaimReady { order_id }) => {
							claim_batch.push(order_id);
							if claim_batch.len() >= CLAIM_BATCH {
								self.process_claim_batch(&mut claim_batch).await?;
							}
						}

						_ => {}
					}
				}

				// Shutdown signal
				_ = tokio::signal::ctrl_c() => {
					break;
				}
			}
		}

		// Cleanup
		self.discovery
			.stop_all()
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		Ok(())
	}

	/// Handles a newly discovered intent.
	///
	/// This method:
	/// 1. Validates the intent to create an order
	/// 2. Stores the validated order
	/// 3. Checks the execution strategy to determine if/when to execute
	/// 4. Publishes appropriate events based on the execution decision
	#[instrument(skip_all, fields(order_id = %truncate_id(&intent.id)))]
	async fn handle_intent(&self, intent: Intent) -> Result<(), SolverError> {
		// Validate intent
		match self.order.validate_intent(&intent).await {
			Ok(order) => {
				self.event_bus
					.publish(SolverEvent::Discovery(DiscoveryEvent::IntentValidated {
						intent_id: intent.id.clone(),
						order: order.clone(),
					}))
					.ok();

				// Store order
				self.storage
					.store("orders", &order.id, &order)
					.await
					.map_err(|e| SolverError::Service(e.to_string()))?;

				// Check execution strategy
				let context = self.build_execution_context().await?;
				match self.order.should_execute(&order, &context).await {
					ExecutionDecision::Execute(params) => {
						tracing::info!("Executing order");
						self.event_bus
							.publish(SolverEvent::Order(OrderEvent::Executing { order, params }))
							.ok();
					}
					ExecutionDecision::Skip(reason) => {
						self.event_bus
							.publish(SolverEvent::Order(OrderEvent::Skipped {
								order_id: order.id,
								reason,
							}))
							.ok();
					}
					ExecutionDecision::Defer(duration) => {
						self.event_bus
							.publish(SolverEvent::Order(OrderEvent::Deferred {
								order_id: order.id,
								retry_after: duration,
							}))
							.ok();
					}
				}
			}
			Err(e) => {
				self.event_bus
					.publish(SolverEvent::Discovery(DiscoveryEvent::IntentRejected {
						intent_id: intent.id,
						reason: e.to_string(),
					}))
					.ok();
			}
		}

		Ok(())
	}

	/// Handles order execution by generating and submitting a fill transaction.
	///
	/// This method:
	/// 1. Generates a fill transaction for the order
	/// 2. Submits the transaction through the delivery service
	/// 3. Stores transaction hashes and mappings for later retrieval
	#[instrument(skip_all, fields(order_id = %truncate_id(&order.id)))]
	async fn handle_order_execution(
		&self,
		order: Order,
		params: solver_types::ExecutionParams,
	) -> Result<(), SolverError> {
		// Generate fill transaction
		let tx = self
			.order
			.generate_fill_transaction(&order, &params)
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		// Submit transaction
		let tx_hash = self
			.delivery
			.deliver(tx)
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		self.event_bus
			.publish(SolverEvent::Delivery(DeliveryEvent::TransactionPending {
				order_id: order.id.clone(),
				tx_hash: tx_hash.clone(),
				tx_type: TransactionType::Fill,
			}))
			.ok();

		// Store fill transaction and timestamp
		self.storage
			.store("fills", &order.id, &tx_hash)
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		// Store reverse mapping: tx_hash -> order_id
		self.storage
			.store("tx_to_order", &hex::encode(&tx_hash.0), &order.id)
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		Ok(())
	}

	/// Monitors a pending transaction until it is confirmed or fails.
	///
	/// Spawns an async task that polls the transaction status at regular intervals
	/// until the transaction is confirmed, fails, or the monitoring timeout is reached.
	#[instrument(skip_all, fields(order_id = %truncate_id(&order_id), tx_hash = %truncate_id(&hex::encode(&tx_hash.0))))]
	async fn handle_transaction_pending(
		&self,
		order_id: String,
		tx_hash: solver_types::TransactionHash,
		tx_type: TransactionType,
	) -> Result<(), SolverError> {
		// Spawn a task to monitor the transaction
		let delivery = self.delivery.clone();
		let event_bus = self.event_bus.clone();
		let timeout_minutes = self.config.solver.monitoring_timeout_minutes;

		tokio::spawn(async move {
			let monitoring_timeout = tokio::time::Duration::from_secs(timeout_minutes * 60);
			let poll_interval = tokio::time::Duration::from_secs(3); // Poll every 3 seconds for faster confirmation

			let start_time = tokio::time::Instant::now();

			loop {
				// Check if we've exceeded the timeout
				if start_time.elapsed() > monitoring_timeout {
					tracing::warn!(
						order_id = %truncate_id(&order_id),
						tx_hash = %truncate_id(&hex::encode(&tx_hash.0)),
						tx_type = ?tx_type,
						"Transaction monitoring timeout reached after {} minutes",
						timeout_minutes
					);
					break;
				}

				// Try to get transaction status
				match delivery.get_status(&tx_hash).await {
					Ok(true) => {
						// Transaction is confirmed and successful
						// Get the full receipt for the event
						match delivery.confirm_with_default(&tx_hash).await {
							Ok(receipt) => {
								tracing::info!(
									order_id = %truncate_id(&order_id),
									tx_hash = %truncate_id(&hex::encode(&tx_hash.0)),
									"Confirmed {}",
									match tx_type {
										TransactionType::Fill => "fill",
										TransactionType::Claim => "claim",
									}
								);
								event_bus
									.publish(SolverEvent::Delivery(
										DeliveryEvent::TransactionConfirmed {
											tx_hash: tx_hash.clone(),
											receipt,
											tx_type,
										},
									))
									.ok();
							}
							Err(e) => {
								tracing::error!(
									order_id = %truncate_id(&order_id),
									tx_hash = %truncate_id(&hex::encode(&tx_hash.0)),
									tx_type = ?tx_type,
									error = %e,
									"Failed to wait for confirmations"
								);
							}
						}
						break;
					}
					Ok(false) => {
						// Transaction failed
						event_bus
							.publish(SolverEvent::Delivery(DeliveryEvent::TransactionFailed {
								tx_hash: tx_hash.clone(),
								error: "Transaction reverted".to_string(),
							}))
							.ok();
						break;
					}
					Err(e) => {
						// Transaction not yet confirmed or error
						// Show user-friendly message for common cases
						let message = match e {
							DeliveryError::NoProviderAvailable => {
								"Waiting for transaction to be mined"
							}
							_ => "Checking transaction status",
						};

						// Always log at info level so users see progress
						tracing::info!(
							order_id = %truncate_id(&order_id),
							tx_hash = %truncate_id(&hex::encode(&tx_hash.0)),
							tx_type = ?tx_type,
							elapsed_secs = start_time.elapsed().as_secs(),
							"{}",
							message
						);
					}
				}

				tokio::time::sleep(poll_interval).await;
			}
		});

		Ok(())
	}

	/// Handles confirmed transactions based on their type.
	///
	/// Routes handling to specific methods based on whether this is a fill
	/// or claim transaction.
	#[instrument(skip_all, fields(tx_hash = %truncate_id(&hex::encode(&tx_hash.0))))]
	async fn handle_transaction_confirmed(
		&self,
		tx_hash: solver_types::TransactionHash,
		_receipt: solver_types::TransactionReceipt,
		tx_type: TransactionType,
	) -> Result<(), SolverError> {
		if !_receipt.success {
			self.event_bus
				.publish(SolverEvent::Delivery(DeliveryEvent::TransactionFailed {
					tx_hash,
					error: "Transaction reverted".to_string(),
				}))
				.ok();
			return Ok(());
		}

		// Handle based on transaction type
		match tx_type {
			TransactionType::Fill => {
				// For fill transactions, start settlement monitoring
				self.handle_fill_confirmed(tx_hash, _receipt).await?;
			}
			TransactionType::Claim => {
				// For claim transactions, mark order as completed
				self.handle_claim_confirmed(tx_hash, _receipt).await?;
			}
		}

		Ok(())
	}

	/// Handles confirmed fill transactions.
	///
	/// This method:
	/// 1. Looks up the order associated with the transaction
	/// 2. Spawns a task to validate the fill and monitor claim readiness
	async fn handle_fill_confirmed(
		&self,
		tx_hash: solver_types::TransactionHash,
		_receipt: solver_types::TransactionReceipt,
	) -> Result<(), SolverError> {
		// Look up the order ID from the transaction hash
		let order_id = match self
			.storage
			.retrieve::<String>("tx_to_order", &hex::encode(&tx_hash.0))
			.await
		{
			Ok(id) => id,
			Err(_) => {
				return Ok(()); // Don't fail the handler, just continue
			}
		};

		// Retrieve the order
		let order = match self.storage.retrieve::<Order>("orders", &order_id).await {
			Ok(order) => order,
			Err(_) => {
				return Ok(());
			}
		};

		// Spawn a task to validate fill and monitor claim readiness
		let settlement = self.settlement.clone();
		let storage = self.storage.clone();
		let event_bus = self.event_bus.clone();
		let timeout_minutes = self.config.solver.monitoring_timeout_minutes;

		tokio::spawn(async move {
			// Validate the fill and extract proof
			let fill_proof = match settlement.validate_fill(&order, &tx_hash).await {
				Ok(proof) => proof,
				Err(e) => {
					tracing::error!(
						order_id = %truncate_id(&order_id),
						error = %e,
						"Failed to validate fill"
					);
					return;
				}
			};

			// Store the fill proof
			if storage
				.store("fill_proofs", &order.id, &fill_proof)
				.await
				.is_err()
			{
				return;
			}

			// Monitor claim readiness
			let monitoring_timeout = tokio::time::Duration::from_secs(timeout_minutes * 60);
			let check_interval = tokio::time::Duration::from_secs(1); // Check every 1 second for faster claim detection
			let start_time = tokio::time::Instant::now();

			loop {
				// Check if we've exceeded the timeout
				if start_time.elapsed() > monitoring_timeout {
					tracing::warn!(
						order_id = %truncate_id(&order_id),
						"Claim readiness monitoring timeout reached after {} minutes",
						timeout_minutes
					);
					break;
				}

				// Check if we can claim
				if settlement.can_claim(&order, &fill_proof).await {
					tracing::info!(
						order_id = %truncate_id(&order_id),
						"Ready to claim"
					);
					event_bus
						.publish(SolverEvent::Settlement(SettlementEvent::ClaimReady {
							order_id: order.id,
						}))
						.ok();
					break;
				}

				// Wait before next check
				tokio::time::sleep(check_interval).await;
			}
		});

		Ok(())
	}

	/// Handles confirmed claim transactions.
	///
	/// Marks the order as completed and publishes the completion event.
	async fn handle_claim_confirmed(
		&self,
		tx_hash: solver_types::TransactionHash,
		_receipt: solver_types::TransactionReceipt,
	) -> Result<(), SolverError> {
		// Look up the order ID from the transaction hash
		let order_id = match self
			.storage
			.retrieve::<String>("tx_to_order", &hex::encode(&tx_hash.0))
			.await
		{
			Ok(id) => id,
			Err(_) => {
				return Ok(());
			}
		};

		// Emit completed event
		tracing::info!(
			order_id = %truncate_id(&order_id),
			"Completed"
		);
		self.event_bus
			.publish(SolverEvent::Settlement(SettlementEvent::Completed {
				order_id: order_id.clone(),
			}))
			.ok();

		// Optional: Clean up storage for completed orders

		Ok(())
	}

	/// Processes a batch of orders ready for claiming.
	///
	/// For each order in the batch:
	/// 1. Retrieves the order and fill proof from storage
	/// 2. Generates a claim transaction
	/// 3. Submits the claim transaction
	/// 4. Stores transaction hashes and mappings
	#[instrument(skip_all)]
	async fn process_claim_batch(&self, batch: &mut Vec<String>) -> Result<(), SolverError> {
		for order_id in batch.drain(..) {
			// Retrieve order
			let order: Order = self
				.storage
				.retrieve("orders", &order_id)
				.await
				.map_err(|e| SolverError::Service(e.to_string()))?;

			// Retrieve fill proof (already validated when ClaimReady was emitted)
			let fill_proof: solver_types::FillProof = self
				.storage
				.retrieve("fill_proofs", &order_id)
				.await
				.map_err(|e| SolverError::Service(e.to_string()))?;

			// Generate claim transaction
			let claim_tx = self
				.order
				.generate_claim_transaction(&order, &fill_proof)
				.await
				.map_err(|e| SolverError::Service(e.to_string()))?;

			// Submit claim transaction through delivery service
			let claim_tx_hash = self
				.delivery
				.deliver(claim_tx)
				.await
				.map_err(|e| SolverError::Service(e.to_string()))?;

			self.event_bus
				.publish(SolverEvent::Delivery(DeliveryEvent::TransactionPending {
					order_id: order.id.clone(),
					tx_hash: claim_tx_hash.clone(),
					tx_type: TransactionType::Claim,
				}))
				.ok();

			// Store claim transaction hash
			self.storage
				.store("claims", &order.id, &claim_tx_hash)
				.await
				.map_err(|e| SolverError::Service(e.to_string()))?;

			// Store reverse mapping: tx_hash -> order_id
			self.storage
				.store("tx_to_order", &hex::encode(&claim_tx_hash.0), &order.id)
				.await
				.map_err(|e| SolverError::Service(e.to_string()))?;
		}
		Ok(())
	}

	/// Builds the execution context for strategy decisions.
	///
	/// TODO: this should fetch real-time data such as gas prices,
	/// solver balances, and other relevant market conditions.
	async fn build_execution_context(&self) -> Result<ExecutionContext, SolverError> {
		Ok(ExecutionContext {
			gas_price: U256::from(20_000_000_000u64), // 20 gwei
			timestamp: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			solver_balance: HashMap::new(),
		})
	}

	/// Returns a reference to the event bus.
	pub fn event_bus(&self) -> &EventBus {
		&self.event_bus
	}

	/// Returns a reference to the configuration.
	pub fn config(&self) -> &Config {
		&self.config
	}
}

/// Type alias for storage backend factory function.
type StorageFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_storage::StorageInterface> + Send>;
/// Type alias for account provider factory function.
type AccountFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_account::AccountInterface> + Send>;
/// Type alias for delivery provider factory function.
type DeliveryFactory =
	Box<dyn Fn(&toml::Value) -> Box<dyn solver_delivery::DeliveryInterface> + Send>;
/// Type alias for discovery source factory function.
type DiscoveryFactory =
	Box<dyn Fn(&toml::Value) -> Box<dyn solver_discovery::DiscoveryInterface> + Send>;
/// Type alias for order implementation factory function.
type OrderFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_order::OrderInterface> + Send>;
/// Type alias for settlement implementation factory function.
type SettlementFactory =
	Box<dyn Fn(&toml::Value) -> Box<dyn solver_settlement::SettlementInterface> + Send>;
/// Type alias for execution strategy factory function.
type StrategyFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_order::ExecutionStrategy> + Send>;

/// Builder for constructing a SolverEngine with pluggable implementations.
///
/// The SolverBuilder uses the factory pattern to allow different implementations
/// of each service to be plugged in based on configuration. This enables
/// flexibility in supporting different blockchains, order types, and strategies.
pub struct SolverBuilder {
	config: Config,
	storage_factory: Option<StorageFactory>,
	account_factory: Option<AccountFactory>,
	delivery_factories: HashMap<String, DeliveryFactory>,
	discovery_factories: HashMap<String, DiscoveryFactory>,
	order_factories: HashMap<String, OrderFactory>,
	settlement_factories: HashMap<String, SettlementFactory>,
	strategy_factory: Option<StrategyFactory>,
}

impl SolverBuilder {
	/// Creates a new SolverBuilder with the given configuration.
	pub fn new(config: Config) -> Self {
		Self {
			config,
			storage_factory: None,
			account_factory: None,
			delivery_factories: HashMap::new(),
			discovery_factories: HashMap::new(),
			order_factories: HashMap::new(),
			settlement_factories: HashMap::new(),
			strategy_factory: None,
		}
	}

	/// Sets the factory function for creating storage backends.
	pub fn with_storage_factory<F>(mut self, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_storage::StorageInterface> + Send + 'static,
	{
		self.storage_factory = Some(Box::new(factory));
		self
	}

	/// Sets the factory function for creating account providers.
	pub fn with_account_factory<F>(mut self, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_account::AccountInterface> + Send + 'static,
	{
		self.account_factory = Some(Box::new(factory));
		self
	}

	/// Adds a factory function for creating delivery providers.
	///
	/// The name parameter should match the provider name in the configuration.
	pub fn with_delivery_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_delivery::DeliveryInterface> + Send + 'static,
	{
		self.delivery_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	/// Adds a factory function for creating discovery sources.
	///
	/// The name parameter should match the source name in the configuration.
	pub fn with_discovery_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_discovery::DiscoveryInterface> + Send + 'static,
	{
		self.discovery_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	/// Adds a factory function for creating order implementations.
	///
	/// The name parameter should match the implementation name in the configuration.
	pub fn with_order_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_order::OrderInterface> + Send + 'static,
	{
		self.order_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	/// Adds a factory function for creating settlement implementations.
	///
	/// The name parameter should match the implementation name in the configuration.
	pub fn with_settlement_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_settlement::SettlementInterface> + Send + 'static,
	{
		self.settlement_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	/// Sets the factory function for creating execution strategies.
	pub fn with_strategy_factory<F>(mut self, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_order::ExecutionStrategy> + Send + 'static,
	{
		self.strategy_factory = Some(Box::new(factory));
		self
	}

	/// Builds the SolverEngine using the configured factories.
	///
	/// This method:
	/// 1. Creates all service instances using the provided factories
	/// 2. Validates that all required services are configured
	/// 3. Wires up the services with proper dependencies
	/// 4. Returns a fully configured SolverEngine ready to run
	pub fn build(self) -> Result<SolverEngine, SolverError> {
		// Create storage backend
		let storage_backend = self
			.storage_factory
			.ok_or_else(|| SolverError::Config("Storage factory not provided".into()))?(
			&self.config.storage.config,
		);
		let storage = Arc::new(StorageService::new(storage_backend));
		tracing::info!(component = "storage", implementation = %self.config.storage.backend, "Loaded");

		// Create account provider
		let account_provider = self
			.account_factory
			.ok_or_else(|| SolverError::Config("Account factory not provided".into()))?(
			&self.config.account.config,
		);
		let account = Arc::new(AccountService::new(account_provider));
		tracing::info!(component = "account", implementation = %self.config.account.provider, "Loaded");

		// Create delivery providers
		let mut delivery_providers = HashMap::new();
		for (name, config) in &self.config.delivery.providers {
			if let Some(factory) = self.delivery_factories.get(name) {
				// Extract chain_id from the config
				let chain_id = config
					.get("chain_id")
					.and_then(|v| v.as_integer())
					.ok_or_else(|| {
						SolverError::Config(format!(
							"chain_id missing for delivery provider {}",
							name
						))
					})? as u64;

				let provider = factory(config);

				// Validate the configuration using the provider's schema
				provider.config_schema().validate(config).map_err(|e| {
					SolverError::Config(format!(
						"Invalid configuration for delivery provider '{}': {}",
						name, e
					))
				})?;

				delivery_providers.insert(chain_id, provider);
				tracing::info!(component = "delivery", implementation = %name, chain_id = %chain_id, "Loaded");
			}
		}

		if delivery_providers.is_empty() {
			return Err(SolverError::Config(
				"No delivery providers configured".into(),
			));
		}

		let delivery = Arc::new(DeliveryService::new(
			delivery_providers,
			account.clone(),
			self.config.delivery.min_confirmations,
		));

		// Create discovery sources
		let mut discovery_sources = Vec::new();
		for (name, config) in &self.config.discovery.sources {
			if let Some(factory) = self.discovery_factories.get(name) {
				let source = factory(config);

				// Validate the configuration using the source's schema
				source.config_schema().validate(config).map_err(|e| {
					SolverError::Config(format!(
						"Invalid configuration for discovery source '{}': {}",
						name, e
					))
				})?;

				discovery_sources.push(source);
				tracing::info!(component = "discovery", implementation = %name, "Loaded");
			}
		}

		let discovery = Arc::new(DiscoveryService::new(discovery_sources));

		// Create order implementations
		let mut order_impls = HashMap::new();
		for (name, config) in &self.config.order.implementations {
			if let Some(factory) = self.order_factories.get(name) {
				let implementation = factory(config);

				// Validate the configuration using the implementation's schema
				implementation
					.config_schema()
					.validate(config)
					.map_err(|e| {
						SolverError::Config(format!(
							"Invalid configuration for order implementation '{}': {}",
							name, e
						))
					})?;

				order_impls.insert(name.clone(), implementation);
				tracing::info!(component = "order", implementation = %name, "Loaded");
			}
		}

		// Create execution strategy
		let strategy = self
			.strategy_factory
			.ok_or_else(|| SolverError::Config("Strategy factory not provided".into()))?(
			&self.config.order.execution_strategy.config,
		);
		tracing::info!(component = "strategy", implementation = %self.config.order.execution_strategy.strategy_type, "Loaded");

		let order = Arc::new(OrderService::new(order_impls, strategy));

		// Create settlement implementations
		let mut settlement_impls = HashMap::new();
		for (name, config) in &self.config.settlement.implementations {
			if let Some(factory) = self.settlement_factories.get(name) {
				let implementation = factory(config);

				// Validate the configuration using the implementation's schema
				implementation
					.config_schema()
					.validate(config)
					.map_err(|e| {
						SolverError::Config(format!(
							"Invalid configuration for settlement '{}': {}",
							name, e
						))
					})?;

				settlement_impls.insert(name.clone(), implementation);
				tracing::info!(component = "settlement", implementation = %name, "Loaded");
			}
		}

		let settlement = Arc::new(SettlementService::new(settlement_impls));

		Ok(SolverEngine {
			config: self.config,
			storage,
			delivery,
			discovery,
			order,
			settlement,
			event_bus: EventBus::new(1000),
		})
	}
}
