//! Direct settlement strategy that supports multiple order standards.

use async_trait::async_trait;
use solver_delivery::DeliveryService;
use solver_types::{
	chains::{ChainId, Transaction},
	common::{Address, TxHash},
	errors::{Result, SolverError},
	orders::{Order, OrderId, OrderStandard},
};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::{
	encoders::{
		eip7683::{EIP7683EncoderConfig, EIP7683SettlementEncoder},
		SettlementEncoder,
	},
	implementations::SettlementStrategy,
	types::{Attestation, DirectConfig},
};

/// Direct settlement - immediately claim after fill (no attestation needed)
/// Supports multiple order standards through encoders
#[derive(Clone)]
pub struct DirectSettlementStrategy {
	config: DirectConfig,
	chain_registry: Arc<solver_chains::ChainRegistry>,
	delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
	encoders: HashMap<OrderStandard, Arc<dyn SettlementEncoder>>,
}

impl DirectSettlementStrategy {
	pub fn new(
		config: DirectConfig,
		chain_registry: Arc<solver_chains::ChainRegistry>,
		delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
	) -> Self {
		let mut encoders = HashMap::new();

		// Initialize EIP-7683 encoder
		let eip7683_config = EIP7683EncoderConfig {
			settler_addresses: config.settler_addresses.clone(),
			oracle_address: config.oracle_address,
			solver_address: config.solver_address,
			gas_limit: config.gas_limit,
		};
		encoders.insert(
			OrderStandard::EIP7683,
			Arc::new(EIP7683SettlementEncoder::new(eip7683_config)) as Arc<dyn SettlementEncoder>,
		);

		Self {
			config,
			chain_registry,
			delivery_service,
			encoders,
		}
	}

	/// Get encoder for the order standard
	fn get_encoder(&self, order: &dyn Order) -> Result<&Arc<dyn SettlementEncoder>> {
		let standard = order.standard();
		self.encoders.get(&standard).ok_or_else(|| {
			SolverError::Settlement(format!(
				"No settlement encoder configured for order standard: {:?}",
				standard
			))
		})
	}

	/// Get settler address for a chain
	fn get_settler_address(&self, chain_id: ChainId) -> Result<Address> {
		self.config
			.settler_addresses
			.get(&chain_id)
			.copied()
			.ok_or_else(|| {
				SolverError::Settlement(format!(
					"No settler address configured for chain {}",
					chain_id
				))
			})
	}
}

#[async_trait]
impl SettlementStrategy for DirectSettlementStrategy {
	fn name(&self) -> &str {
		"DirectSettlement"
	}

	async fn check_attestation(
		&self,
		order_id: OrderId,
		_fill_tx: TxHash,
		fill_timestamp: u64,
		_origin_chain: ChainId,
		_destination_chain: ChainId,
	) -> Result<Option<Attestation>> {
		debug!(
			"Direct settlement for order {} - no attestation needed",
			order_id
		);

		// Direct settlement doesn't need attestation
		// Return a dummy attestation immediately
		// Use configured solver address or get from delivery service
		let solver_address = if let Some(addr) = self.config.solver_address {
			addr
		} else {
			// Get from delivery service config
			Address::zero() // TODO: Get actual solver address from delivery service
		};

		Ok(Some(Attestation {
			order_id,
			fill_hash: TxHash::zero(),
			solver: solver_address,
			timestamp: fill_timestamp,
			data: vec![], // No attestation data needed
			signature: None,
		}))
	}

