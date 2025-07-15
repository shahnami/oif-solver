//! Coordinates between different solver components.

use anyhow::Result;
use ethers::signers::Signer;
use futures::StreamExt;
use solver_chains::ChainRegistry;
use solver_config::types::SolverConfig;
use solver_delivery::{DeliveryConfig, RpcDelivery};
use solver_discovery::{
	monitor::MonitorConfig,
	sources::{OnChainConfig, OnChainSource},
	IntentDiscovery,
};
use solver_orders::OrderRegistry;
use solver_settlement::{SettlementConfig as SettlementManagerConfig, SettlementManager};
use solver_state::{StateConfig, StateManager, StorageBackend};
use solver_types::{chains::ChainId, errors::SolverError, orders::OrderStatus};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tracing::{error, info, instrument, warn};

use crate::SolverEngine;
use solver_types::Order;

/// Type alias for the process handle
type ProcessHandle =
	Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<Result<(), anyhow::Error>>>>>;

/// Coordinates discovery, validation, execution, and settlement
pub struct SolverCoordinator {
	config: SolverConfig,
	engine: Arc<SolverEngine>,
	discovery: Arc<IntentDiscovery>,
	state_manager: Arc<StateManager>,
	settlement_manager: Arc<SettlementManager>,
	order_registry: Arc<OrderRegistry>,
	#[allow(dead_code)]
	chain_registry: Arc<ChainRegistry>,
	process_handle: ProcessHandle,
}

impl SolverCoordinator {
	/// Create new solver coordinator
	#[instrument(skip(config))]
	pub async fn new(config: SolverConfig) -> Result<Self> {
		info!("Initializing solver components");

		// 1. Initialize chain registry
		let chain_registry = Self::init_chains(&config).await?;

		// 2. Initialize delivery service
		let delivery = Self::init_delivery(&config, chain_registry.clone())?;

		// 3. Initialize state manager
		let state_manager = Self::init_state(&config).await?;

		// 4. Initialize order registry
		let order_registry = Arc::new(OrderRegistry::new());

		// 5. Initialize discovery
		let discovery =
			Self::init_discovery(&config, chain_registry.clone(), order_registry.clone())?;

		// 6. Initialize settlement manager
		let settlement_manager = Self::init_settlement(
			&config,
			state_manager.clone(),
			order_registry.clone(),
			chain_registry.clone(),
			delivery.clone(),
		)
		.await?;

		// 7. Initialize solver engine
		// Parse solver private key to get address
		let solver_key = config.solver.private_key.trim_start_matches("0x");
		let wallet = solver_key
			.parse::<ethers::signers::LocalWallet>()
			.map_err(|e| SolverError::Config(format!("Invalid private key: {}", e)))?;
		let solver_address = ethers::types::Address::from(wallet.address());

		let engine = Arc::new(SolverEngine::new(
			chain_registry.clone(),
			delivery,
			settlement_manager.clone(),
			state_manager.clone(),
			solver_address,
			config.strategy.profitability.min_profit_bps,
		));

		info!("Solver coordinator initialized successfully");

		Ok(Self {
			config,
			engine,
			discovery,
			state_manager,
			settlement_manager,
			order_registry,
			chain_registry,
			process_handle: Arc::new(tokio::sync::Mutex::new(None)),
		})
	}

