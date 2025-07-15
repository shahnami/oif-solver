//! Settlement manager that coordinates the settlement process.

use crate::{
	implementations::{create_strategy, SettlementStrategy, SettlementStrategyImpl},
	types::{Attestation, SettlementData, SettlementStatus, SettlementType},
};
use solver_orders::OrderRegistry;
use solver_state::StateManager;
use solver_types::{
	common::TxHash,
	errors::{Result, SolverError},
	orders::{Order, OrderId},
};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Settlement configuration
#[derive(Debug, Clone)]
pub struct SettlementConfig {
	/// Settlement strategies by type
	pub strategies: HashMap<SettlementType, serde_json::Value>,
	/// Default strategy to use
	pub default_strategy: SettlementType,
	/// How often to check for attestations
	pub poll_interval: Duration,
	/// Maximum attempts before giving up
	pub max_attempts: u32,
}

/// Manages the settlement lifecycle
pub struct SettlementManager {
	strategies: HashMap<SettlementType, Arc<SettlementStrategyImpl>>,
	settlements: Arc<RwLock<HashMap<OrderId, SettlementData>>>,
	state_manager: Arc<StateManager>,
	order_registry: Arc<OrderRegistry>,
	config: SettlementConfig,
}

impl SettlementManager {
	/// Create new settlement manager
	pub async fn new(
		config: SettlementConfig,
		state_manager: Arc<StateManager>,
		order_registry: Arc<OrderRegistry>,
		chain_registry: Arc<solver_chains::ChainRegistry>,
		delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
	) -> Result<Self> {
		let mut strategies = HashMap::new();

		// Initialize configured strategies
		for (settlement_type, strategy_config) in &config.strategies {
			let strategy = create_strategy(
				settlement_type.clone(),
				strategy_config.clone(),
				chain_registry.clone(),
				delivery_service.clone(),
			)?;
			strategies.insert(settlement_type.clone(), strategy);
		}

		Ok(Self {
			strategies,
			settlements: Arc::new(RwLock::new(HashMap::new())),
			state_manager,
			order_registry,
			config,
		})
	}

	/// Register a fill for settlement
	pub async fn register_fill(
		&self,
		order: &dyn Order,
		fill_tx: TxHash,
		fill_timestamp: Option<u64>,
		settlement_type: Option<SettlementType>,
	) -> Result<()> {
		let order_id = order.id();
		let settlement_type = settlement_type.unwrap_or(self.config.default_strategy.clone());

		info!(
			"Registering fill for order {} with {:?} settlement",
			order_id, settlement_type
		);

		let settlement_data = SettlementData {
			order_id,
			origin_chain: order.origin_chain(),
			destination_chain: order.destination_chains()[0], // Assuming single destination
			settler_address: Default::default(),              // Would get from order data
			status: SettlementStatus::AwaitingAttestation {
				fill_tx,
				filled_at: fill_timestamp.unwrap_or(chrono::Utc::now().timestamp() as u64),
			},
			settlement_type,
			created_at: chrono::Utc::now().timestamp() as u64,
			updated_at: chrono::Utc::now().timestamp() as u64,
			attempts: 0,
			fill_timestamp,
		};

		self.settlements
			.write()
			.await
			.insert(order_id, settlement_data);

		// Update order status
		self.state_manager
			.update_order_status(
				&order_id,
				solver_discovery::OrderStatus::Ready, // Use Ready as closest to settling
				None,
			)
			.await?;

		Ok(())
	}

	/// Check and process pending settlements
	pub async fn process_settlements(&self) -> Result<()> {
		let settlements = self.settlements.read().await.clone();

		for (order_id, mut settlement) in settlements {
			if settlement.attempts >= self.config.max_attempts {
				warn!("Max attempts reached for order {}", order_id);
				continue;
			}

			let fill_tx = match &settlement.status {
				SettlementStatus::AwaitingAttestation { fill_tx, .. } => *fill_tx,
				_ => TxHash::zero(),
			};

			match &settlement.status {
				SettlementStatus::AwaitingAttestation { .. } => {
					self.check_attestation(&mut settlement, fill_tx).await?;
				}
				SettlementStatus::ReadyToClaim { .. } => {
					self.claim_settlement(&mut settlement).await?;
				}
				SettlementStatus::Claiming { submitted_at, .. } => {
					// Check if claim is confirmed
					let elapsed = chrono::Utc::now().timestamp() as u64 - submitted_at;
					if elapsed > 300 {
						// 5 minutes timeout
						settlement.attempts += 1;
						settlement.status = SettlementStatus::Failed {
							reason: "Claim timeout".to_string(),
							can_retry: true,
						};
					}
				}
				_ => {} // Completed or Failed - nothing to do
			}

			// Update settlement data
			settlement.updated_at = chrono::Utc::now().timestamp() as u64;
			self.settlements.write().await.insert(order_id, settlement);
		}

		Ok(())
	}