	async fn claim_settlement(
		&self,
		order: &dyn Order,
		_attestation: Attestation,
	) -> Result<TxHash> {
		info!(
			"Direct settlement for order {} ({:?}) - claiming immediately",
			order.id(),
			order.standard()
		);

		// Get the appropriate encoder for this order standard
		let encoder = self.get_encoder(order)?;

		// Get settler address for the origin chain
		let origin_chain = order.origin_chain();
		let settler_address = self.get_settler_address(origin_chain)?;

		info!(
			"Using {} encoder with settler address {} on chain {}",
			encoder.name(),
			settler_address,
			origin_chain
		);

		// Build the settlement transaction using the appropriate encoder
		let tx = encoder
			.encode_claim_transaction(order, settler_address, &_attestation)
			.await?;

		// Apply gas multiplier if configured
		let final_tx = if let Some(gas_limit) = tx.gas_limit {
			let gas_limit_u64 = gas_limit.as_u64();
			let final_gas = if let Some(multiplier) = self.config.gas_multiplier {
				((gas_limit_u64 as f64) * multiplier) as u64
			} else {
				gas_limit_u64
			};
			Transaction {
				gas_limit: Some(final_gas.into()),
				..tx
			}
		} else {
			tx
		};

		// Submit transaction via delivery service
		match self
			.delivery_service
			.submit_transaction(origin_chain, final_tx.clone())
			.await
		{
			Ok(tx_hash) => {
				info!(
					"Successfully submitted settlement claim for order {} using {} encoder: {}",
					order.id(),
					encoder.name(),
					tx_hash
				);
				Ok(tx_hash)
			}
			Err(e) => {
				warn!(
					"Failed to submit settlement claim for order {} using {} encoder: {}",
					order.id(),
					encoder.name(),
					e
				);
				Err(e)
			}
		}
	}

	async fn estimate_attestation_time(&self) -> std::time::Duration {
		// Direct settlement is immediate
		std::time::Duration::from_secs(0)
	}