	/// Initialize chain adapters
	#[instrument(skip(config))]
	async fn init_chains(config: &SolverConfig) -> Result<Arc<ChainRegistry>> {
		let mut rpc_endpoints = HashMap::new();
		let mut confirmations = HashMap::new();

		for (chain_id, chain_config) in &config.chains {
			info!("Configuring chain {}: {}", chain_id, chain_config.name);
			rpc_endpoints.insert(*chain_id, chain_config.rpc_url.clone());
			confirmations.insert(*chain_id, chain_config.confirmations);
		}

		// Parse solver private key to get wallet for signing
		let solver_key = config.solver.private_key.trim_start_matches("0x");
		let wallet = solver_key
			.parse::<ethers::signers::LocalWallet>()
			.map_err(|e| SolverError::Config(format!("Invalid private key: {}", e)))?;
		let solver_address = ethers::types::Address::from(wallet.address());

		// Get max_retries from delivery configuration
		let delivery_config = config
			.delivery
			.services
			.get(&config.delivery.default_service)
			.ok_or_else(|| {
				SolverError::Config("Default delivery service not configured".to_string())
			})?;

		// Create registry with signing support
		let registry = ChainRegistry::with_signing_support(
			rpc_endpoints,
			confirmations,
			Some(wallet),
			solver_chains::implementations::evm::GasStrategy::Standard,
			solver_address,
			delivery_config.max_retries,
		)
		.await?;
		Ok(Arc::new(registry))
	}

	/// Initialize delivery service
	#[instrument(skip(config, chain_registry))]
	fn init_delivery(
		config: &SolverConfig,
		chain_registry: Arc<ChainRegistry>,
	) -> Result<Arc<solver_delivery::DeliveryServiceImpl>> {
		let delivery_config = config
			.delivery
			.services
			.get(&config.delivery.default_service)
			.ok_or_else(|| {
				SolverError::Config("Default delivery service not configured".to_string())
			})?;

		// Get default confirmations from chain configs (use minimum across all chains)
		let default_confirmations = config
			.chains
			.values()
			.map(|chain| chain.confirmations)
			.min()
			.unwrap_or(1);

		// Parse solver private key to get address for delivery config
		let solver_key = config.solver.private_key.trim_start_matches("0x");
		let wallet = solver_key
			.parse::<ethers::signers::LocalWallet>()
			.map_err(|e| SolverError::Config(format!("Invalid private key: {}", e)))?;
		let solver_address = ethers::types::Address::from(wallet.address());

		// Convert to solver-delivery config
		let service_config = DeliveryConfig {
			endpoints: delivery_config.endpoints.clone(),
			api_key: delivery_config.api_key.clone(),
			gas_strategy: match &delivery_config.gas_strategy {
				solver_config::types::GasStrategyConfig::Standard => {
					solver_delivery::GasStrategy::Standard
				}
				solver_config::types::GasStrategyConfig::Fast => solver_delivery::GasStrategy::Fast,
				solver_config::types::GasStrategyConfig::Custom { multiplier } => {
					solver_delivery::GasStrategy::Custom {
						multiplier: *multiplier,
					}
				}
				solver_config::types::GasStrategyConfig::Eip1559 { max_priority_fee } => {
					solver_delivery::GasStrategy::Eip1559 {
						max_priority_fee: *max_priority_fee,
					}
				}
			},
			max_retries: delivery_config.max_retries,
			confirmations: default_confirmations,
			from_address: solver_address,
		};

		// Match on the service type
		let delivery_impl = match config.delivery.default_service.as_str() {
			"rpc" | "alchemy" => solver_delivery::DeliveryServiceImpl::Rpc(RpcDelivery::new(
				service_config,
				chain_registry,
			)),
			service_type => {
				return Err(SolverError::Config(format!(
					"Unsupported delivery service type: {}",
					service_type
				))
				.into())
			}
		};

		Ok(Arc::new(delivery_impl))
	}

	/// Initialize state manager
	#[instrument(skip(config))]
	async fn init_state(config: &SolverConfig) -> Result<Arc<StateManager>> {
		let storage_backend = match config.state.storage_backend.as_str() {
			"memory" => StorageBackend::Memory,
			"file" => StorageBackend::File {
				path: config
					.state
					.storage_path
					.clone()
					.unwrap_or_else(|| "./data/solver-state".into()),
			},
			_ => return Err(SolverError::Config("Invalid storage backend".to_string()).into()),
		};

		let state_config = StateConfig {
			max_queue_size: config.state.max_queue_size,
			storage_backend,
			recover_on_startup: config.state.recover_on_startup,
		};

		let manager = StateManager::new(state_config).await?;
		Ok(Arc::new(manager))
	}

