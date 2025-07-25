use alloy_primitives::U256;
use solver_account::AccountService;
use solver_config::Config;
use solver_delivery::DeliveryService;
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

pub mod event_bus;

#[derive(Debug, Error)]
pub enum SolverError {
	#[error("Configuration error: {0}")]
	Config(String),
	#[error("Service error: {0}")]
	Service(String),
}

pub struct SolverEngine {
	config: Config,
	storage: Arc<StorageService>,
	delivery: Arc<DeliveryService>,
	discovery: Arc<DiscoveryService>,
	order: Arc<OrderService>,
	settlement: Arc<SettlementService>,
	event_bus: EventBus,
}

static CLAIM_BATCH: usize = 1;

impl SolverEngine {
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
					log::info!("Shutting down solver");
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

		// Store fill transaction
		self.storage
			.store("fills", &order.id, &tx_hash)
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		// Store reverse mapping: tx_hash -> order_id
		self.storage
			.store("tx_to_order", &format!("{:?}", tx_hash), &order.id)
			.await
			.map_err(|e| SolverError::Service(e.to_string()))?;

		Ok(())
	}

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
			let poll_interval = tokio::time::Duration::from_secs(30); // Poll every 30 seconds

			let start_time = tokio::time::Instant::now();

			loop {
				// Check if we've exceeded the timeout
				if start_time.elapsed() > monitoring_timeout {
					log::warn!(
						"Transaction monitoring timeout for {} after {} minutes",
						order_id,
						timeout_minutes
					);
					break;
				}

				// Try to get transaction status
				match delivery.get_status(&tx_hash).await {
					Ok(true) => {
						// Transaction is confirmed and successful
						// Get the full receipt for the event
						if let Ok(receipt) = delivery.confirm_with_default(&tx_hash).await {
							event_bus
								.publish(SolverEvent::Delivery(
									DeliveryEvent::TransactionConfirmed {
										tx_hash: tx_hash.clone(),
										receipt,
										tx_type,
									},
								))
								.ok();

							log::info!(
								"Transaction confirmed for order {}: {:?}",
								order_id,
								tx_type
							);
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

						log::error!("Transaction failed for order {}: {:?}", order_id, tx_type);
						break;
					}
					Err(e) => {
						// Transaction not yet confirmed or error
						log::debug!(
							"Transaction not yet confirmed for order {}: {}",
							order_id,
							e
						);
					}
				}

				// Wait before next poll
				tokio::time::sleep(poll_interval).await;
			}
		});

		Ok(())
	}

	async fn handle_transaction_confirmed(
		&self,
		tx_hash: solver_types::TransactionHash,
		receipt: solver_types::TransactionReceipt,
		tx_type: TransactionType,
	) -> Result<(), SolverError> {
		if !receipt.success {
			log::error!("Transaction failed: {:?}", tx_hash);
			self.event_bus
				.publish(SolverEvent::Delivery(DeliveryEvent::TransactionFailed {
					tx_hash,
					error: "Transaction reverted".to_string(),
				}))
				.ok();
			return Ok(());
		}

		log::info!(
			"Transaction confirmed: {:?} at block {} (type: {:?})",
			tx_hash,
			receipt.block_number,
			tx_type
		);

		// Handle based on transaction type
		match tx_type {
			TransactionType::Fill => {
				// For fill transactions, start settlement monitoring
				self.handle_fill_confirmed(tx_hash, receipt).await?;
			}
			TransactionType::Claim => {
				// For claim transactions, mark order as completed
				self.handle_claim_confirmed(tx_hash, receipt).await?;
			}
		}

		Ok(())
	}

	async fn handle_fill_confirmed(
		&self,
		tx_hash: solver_types::TransactionHash,
		receipt: solver_types::TransactionReceipt,
	) -> Result<(), SolverError> {
		// Look up the order ID from the transaction hash
		let order_id = match self
			.storage
			.retrieve::<String>("tx_to_order", &format!("{:?}", tx_hash))
			.await
		{
			Ok(id) => id,
			Err(e) => {
				log::error!("Failed to find order for fill tx {:?}: {}", tx_hash, e);
				return Ok(()); // Don't fail the handler, just log and continue
			}
		};

		// Retrieve the order
		let order = match self.storage.retrieve::<Order>("orders", &order_id).await {
			Ok(order) => order,
			Err(e) => {
				log::error!("Failed to retrieve order {}: {}", order_id, e);
				return Ok(());
			}
		};

		log::info!(
			"Fill transaction confirmed for order {}: {:?} at block {}",
			order_id,
			tx_hash,
			receipt.block_number
		);

		// Validate the fill and extract proof immediately (synchronously)
		let fill_proof = match self.settlement.validate_fill(&order, &tx_hash).await {
			Ok(proof) => {
				log::info!("Fill validated for order {}", order_id);
				proof
			}
			Err(e) => {
				log::error!("Failed to validate fill for order {}: {:?}", order_id, e);
				return Ok(());
			}
		};

		// Store the fill proof
		if let Err(e) = self
			.storage
			.store("fill_proofs", &order.id, &fill_proof)
			.await
		{
			log::error!("Failed to store fill proof for order {}: {}", order_id, e);
			return Ok(());
		}

		// Now spawn a task to monitor claim readiness
		let settlement = self.settlement.clone();
		let event_bus = self.event_bus.clone();
		let order_clone = order.clone();
		let timeout_minutes = self.config.solver.monitoring_timeout_minutes;

		tokio::spawn(async move {
			let monitoring_timeout = tokio::time::Duration::from_secs(timeout_minutes * 60);
			let check_interval = tokio::time::Duration::from_secs(60); // Check every minute
			let start_time = tokio::time::Instant::now();

			loop {
				// Check if we've exceeded the timeout
				if start_time.elapsed() > monitoring_timeout {
					log::warn!(
						"Claim readiness monitoring timeout for order {} after {} minutes",
						order_id,
						timeout_minutes
					);
					break;
				}

				// Check if we can claim
				if settlement.can_claim(&order_clone, &fill_proof).await {
					event_bus
						.publish(SolverEvent::Settlement(SettlementEvent::ClaimReady {
							order_id: order_clone.id,
						}))
						.ok();
					log::info!("Order {} is ready to claim", order_id);
					break;
				}

				// Wait before next check
				tokio::time::sleep(check_interval).await;
			}
		});

		Ok(())
	}

	async fn handle_claim_confirmed(
		&self,
		tx_hash: solver_types::TransactionHash,
		receipt: solver_types::TransactionReceipt,
	) -> Result<(), SolverError> {
		// Look up the order ID from the transaction hash
		let order_id = match self
			.storage
			.retrieve::<String>("tx_to_order", &format!("{:?}", tx_hash))
			.await
		{
			Ok(id) => id,
			Err(e) => {
				log::error!("Failed to find order for claim tx {:?}: {}", tx_hash, e);
				return Ok(());
			}
		};

		log::info!(
			"Claim transaction confirmed for order {}: {:?} at block {}",
			order_id,
			tx_hash,
			receipt.block_number
		);

		// Emit completed event
		self.event_bus
			.publish(SolverEvent::Settlement(SettlementEvent::Completed {
				order_id: order_id.clone(),
			}))
			.ok();

		// Optional: Clean up storage for completed orders
		log::info!("Order {} settlement completed successfully", order_id);

		Ok(())
	}

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
				.store("tx_to_order", &format!("{:?}", claim_tx_hash), &order.id)
				.await
				.map_err(|e| SolverError::Service(e.to_string()))?;
		}
		Ok(())
	}

	async fn build_execution_context(&self) -> Result<ExecutionContext, SolverError> {
		// In production, would fetch real data
		Ok(ExecutionContext {
			gas_price: U256::from(20_000_000_000u64), // 20 gwei
			timestamp: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			solver_balance: HashMap::new(),
		})
	}

	pub fn event_bus(&self) -> &EventBus {
		&self.event_bus
	}

	pub fn config(&self) -> &Config {
		&self.config
	}
}