	/// Check if attestation is available
	async fn check_attestation(
		&self,
		settlement: &mut SettlementData,
		fill_tx: TxHash,
	) -> Result<()> {
		let strategy = self
			.strategies
			.get(&settlement.settlement_type)
			.ok_or_else(|| SolverError::Settlement("Strategy not found".to_string()))?;

		debug!("Checking attestation for order {}", settlement.order_id);

		let fill_timestamp = match &settlement.status {
			SettlementStatus::AwaitingAttestation { filled_at, .. } => *filled_at,
			_ => return Ok(()),
		};

		match strategy
			.check_attestation(
				settlement.order_id,
				fill_tx,
				fill_timestamp,
				settlement.origin_chain,
				settlement.destination_chain,
			)
			.await?
		{
			Some(attestation) => {
				info!("Attestation available for order {}", settlement.order_id);
				settlement.status = SettlementStatus::ReadyToClaim {
					attestation_block: chrono::Utc::now().timestamp() as u64,
					attestation_data: attestation.data,
				};
			}
			None => {
				debug!(
					"Attestation not yet available for order {}",
					settlement.order_id
				);
			}
		}

		Ok(())
	}

	/// Claim settlement
	async fn claim_settlement(&self, settlement: &mut SettlementData) -> Result<()> {
		let strategy: &Arc<SettlementStrategyImpl> = self
			.strategies
			.get(&settlement.settlement_type)
			.ok_or_else(|| SolverError::Settlement("Strategy not found".to_string()))?;

		// Get order from state
		let order_state = self
			.state_manager
			.get_order_state(&settlement.order_id)
			.await?
			.ok_or_else(|| SolverError::Settlement("Order not found".to_string()))?;

		info!("Claiming settlement for order {}", settlement.order_id);

		// Parse the order from raw data
		let order = self
			.order_registry
			.parse_order(&order_state.order_data)
			.await?;

		// Extract attestation data from settlement
		let attestation_data = match &settlement.status {
			SettlementStatus::ReadyToClaim {
				attestation_data, ..
			} => attestation_data.clone(),
			_ => vec![],
		};

		// Create attestation for the claim
		let attestation = Attestation {
			order_id: settlement.order_id,
			fill_hash: TxHash::zero(), // Will be populated by strategy if needed
			solver: settlement.settler_address,
			timestamp: settlement
				.fill_timestamp
				.unwrap_or(chrono::Utc::now().timestamp() as u64),
			data: attestation_data,
			signature: None,
		};

		// Call the strategy to submit the claim transaction
		match strategy.claim_settlement(&order, attestation).await {
			Ok(claim_tx) => {
				info!(
					"Successfully submitted settlement claim for order {}: {}",
					settlement.order_id, claim_tx
				);
				settlement.status = SettlementStatus::Claiming {
					claim_tx,
					submitted_at: chrono::Utc::now().timestamp() as u64,
				};
				Ok(())
			}
			Err(e) => {
				warn!(
					"Failed to submit settlement claim for order {}: {}",
					settlement.order_id, e
				);
				settlement.attempts += 1;
				Err(e)
			}
		}
	}

	/// Get settlement data for an order
	pub async fn get_settlement(&self, order_id: &OrderId) -> Option<SettlementData> {
		self.settlements.read().await.get(order_id).cloned()
	}

	/// Get all settlements by status
	pub async fn get_settlements_by_status(
		&self,
		status: &SettlementStatus,
	) -> Vec<SettlementData> {
		self.settlements
			.read()
			.await
			.values()
			.filter(|s| std::mem::discriminant(&s.status) == std::mem::discriminant(status))
			.cloned()
			.collect()
	}

	/// Start settlement monitoring loop
	pub async fn start_monitoring(self: Arc<Self>) {
		info!(
			"Starting settlement monitoring with interval {:?}",
			self.config.poll_interval
		);

		tokio::spawn(async move {
			loop {
				if let Err(e) = self.process_settlements().await {
					warn!("Error processing settlements: {}", e);
				}

				tokio::time::sleep(self.config.poll_interval).await;
			}
		});
	}
}