	/// Initialize discovery service
	#[instrument(skip(config, chain_adapters, order_registry))]
	fn init_discovery(
		config: &SolverConfig,
		chain_adapters: Arc<ChainRegistry>,
		order_registry: Arc<OrderRegistry>,
	) -> Result<Arc<IntentDiscovery>> {
		let mut sources: Vec<Box<dyn solver_discovery::IntentSource>> = Vec::new();

		// Get event signatures dynamically from the order registry
		let event_signatures = order_registry.get_event_signatures();

		if event_signatures.is_empty() {
			warn!(
				"No event signatures found in order registry. Discovery may not find any orders."
			);
		} else {
			info!(
				"Discovered {} event signatures from order registry",
				event_signatures.len()
			);
		}

		// Create an OnChainSource for each chain we want to monitor
		for chain_id in &config.discovery.monitor_chains {
			if let Some(adapter) = chain_adapters.get(chain_id) {
				if let Some(chain_config) = config.chains.get(chain_id) {
					if let Some(settler_address) = chain_config.contracts.settler {
						// Create OnChainSource for this chain
						let onchain_config = OnChainConfig {
							chain_id: *chain_id,
							settler_addresses: vec![settler_address],
							start_block: config.discovery.start_blocks.get(chain_id).copied(),
							event_signatures: event_signatures.clone(),
							monitor_config: MonitorConfig::default(),
						};

						let source = OnChainSource::new(onchain_config, adapter.clone());
						sources.push(Box::new(source));
					}
				}
			}
		}

		let discovery = IntentDiscovery::new(sources, order_registry);
		Ok(Arc::new(discovery))
	}