	async fn is_claimed(&self, order_id: OrderId, origin_chain: ChainId) -> Result<bool> {
		// Check on-chain if the order has been claimed
		// This would require reading from the settler contract

		// For now, we'll use a simple approach: check if we can get a chain adapter
		match self.chain_registry.get(&origin_chain) {
			Some(_adapter) => {
				// TODO: Implement actual on-chain check
				// Would need to call a view function on the settler contract
				debug!(
					"Checking if order {} is claimed on chain {}",
					order_id, origin_chain
				);
				Ok(false) // For now, assume not claimed
			}
			None => {
				warn!("No chain adapter for chain {}", origin_chain);
				Ok(false)
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::orders::OrderStandard;
	use std::collections::HashMap;

	fn create_test_config() -> DirectConfig {
		let mut settler_addresses = HashMap::new();
		settler_addresses.insert(ChainId(1), Address::from([1u8; 20]));
		settler_addresses.insert(ChainId(137), Address::from([2u8; 20]));

		DirectConfig {
			settler_addresses,
			gas_limit: Some(250_000),
			gas_multiplier: Some(1.1),
			solver_address: Some(Address::from([3u8; 20])),
			allocator_address: None,
			oracle_address: None,
			default_expiry_duration: 3600,
		}
	}

	fn create_test_strategy() -> DirectSettlementStrategy {
		let config = create_test_config();
		let chain_registry = Arc::new(solver_chains::ChainRegistry::new());
		let delivery_config = solver_delivery::DeliveryConfig {
			endpoints: HashMap::new(),
			api_key: "test".to_string(),
			gas_strategy: solver_delivery::GasStrategy::Standard,
			max_retries: 3,
			confirmations: 1,
			from_address: Address::zero(),
		};
		let delivery_service = Arc::new(solver_delivery::DeliveryServiceImpl::Rpc(
			solver_delivery::RpcDelivery::new(delivery_config, chain_registry.clone()),
		));

		DirectSettlementStrategy::new(config, chain_registry, delivery_service)
	}

	// Mock order for testing
	#[derive(Debug)]
	struct MockOrder {
		id: OrderId,
		origin: ChainId,
	}

	#[async_trait]
	impl Order for MockOrder {
		fn id(&self) -> OrderId {
			self.id
		}
		fn standard(&self) -> OrderStandard {
			OrderStandard::EIP7683
		}
		fn origin_chain(&self) -> ChainId {
			self.origin
		}
		fn destination_chains(&self) -> Vec<ChainId> {
			vec![ChainId(137)]
		}
		fn created_at(&self) -> u64 {
			0
		}
		fn expires_at(&self) -> u64 {
			0
		}
		async fn validate(&self) -> Result<()> {
			Ok(())
		}
		async fn to_fill_instructions(&self) -> Result<Vec<solver_types::orders::FillInstruction>> {
			Ok(vec![])
		}
		fn as_any(&self) -> &dyn std::any::Any {
			self
		}
		fn user(&self) -> Address {
			Address::from([1u8; 20])
		}
		fn inputs(&self) -> Result<Vec<solver_types::orders::Input>> {
			Ok(vec![solver_types::orders::Input {
				token: Address::from([2u8; 20]),
				amount: solver_types::common::U256::from(1_000_000_000_000_000_000u64),
			}])
		}
		fn outputs(&self) -> Result<Vec<solver_types::orders::Output>> {
			Ok(vec![solver_types::orders::Output {
				token: Address::from([3u8; 20]),
				amount: solver_types::common::U256::from(1_000_000_000_000_000_000u64),
				recipient: Address::from([4u8; 20]),
				chain_id: ChainId(137),
			}])
		}
	}

	#[tokio::test]
	async fn test_direct_settlement_immediate_attestation() {
		let strategy = create_test_strategy();
		let order_id = OrderId::zero();

		// Should return attestation immediately
		let attestation = strategy
			.check_attestation(
				order_id,
				TxHash::zero(),
				1640995200,
				ChainId(1),
				ChainId(137),
			)
			.await
			.unwrap();

		assert!(attestation.is_some());
		let attestation = attestation.unwrap();
		assert_eq!(attestation.order_id, order_id);
		assert_eq!(attestation.solver, Address::from([3u8; 20])); // From config
	}

	#[tokio::test]
	async fn test_get_settler_address() {
		let strategy = create_test_strategy();

		// Should return configured address for chain 1
		let addr = strategy.get_settler_address(ChainId(1)).unwrap();
		assert_eq!(addr, Address::from([1u8; 20]));

		// Should return configured address for chain 137
		let addr = strategy.get_settler_address(ChainId(137)).unwrap();
		assert_eq!(addr, Address::from([2u8; 20]));

		// Should error for unconfigured chain
		assert!(strategy.get_settler_address(ChainId(999)).is_err());
	}

	#[tokio::test]
	async fn test_encoder_selection() {
		let strategy = create_test_strategy();
		let order = MockOrder {
			id: OrderId::from([4u8; 32]),
			origin: ChainId(1),
		};

		// Should find EIP7683 encoder
		let encoder = strategy.get_encoder(&order).unwrap();
		assert_eq!(encoder.name(), "EIP7683");
		assert!(encoder.supports(&order));
	}

	#[tokio::test]
	async fn test_no_attestation_wait_time() {
		let strategy = create_test_strategy();

		// Direct settlement has no wait time
		assert_eq!(
			strategy.estimate_attestation_time().await,
			std::time::Duration::from_secs(0)
		);
	}

	#[tokio::test]
	async fn test_direct_settlement_with_solver_address() {
		// Test with a specific solver address configured
		let mut config = create_test_config();
		config.solver_address = Some(Address::from([0xf3; 20]));

		let chain_registry = Arc::new(solver_chains::ChainRegistry::new());
		let delivery_service = Arc::new(solver_delivery::DeliveryServiceImpl::Rpc(
			solver_delivery::RpcDelivery::new(
				solver_delivery::DeliveryConfig {
					endpoints: HashMap::new(),
					api_key: "test".to_string(),
					gas_strategy: solver_delivery::GasStrategy::Standard,
					max_retries: 3,
					confirmations: 0,
					from_address: Address::from([0xf3; 20]),
				},
				chain_registry.clone(),
			),
		));

		let strategy = DirectSettlementStrategy::new(config, chain_registry, delivery_service);

		let order_id = OrderId::from([1u8; 32]);
		let attestation = strategy
			.check_attestation(
				order_id,
				TxHash::zero(),
				1640995200,
				ChainId(1),
				ChainId(137),
			)
			.await
			.unwrap();

		assert!(attestation.is_some());
		let attestation = attestation.unwrap();
		assert_eq!(attestation.solver, Address::from([0xf3; 20]));
	}

	#[tokio::test]
	async fn test_unsupported_order_standard() {
		let strategy = create_test_strategy();

		// Create a mock order with a custom standard
		#[derive(Debug)]
		struct CustomOrder {
			id: OrderId,
			origin: ChainId,
		}

		#[async_trait]
		impl Order for CustomOrder {
			fn id(&self) -> OrderId {
				self.id
			}
			fn standard(&self) -> OrderStandard {
				OrderStandard::Custom("Custom".to_string())
			}
			fn origin_chain(&self) -> ChainId {
				self.origin
			}
			fn destination_chains(&self) -> Vec<ChainId> {
				vec![ChainId(137)]
			}
			fn created_at(&self) -> u64 {
				0
			}
			fn expires_at(&self) -> u64 {
				0
			}
			async fn validate(&self) -> Result<()> {
				Ok(())
			}
			async fn to_fill_instructions(
				&self,
			) -> Result<Vec<solver_types::orders::FillInstruction>> {
				Ok(vec![])
			}
			fn as_any(&self) -> &dyn std::any::Any {
				self
			}
			fn user(&self) -> Address {
				Address::from([1u8; 20])
			}
			fn inputs(&self) -> Result<Vec<solver_types::orders::Input>> {
				Ok(vec![])
			}
			fn outputs(&self) -> Result<Vec<solver_types::orders::Output>> {
				Ok(vec![])
			}
		}

		let custom_order = CustomOrder {
			id: OrderId::from([7u8; 32]),
			origin: ChainId(1),
		};

		// Should fail to find encoder for unsupported standard
		let result = strategy.get_encoder(&custom_order);
		assert!(result.is_err());
		assert!(result
			.err()
			.unwrap()
			.to_string()
			.contains("No settlement encoder configured for order standard: Custom(\"Custom\")"));
	}

	#[tokio::test]
	async fn test_claim_settlement_with_encoder() {
		// Set up strategy with actual endpoint
		let mut settler_addresses = HashMap::new();
		settler_addresses.insert(ChainId(31337), Address::from([0xcf; 20]));

		let config = DirectConfig {
			settler_addresses,
			gas_limit: Some(250_000),
			gas_multiplier: Some(1.1),
			solver_address: None,
			allocator_address: None,
			oracle_address: None,
			default_expiry_duration: 3600,
		};

		let chain_registry = Arc::new(solver_chains::ChainRegistry::new());
		let mut endpoints = HashMap::new();
		endpoints.insert(ChainId(31337), "http://localhost:8545".to_string());

		let delivery_service = Arc::new(solver_delivery::DeliveryServiceImpl::Rpc(
			solver_delivery::RpcDelivery::new(
				solver_delivery::DeliveryConfig {
					endpoints,
					api_key: "test".to_string(),
					gas_strategy: solver_delivery::GasStrategy::Standard,
					max_retries: 3,
					confirmations: 0,
					from_address: Address::from([0xf3; 20]),
				},
				chain_registry.clone(),
			),
		));

		let strategy = DirectSettlementStrategy::new(config, chain_registry, delivery_service);

		let test_order = MockOrder {
			id: OrderId::from([2u8; 32]),
			origin: ChainId(31337),
		};

		let attestation = Attestation {
			order_id: test_order.id,
			fill_hash: TxHash::zero(),
			solver: Address::zero(),
			timestamp: 0,
			data: vec![],
			signature: None,
		};

		// Test claim - should use the EIP7683 encoder
		match strategy.claim_settlement(&test_order, attestation).await {
			Ok(_tx_hash) => {
				// In a real test environment with a running chain, this would succeed
				println!("✅ Claim settlement transaction would be submitted using encoder");
			}
			Err(e) => {
				// Expected in test environment without actual chain
				println!("⚠️  Expected error without chain: {}", e);
				// Could be "No Signer" or other RPC errors depending on configuration
				assert!(
					e.to_string().contains("No Signer available")
						|| e.to_string().contains("RPC error")
						|| e.to_string().contains("Network error")
						|| e.to_string().contains("No chain adapter configured")
				);
			}
		}
	}
}