// Type aliases for factory functions
type StorageFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_storage::StorageInterface> + Send>;
type AccountFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_account::AccountInterface> + Send>;
type DeliveryFactory =
	Box<dyn Fn(&toml::Value) -> Box<dyn solver_delivery::DeliveryInterface> + Send>;
type DiscoveryFactory =
	Box<dyn Fn(&toml::Value) -> Box<dyn solver_discovery::DiscoveryInterface> + Send>;
type OrderFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_order::OrderInterface> + Send>;
type SettlementFactory =
	Box<dyn Fn(&toml::Value) -> Box<dyn solver_settlement::SettlementInterface> + Send>;
type StrategyFactory = Box<dyn Fn(&toml::Value) -> Box<dyn solver_order::ExecutionStrategy> + Send>;

// Factory pattern for creating services from config
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

	pub fn with_storage_factory<F>(mut self, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_storage::StorageInterface> + Send + 'static,
	{
		self.storage_factory = Some(Box::new(factory));
		self
	}

	pub fn with_account_factory<F>(mut self, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_account::AccountInterface> + Send + 'static,
	{
		self.account_factory = Some(Box::new(factory));
		self
	}

	pub fn with_delivery_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_delivery::DeliveryInterface> + Send + 'static,
	{
		self.delivery_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	pub fn with_discovery_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_discovery::DiscoveryInterface> + Send + 'static,
	{
		self.discovery_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	pub fn with_order_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_order::OrderInterface> + Send + 'static,
	{
		self.order_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	pub fn with_settlement_factory<F>(mut self, name: &str, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_settlement::SettlementInterface> + Send + 'static,
	{
		self.settlement_factories
			.insert(name.to_string(), Box::new(factory));
		self
	}

	pub fn with_strategy_factory<F>(mut self, factory: F) -> Self
	where
		F: Fn(&toml::Value) -> Box<dyn solver_order::ExecutionStrategy> + Send + 'static,
	{
		self.strategy_factory = Some(Box::new(factory));
		self
	}

	pub fn build(self) -> Result<SolverEngine, SolverError> {
		// Create storage backend
		let storage_backend = self
			.storage_factory
			.ok_or_else(|| SolverError::Config("Storage factory not provided".into()))?(
			&self.config.storage.config,
		);
		let storage = Arc::new(StorageService::new(storage_backend));

		// Create account provider
		let account_provider = self
			.account_factory
			.ok_or_else(|| SolverError::Config("Account factory not provided".into()))?(
			&self.config.account.config,
		);
		let account = Arc::new(AccountService::new(account_provider));

		// Create delivery providers
		let mut delivery_providers = Vec::new();
		for (name, config) in &self.config.delivery.providers {
			if let Some(factory) = self.delivery_factories.get(name) {
				delivery_providers.push(factory(config));
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
			self.config.delivery.confirmations,
		));

		// Create discovery sources
		let mut discovery_sources = Vec::new();
		for (name, config) in &self.config.discovery.sources {
			if let Some(factory) = self.discovery_factories.get(name) {
				discovery_sources.push(factory(config));
			}
		}

		let discovery = Arc::new(DiscoveryService::new(discovery_sources));

		// Create order implementations
		let mut order_impls = HashMap::new();
		for (name, config) in &self.config.order.implementations {
			if let Some(factory) = self.order_factories.get(name) {
				order_impls.insert(name.clone(), factory(config));
			}
		}

		// Create execution strategy
		let strategy = self
			.strategy_factory
			.ok_or_else(|| SolverError::Config("Strategy factory not provided".into()))?(
			&self.config.order.execution_strategy.config,
		);

		let order = Arc::new(OrderService::new(order_impls, strategy));

		// Create settlement implementations
		let mut settlement_impls = HashMap::new();
		for (name, config) in &self.config.settlement.implementations {
			if let Some(factory) = self.settlement_factories.get(name) {
				settlement_impls.insert(name.clone(), factory(config));
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