	/// Initialize settlement manager
	#[instrument(skip(
		config,
		state_manager,
		order_registry,
		chain_registry,
		delivery_service
	))]
	async fn init_settlement(
		config: &SolverConfig,
		state_manager: Arc<StateManager>,
		order_registry: Arc<OrderRegistry>,
		chain_registry: Arc<ChainRegistry>,
		delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
	) -> Result<Arc<SettlementManager>> {
		let settlement_config = SettlementManagerConfig {
			strategies: config
				.settlement
				.strategies
				.iter()
				.map(|(k, v)| {
					(
						match k.as_str() {
							"ArbitrumBroadcaster" => {
								solver_settlement::SettlementType::ArbitrumBroadcaster
							}
							"Direct" => solver_settlement::SettlementType::Direct,
							_ => solver_settlement::SettlementType::Direct,
						},
						v.clone(),
					)
				})
				.collect(),
			default_strategy: match config.settlement.default_type.as_str() {
				"ArbitrumBroadcaster" => solver_settlement::SettlementType::ArbitrumBroadcaster,
				"Direct" => solver_settlement::SettlementType::Direct,
				_ => return Err(SolverError::Config("Invalid settlement type".to_string()).into()),
			},
			poll_interval: Duration::from_secs(config.settlement.poll_interval_secs),
			max_attempts: config.settlement.max_attempts,
		};

		let manager = SettlementManager::new(
			settlement_config,
			state_manager,
			order_registry,
			chain_registry,
			delivery_service,
		)
		.await?;
		Ok(Arc::new(manager))
	}

	/// Start the coordinator
	#[instrument(skip(self))]
	pub async fn start(&self) -> Result<()> {
		info!("Starting solver coordination");

		// Start engine
		self.engine.start().await?;

		// Start discovery
		let mut discovery_stream = self.discovery.clone().start_discovery().await?;

		// Start settlement monitoring
		self.settlement_manager.clone().start_monitoring().await;

		// Main processing loop
		let engine = self.engine.clone();
		let state_manager = self.state_manager.clone();
		let order_registry = self.order_registry.clone();

		let process_handle = tokio::spawn(async move {
			loop {
				match discovery_stream.next().await {
					Some(Ok(intent)) => {
						info!("Discovered intent: {}", intent.order.id());

						// Add to state
						if let Err(e) = state_manager.add_discovered_intent(intent).await {
							error!("Failed to add intent to state: {}", e);
							continue;
						}

						// Process order
						if let Some(order_state) =
							state_manager.get_next_order().await.unwrap_or(None)
						{
							let engine = engine.clone();
							let state_manager_clone = state_manager.clone();
							let order_registry_clone = order_registry.clone();

							tokio::spawn(async move {
								info!("Processing order: {}", order_state.id);

								// Update order status to Filling
								if let Err(e) = state_manager_clone
									.update_order_status(
										&order_state.id,
										OrderStatus::Filling,
										None,
									)
									.await
								{
									error!("Failed to update order status: {}", e);
									return;
								}

								// Parse the order from raw data
								let order = match order_registry_clone
									.parse_order(&order_state.order_data)
									.await
								{
									Ok(order) => order,
									Err(e) => {
										error!("Failed to parse order {}: {}", order_state.id, e);

										// Update order status to Invalid
										let _ = state_manager_clone
											.update_order_status(
												&order_state.id,
												OrderStatus::Invalid,
												None,
											)
											.await;

										return;
									}
								};

								// Process the order through the engine
								match engine.process_order(order).await {
									Ok(_) => {
										info!("Successfully processed order: {}", order_state.id);

										// Update order status to Filled
										if let Err(e) = state_manager_clone
											.update_order_status(
												&order_state.id,
												OrderStatus::Filled,
												None,
											)
											.await
										{
											error!("Failed to update order status: {}", e);
										}
									}
									Err(e) => {
										error!("Failed to process order {}: {}", order_state.id, e);

										// Update order status to Abandoned
										let _ = state_manager_clone
											.update_order_status(
												&order_state.id,
												OrderStatus::Abandoned,
												Some(e.to_string()),
											)
											.await;
									}
								}
							});
						}
					}
					Some(Err(e)) => {
						error!("Discovery error: {}", e);
					}
					None => {
						warn!("Discovery stream ended, restarting...");
						break;
					}
				}
			}

			Ok::<(), anyhow::Error>(())
		});

		self.process_handle.lock().await.replace(process_handle);

		Ok(())
	}

	/// Stop the coordinator
	#[instrument(skip(self))]
	pub async fn stop(&self) -> Result<()> {
		info!("Stopping solver coordination");

		// Stop engine
		self.engine.stop().await?;

		// Abort processing task
		if let Some(handle) = self.process_handle.lock().await.take() {
			handle.abort();
		}

		info!("Solver coordination stopped");
		Ok(())
	}

	/// Get coordinator statistics
	pub async fn stats(&self) -> CoordinatorStats {
		CoordinatorStats {
			engine_stats: self.engine.stats().await,
			queue_stats: self.state_manager.get_stats().await.unwrap_or_default(),
			config_summary: ConfigSummary {
				solver_name: self.config.solver.name.clone(),
				monitored_chains: self.config.discovery.monitor_chains.clone(),
				storage_backend: self.config.state.storage_backend.clone(),
				settlement_type: self.config.settlement.default_type.clone(),
			},
		}
	}
}

impl Default for SolverCoordinator {
	fn default() -> Self {
		Self::new(SolverConfig::default())
			.now_or_never()
			.unwrap()
			.unwrap()
	}
}

use futures::FutureExt;

/// Coordinator statistics
#[derive(Debug, serde::Serialize)]
pub struct CoordinatorStats {
	pub engine_stats: crate::EngineStats,
	pub queue_stats: solver_state::manager::StateStats,
	pub config_summary: ConfigSummary,
}

/// Configuration summary
#[derive(Debug, serde::Serialize)]
pub struct ConfigSummary {
	pub solver_name: String,
	pub monitored_chains: Vec<ChainId>,
	pub storage_backend: String,
	pub settlement_type: String,
}
