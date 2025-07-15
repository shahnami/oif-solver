//! Core solver engine that orchestrates the solving process.

use solver_delivery::DeliveryService;
use solver_types::{
	chains::Transaction,
	common::{Address, TxHash, U256},
	errors::{Result, SolverError},
	orders::{FillData, FillInstruction, Order, OrderStatus},
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Main solver engine
pub struct SolverEngine {
	/// Order processor
	processor: Arc<OrderProcessor>,
	/// Engine state
	state: Arc<RwLock<EngineState>>,
}

/// Engine state
#[derive(Default)]
struct EngineState {
	pub is_running: bool,
	pub processed_orders: u64,
	pub failed_orders: u64,
}

/// Order processor handles individual order execution
#[derive(Clone)]
pub struct OrderProcessor {
	/// Chain registry for cross-chain operations
	chain_registry: Arc<solver_chains::ChainRegistry>,
	/// Delivery service for transaction submission
	delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
	/// Settlement manager for claiming
	settlement_manager: Arc<solver_settlement::SettlementManager>,
	/// State manager for order updates
	state_manager: Arc<solver_state::StateManager>,
	/// Solver's address
	solver_address: Address,
	/// Minimum profit threshold in basis points
	min_profit_bps: u16,
}

impl SolverEngine {
	pub fn new(
		chain_registry: Arc<solver_chains::ChainRegistry>,
		delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
		settlement_manager: Arc<solver_settlement::SettlementManager>,
		state_manager: Arc<solver_state::StateManager>,
		solver_address: Address,
		min_profit_bps: u16,
	) -> Self {
		Self {
			processor: Arc::new(OrderProcessor::new(
				chain_registry,
				delivery_service,
				settlement_manager,
				state_manager,
				solver_address,
				min_profit_bps,
			)),
			state: Arc::new(RwLock::new(EngineState::default())),
		}
	}

	/// Start the solver engine
	pub async fn start(&self) -> Result<()> {
		info!("Starting solver engine");

		let mut state = self.state.write().await;
		state.is_running = true;

		Ok(())
	}

	/// Stop the solver engine
	pub async fn stop(&self) -> Result<()> {
		info!("Stopping solver engine");

		let mut state = self.state.write().await;
		state.is_running = false;

		Ok(())
	}

	/// Process a discovered order
	pub async fn process_order<O>(&self, order: O) -> Result<()>
	where
		O: Order + 'static,
	{
		let order_id = order.id();
		debug!("Processing order {}", order_id);

		// Validate order
		if let Err(e) = order.validate().await {
			warn!("Order {} validation failed: {}", order_id, e);
			let mut state = self.state.write().await;
			state.failed_orders += 1;
			return Err(e);
		}

		// Process order (placeholder for now)
		self.processor.process(order).await?;

		let mut state = self.state.write().await;
		state.processed_orders += 1;

		Ok(())
	}

	/// Get engine statistics
	pub async fn stats(&self) -> EngineStats {
		let state = self.state.read().await;
		EngineStats {
			is_running: state.is_running,
			processed_orders: state.processed_orders,
			failed_orders: state.failed_orders,
		}
	}
}

impl OrderProcessor {
	pub fn new(
		chain_registry: Arc<solver_chains::ChainRegistry>,
		delivery_service: Arc<solver_delivery::DeliveryServiceImpl>,
		settlement_manager: Arc<solver_settlement::SettlementManager>,
		state_manager: Arc<solver_state::StateManager>,
		solver_address: Address,
		min_profit_bps: u16,
	) -> Self {
		Self {
			chain_registry,
			delivery_service,
			settlement_manager,
			state_manager,
			solver_address,
			min_profit_bps,
		}
	}

	pub async fn process<O>(&self, order: O) -> Result<()>
	where
		O: Order + 'static,
	{
		let order_id = order.id();
		info!("Starting to process order {}", order_id);

		// 1. Check profitability
		if !self.check_profitability(&order).await? {
			warn!("Order {} not profitable, skipping", order_id);
			self.state_manager
				.update_order_status(
					&order_id,
					OrderStatus::Abandoned,
					Some("Not profitable".to_string()),
				)
				.await?;
			return Ok(());
		}

		// 2. Get fill instructions
		let fill_instructions = order.to_fill_instructions().await?;
		if fill_instructions.is_empty() {
			error!("No fill instructions for order {}", order_id);
			return Err(SolverError::Order("No fill instructions".to_string()));
		}

		// 3. Fill on destination
		let (fill_tx, fill_timestamp) = self.execute_fill(&order, &fill_instructions[0]).await?;
		info!("Order {} filled with tx {}", order_id, fill_tx);

		// Update order status to Filled
		self.state_manager
			.update_order_status(&order_id, OrderStatus::Filled, None)
			.await?;

		// 4. Register fill with settlement manager
		self.settlement_manager
			.register_fill(&order, fill_tx, fill_timestamp, None)
			.await?;

		info!(
			"Order {} processing complete, fill registered for settlement",
			order_id
		);

		Ok(())
	}

	/// Check if order is profitable
	async fn check_profitability<O>(&self, order: &O) -> Result<bool>
	where
		O: Order,
	{
		// Simple profitability check - can be enhanced
		// For now, just check if we have minimum profit threshold

		// In a real implementation, you would:
		// 1. Calculate input value
		// 2. Calculate output value
		// 3. Estimate gas costs
		// 4. Check if profit > gas + minimum threshold

		debug!("Checking profitability for order {}", order.id());

		// For POC, always return true if min_profit_bps is 0
		if self.min_profit_bps == 0 {
			return Ok(true);
		}

		// TODO: Implement actual profitability calculation
		Ok(true)
	}

	/// Execute fill on destination chain
	async fn execute_fill<O>(
		&self,
		order: &O,
		instruction: &FillInstruction,
	) -> Result<(TxHash, Option<u64>)>
	where
		O: Order,
	{
		info!(
			"Executing fill for order {} on chain {}",
			order.id(),
			instruction.destination_chain
		);

		// Get chain adapter (unused for now, but will be needed for more complex fills)
		let _chain = self
			.chain_registry
			.get_required(&instruction.destination_chain)?;

		// Build fill transaction
		let fill_tx = self.build_fill_transaction(order, instruction)?;

		// Debug log the transaction details
		info!(
			"Fill transaction - to: {}, data: 0x{}, value: {}",
			fill_tx.to,
			ethers::utils::hex::encode(&fill_tx.data),
			fill_tx.value
		);

		// Estimate gas
		match self
			.delivery_service
			.estimate_gas(instruction.destination_chain, &fill_tx)
			.await
		{
			Ok(estimate) => {
				info!("Gas estimate for fill: {}", estimate);
			}
			Err(e) => {
				error!("Failed to estimate gas: {}", e);
				return Err(e);
			}
		}

		// Submit transaction
		let tx_hash = self
			.delivery_service
			.submit_transaction(instruction.destination_chain, fill_tx)
			.await?;
		info!("Fill transaction submitted: {}", tx_hash);

		// Wait for confirmation using chain-specific configuration
		let chain_adapter = self
			.chain_registry
			.get_required(&instruction.destination_chain)?;
		let confirmations = chain_adapter.confirmations();

		let receipt = self
			.delivery_service
			.wait_for_confirmation(instruction.destination_chain, tx_hash, confirmations)
			.await?;

		if !receipt.status {
			error!("Fill transaction failed for order {}", order.id());
			return Err(SolverError::Order("Fill transaction failed".to_string()));
		}

		Ok((tx_hash, receipt.timestamp))
	}

	/// Build fill transaction
	fn build_fill_transaction<O>(
		&self,
		order: &O,
		instruction: &FillInstruction,
	) -> Result<Transaction>
	where
		O: Order,
	{
		// TODO: This is a simplified version - actual implementation would need proper ABI encoding

		use ethers::abi::{encode, Token};

		let _order_id = order.id();

		// Extract fill data based on type
		let call_data = match &instruction.fill_data {
			FillData::EIP7683 {
				order_id,
				origin_data,
			} => {
				debug!(
					"Fill data - orderId: 0x{}, originData: 0x{} ({} bytes)",
					ethers::utils::hex::encode(order_id.as_ref()),
					ethers::utils::hex::encode(origin_data),
					origin_data.len()
				);

				// Encode fill function call
				// Function: fill(bytes32 orderId, bytes originData, bytes fillerData)
				// The fillerData should contain the solver address as bytes32
				let mut filler_data = vec![0u8; 32];
				// Copy solver address (20 bytes) to the last 20 bytes of the 32-byte array
				filler_data[12..32].copy_from_slice(self.solver_address.as_ref());

				let tokens = vec![
					Token::FixedBytes(order_id.as_ref().to_vec()),
					Token::Bytes(origin_data.clone()),
					Token::Bytes(filler_data),
				];

				// Function selector for fill(bytes32,bytes,bytes)
				let selector = ethers::utils::keccak256(b"fill(bytes32,bytes,bytes)")[..4].to_vec();
				let encoded = encode(&tokens);

				[selector, encoded].concat()
			}
			FillData::Generic(data) => data.clone(),
		};

		// Parse value from MandateOutput if this is an EIP7683 order
		let value = match &instruction.fill_data {
			FillData::EIP7683 { origin_data, .. } => {
				// Try to parse the output amount from MandateOutput
				// The originData contains an ABI-encoded MandateOutput struct
				// For now, we'll check if it's the expected format and extract the amount
				if origin_data.len() >= 32 + 96 + 32 {
					// Read offset
					let offset = U256::from_big_endian(&origin_data[0..32]).as_usize();
					if origin_data.len() >= offset + 192 {
						// Check if token is ETH (address 0)
						let token_start = offset + 96;
						let token_end = token_start + 32;
						let is_eth = if origin_data.len() >= token_end {
							let token_bytes = &origin_data[token_start..token_end];
							token_bytes.iter().all(|&b| b == 0)
						} else {
							false
						};

						// Amount is at offset + 128 (after oracle, settler, chainId, token)
						let amount_start = offset + 128;
						let amount_end = amount_start + 32;
						if origin_data.len() >= amount_end {
							let amount =
								U256::from_big_endian(&origin_data[amount_start..amount_end]);
							info!("Parsed output amount from MandateOutput: {}", amount);
							// Only include value in transaction if token is ETH
							if is_eth {
								amount
							} else {
								U256::zero()
							}
						} else {
							U256::zero()
						}
					} else {
						U256::zero()
					}
				} else {
					U256::zero()
				}
			}
			_ => U256::zero(),
		};

		Ok(Transaction {
			to: instruction.destination_contract,
			value,
			data: call_data,
			gas_limit: None, // Will be estimated
			gas_price: None, // Will be set by delivery service
			nonce: None,     // Will be set by delivery service
		})
	}
}

/// Engine statistics
#[derive(Debug, Clone, serde::Serialize)]
pub struct EngineStats {
	pub is_running: bool,
	pub processed_orders: u64,
	pub failed_orders: u64,
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;
	use solver_types::{
		chains::ChainId,
		common::{Bytes32, Timestamp},
		orders::{Input, OrderId, OrderStandard, Output},
	};
	use std::{any::Any, collections::HashMap};

	// Mock implementations for testing
	#[derive(Clone)]
	struct MockChainRegistry;

	impl MockChainRegistry {
		fn create() -> Arc<solver_chains::ChainRegistry> {
			let mut registry = solver_chains::ChainRegistry::new();

			// Add a mock chain adapter for chain 137
			let mock_adapter = MockChainAdapter {
				chain_id: ChainId(137),
				confirmations: 1,
			};
			registry.register(Arc::new(mock_adapter)).unwrap();

			Arc::new(registry)
		}
	}

	#[derive(Clone)]
	struct MockChainAdapter {
		chain_id: ChainId,
		confirmations: u64,
	}

	#[async_trait]
	impl solver_types::chains::ChainAdapter for MockChainAdapter {
		fn chain_id(&self) -> ChainId {
			self.chain_id
		}

		fn confirmations(&self) -> u64 {
			self.confirmations
		}

		async fn get_block_number(&self) -> Result<u64> {
			Ok(1000)
		}

		async fn get_block_timestamp(&self, _block_number: u64) -> Result<u64> {
			Ok(1640995200) // Mock timestamp
		}

		async fn get_balance(&self, _address: Address) -> Result<U256> {
			Ok(U256::from(1_000_000_000_000_000_000u64)) // 1 ETH
		}

		async fn submit_transaction(&self, _tx: Transaction) -> Result<TxHash> {
			Ok(TxHash::zero())
		}

		async fn get_transaction_receipt(
			&self,
			_tx_hash: TxHash,
		) -> Result<Option<solver_types::chains::TransactionReceipt>> {
			Ok(Some(solver_types::chains::TransactionReceipt {
				transaction_hash: TxHash::zero(),
				block_number: 1001,
				gas_used: U256::from(21000),
				status: true,
				timestamp: Some(1640995200),
			}))
		}

		async fn call(&self, _tx: Transaction, _block: Option<u64>) -> Result<Vec<u8>> {
			Ok(vec![])
		}

		async fn get_logs(
			&self,
			_address: Option<Address>,
			_topics: Vec<Option<solver_types::common::Bytes32>>,
			_from_block: u64,
			_to_block: u64,
		) -> Result<Vec<solver_types::chains::Log>> {
			Ok(vec![])
		}

		async fn estimate_gas(&self, _tx: &Transaction) -> Result<U256> {
			Ok(U256::from(100_000))
		}

		async fn get_gas_price(&self) -> Result<U256> {
			Ok(U256::from(20_000_000_000u64)) // 20 gwei
		}
	}

	// Mock order for testing
	#[derive(Debug, Clone)]
	struct MockOrder {
		id: OrderId,
		origin_chain: ChainId,
		destination_chains: Vec<ChainId>,
		expires_at: Timestamp,
		fill_instructions: Vec<FillInstruction>,
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
			self.origin_chain
		}

		fn destination_chains(&self) -> Vec<ChainId> {
			self.destination_chains.clone()
		}

		fn created_at(&self) -> Timestamp {
			0
		}

		fn expires_at(&self) -> Timestamp {
			self.expires_at
		}

		async fn validate(&self) -> Result<()> {
			Ok(())
		}

		async fn to_fill_instructions(&self) -> Result<Vec<FillInstruction>> {
			Ok(self.fill_instructions.clone())
		}

		fn as_any(&self) -> &dyn Any {
			self
		}

		fn user(&self) -> Address {
			Address::from([1u8; 20])
		}

		fn inputs(&self) -> Result<Vec<Input>> {
			Ok(vec![Input {
				token: Address::from([2u8; 20]),
				amount: U256::from(1_000_000_000_000_000_000u64),
			}])
		}

		fn outputs(&self) -> Result<Vec<Output>> {
			Ok(vec![Output {
				token: Address::from([3u8; 20]),
				amount: U256::from(1_000_000_000_000_000_000u64),
				recipient: Address::from([4u8; 20]),
				chain_id: self
					.destination_chains
					.first()
					.cloned()
					.unwrap_or(ChainId(1)),
			}])
		}
	}

	async fn create_test_engine() -> (SolverEngine, Arc<solver_state::StateManager>) {
		let chain_registry = MockChainRegistry::create();
		let delivery_service = Arc::new(solver_delivery::DeliveryServiceImpl::Rpc(
			solver_delivery::RpcDelivery::new(
				solver_delivery::DeliveryConfig {
					endpoints: std::collections::HashMap::new(),
					api_key: "test".to_string(),
					gas_strategy: solver_delivery::GasStrategy::Standard,
					max_retries: 3,
					confirmations: 1,
					from_address: Address::zero(),
				},
				chain_registry.clone(),
			),
		));

		// Create a mock state manager
		let state_config = solver_state::StateConfig {
			max_queue_size: 100,
			storage_backend: solver_state::StorageBackend::Memory,
			recover_on_startup: false,
		};
		let state_manager = Arc::new(solver_state::StateManager::new(state_config).await.unwrap());

		// Create a mock order registry
		let order_registry = Arc::new(solver_orders::OrderRegistry::new());

		// Create a mock settlement manager
		let settlement_config = solver_settlement::SettlementConfig {
			strategies: std::collections::HashMap::new(),
			default_strategy: solver_settlement::SettlementType::Direct,
			poll_interval: std::time::Duration::from_secs(5),
			max_attempts: 3,
		};
		let settlement_manager = Arc::new(
			solver_settlement::SettlementManager::new(
				settlement_config,
				state_manager.clone(),
				order_registry.clone(),
				chain_registry.clone(),
				delivery_service.clone(),
			)
			.await
			.unwrap(),
		);

		let engine = SolverEngine::new(
			chain_registry,
			delivery_service,
			settlement_manager,
			state_manager.clone(),
			Address::zero(),
			0, // min_profit_bps
		);

		(engine, state_manager)
	}

	#[tokio::test]
	async fn test_engine_lifecycle() {
		let (engine, _) = create_test_engine().await;

		// Test starting the engine
		assert!(engine.start().await.is_ok());

		// Check stats
		let stats = engine.stats().await;
		assert!(stats.is_running);
		assert_eq!(stats.processed_orders, 0);
		assert_eq!(stats.failed_orders, 0);

		// Test stopping the engine
		assert!(engine.stop().await.is_ok());

		// Check stats again
		let stats = engine.stats().await;
		assert!(!stats.is_running);
	}

	#[tokio::test]
	async fn test_process_order_validation_failure() {
		let (engine, _) = create_test_engine().await;
		engine.start().await.unwrap();

		// Create an order that fails validation
		#[derive(Debug, Clone)]
		struct InvalidOrder;

		#[async_trait]
		impl Order for InvalidOrder {
			fn id(&self) -> OrderId {
				Bytes32::from([2u8; 32])
			}

			fn standard(&self) -> OrderStandard {
				OrderStandard::EIP7683
			}

			fn origin_chain(&self) -> ChainId {
				ChainId(1)
			}

			fn destination_chains(&self) -> Vec<ChainId> {
				vec![ChainId(137)]
			}

			fn created_at(&self) -> Timestamp {
				0
			}

			fn expires_at(&self) -> Timestamp {
				chrono::Utc::now().timestamp() as u64 + 3600
			}

			async fn validate(&self) -> Result<()> {
				Err(SolverError::Order("Invalid order".to_string()))
			}

			async fn to_fill_instructions(&self) -> Result<Vec<FillInstruction>> {
				Ok(vec![])
			}

			fn as_any(&self) -> &dyn Any {
				self
			}

			fn user(&self) -> Address {
				Address::from([1u8; 20])
			}

			fn inputs(&self) -> Result<Vec<Input>> {
				Ok(vec![Input {
					token: Address::from([2u8; 20]),
					amount: U256::from(1_000_000_000_000_000_000u64),
				}])
			}

			fn outputs(&self) -> Result<Vec<Output>> {
				Ok(vec![Output {
					token: Address::from([3u8; 20]),
					amount: U256::from(1_000_000_000_000_000_000u64),
					recipient: Address::from([4u8; 20]),
					chain_id: ChainId(137),
				}])
			}
		}

		let order = InvalidOrder;
		let result = engine.process_order(order).await;
		assert!(result.is_err());

		// Check that failed orders counter was incremented
		let stats = engine.stats().await;
		assert_eq!(stats.processed_orders, 0);
		assert_eq!(stats.failed_orders, 1);
	}

	#[tokio::test]
	async fn test_profitability_check() {
		let chain_registry = MockChainRegistry::create();
		let delivery_service = Arc::new(solver_delivery::DeliveryServiceImpl::Rpc(
			solver_delivery::RpcDelivery::new(
				solver_delivery::DeliveryConfig {
					endpoints: HashMap::new(),
					api_key: "test".to_string(),
					gas_strategy: solver_delivery::GasStrategy::Standard,
					max_retries: 3,
					confirmations: 1,
					from_address: Address::zero(),
				},
				chain_registry.clone(),
			),
		));
		let state_config = solver_state::StateConfig {
			max_queue_size: 100,
			storage_backend: solver_state::StorageBackend::Memory,
			recover_on_startup: false,
		};
		let state_manager = Arc::new(solver_state::StateManager::new(state_config).await.unwrap());

		// Create a mock order registry
		let order_registry = Arc::new(solver_orders::OrderRegistry::new());

		let settlement_config = solver_settlement::SettlementConfig {
			strategies: std::collections::HashMap::new(),
			default_strategy: solver_settlement::SettlementType::Direct,
			poll_interval: std::time::Duration::from_secs(5),
			max_attempts: 3,
		};
		let settlement_manager = Arc::new(
			solver_settlement::SettlementManager::new(
				settlement_config,
				state_manager.clone(),
				order_registry.clone(),
				chain_registry.clone(),
				delivery_service.clone(),
			)
			.await
			.unwrap(),
		);

		// Create processor with non-zero min_profit_bps
		let processor = OrderProcessor::new(
			chain_registry,
			delivery_service,
			settlement_manager,
			state_manager,
			Address::zero(),
			100, // 1% minimum profit
		);

		let order = Box::new(MockOrder {
			id: Bytes32::from([3u8; 32]),
			origin_chain: ChainId(1),
			destination_chains: vec![ChainId(137)],
			expires_at: chrono::Utc::now().timestamp() as u64 + 3600,
			fill_instructions: vec![],
		});

		// For now, profitability check always returns true if implemented
		// In a real implementation, this would check actual profitability
		let profitable = processor.check_profitability(order.as_ref()).await.unwrap();
		assert!(profitable);
	}

	#[tokio::test]
	async fn test_build_fill_transaction() {
		let (engine, _) = create_test_engine().await;

		let order = Box::new(MockOrder {
			id: Bytes32::from([4u8; 32]),
			origin_chain: ChainId(1),
			destination_chains: vec![ChainId(137)],
			expires_at: chrono::Utc::now().timestamp() as u64 + 3600,
			fill_instructions: vec![],
		});

		let instruction = FillInstruction {
			destination_chain: ChainId(137),
			destination_contract: Address::from([5u8; 20]),
			fill_data: FillData::EIP7683 {
				order_id: Bytes32::from([4u8; 32]),
				origin_data: vec![0xaa, 0xbb, 0xcc],
			},
		};

		let tx = engine
			.processor
			.build_fill_transaction(order.as_ref(), &instruction)
			.unwrap();

		assert_eq!(tx.to, Address::from([5u8; 20]));
		assert_eq!(tx.value, U256::zero());
		assert!(!tx.data.is_empty());

		// Check that the transaction data starts with the function selector
		let expected_selector =
			ethers::utils::keccak256(b"fill(bytes32,bytes,bytes)")[..4].to_vec();
		assert_eq!(&tx.data[..4], &expected_selector);
	}
}
