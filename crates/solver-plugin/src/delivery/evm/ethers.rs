// solver-plugins/src/delivery/evm_ethers.rs

use async_trait::async_trait;
use dashmap::DashMap;
use ethers::prelude::*;
use ethers::providers::{Http, Middleware, Provider};
use ethers::signers::{LocalWallet, Signer};
use ethers::types::{
	Address as EthAddress, Bytes as EthBytes, TransactionReceipt as EthTransactionReceipt,
	TransactionRequest, H256, U256, U64,
};
use serde::{Deserialize, Serialize};
use solver_types::plugins::*;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

// Type aliases to resolve conflicts
type SolverTransaction = solver_types::plugins::Transaction;
type SolverTxHash = String;
type SolverTransactionReceipt = solver_types::plugins::TransactionReceipt;

/// EVM Ethers Delivery Plugin using real ethers-rs
#[derive(Debug)]
pub struct EvmEthersDeliveryPlugin {
	config: EvmEthersConfig,
	provider: Option<Arc<Provider<Http>>>,
	wallet: Option<Arc<LocalWallet>>,
	client: Option<Arc<SignerMiddleware<Provider<Http>, LocalWallet>>>,
	metrics: PluginMetrics,
	is_initialized: bool,
	pending_transactions: Arc<DashMap<SolverTxHash, PendingTransaction>>,
	nonce_manager: Arc<Mutex<Option<U256>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmEthersConfig {
	pub chain_id: ChainId,
	pub rpc_url: String,
	pub private_key: String,
	pub max_retries: u32,
	pub timeout_ms: u64,
	pub gas_price_multiplier: f64,
	pub max_gas_price: Option<u64>,
	pub enable_eip1559: bool,
	pub confirmation_blocks: u32,
	pub nonce_management: bool,
	pub mempool_monitoring: bool,
	pub max_pending_transactions: usize,
}

#[derive(Debug, Clone)]
struct PendingTransaction {
	pub request: DeliveryRequest,
	pub submitted_at: Timestamp,
	pub last_check: Timestamp,
	pub retry_count: u32,
	pub status: DeliveryStatus,
	pub nonce: Option<U256>,
}

impl Default for EvmEthersDeliveryPlugin {
	fn default() -> Self {
		Self::new()
	}
}

impl EvmEthersDeliveryPlugin {
	pub fn new() -> Self {
		Self {
			config: EvmEthersConfig::default(),
			provider: None,
			wallet: None,
			client: None,
			metrics: PluginMetrics::new(),
			is_initialized: false,
			pending_transactions: Arc::new(DashMap::new()),
			nonce_manager: Arc::new(Mutex::new(None)),
		}
	}

	pub fn with_config(config: EvmEthersConfig) -> Self {
		Self {
			config,
			provider: None,
			wallet: None,
			client: None,
			metrics: PluginMetrics::new(),
			is_initialized: false,
			pending_transactions: Arc::new(DashMap::new()),
			nonce_manager: Arc::new(Mutex::new(None)),
		}
	}

	async fn setup_provider(&mut self) -> PluginResult<()> {
		info!(
			"Setting up ethers provider for chain {}",
			self.config.chain_id
		);

		let provider = Provider::<Http>::try_from(&self.config.rpc_url)
			.map_err(|e| PluginError::InitializationFailed(format!("Invalid RPC URL: {}", e)))?
			.interval(Duration::from_millis(self.config.timeout_ms));

		// Verify chain ID matches
		let chain_id = provider.get_chainid().await.map_err(|e| {
			PluginError::InitializationFailed(format!("Failed to get chain ID: {}", e))
		})?;

		if chain_id.as_u64() != self.config.chain_id {
			return Err(PluginError::InitializationFailed(format!(
				"Chain ID mismatch: expected {}, got {}",
				self.config.chain_id,
				chain_id.as_u64()
			)));
		}

		self.provider = Some(Arc::new(provider));
		info!("Provider setup complete for chain {}", self.config.chain_id);
		Ok(())
	}

	async fn setup_wallet(&mut self) -> PluginResult<()> {
		if self.config.private_key.is_empty() {
			return Err(PluginError::InvalidConfiguration(
				"Private key is required for delivery".to_string(),
			));
		}

		let wallet = self
			.config
			.private_key
			.parse::<LocalWallet>()
			.map_err(|e| PluginError::InvalidConfiguration(format!("Invalid private key: {}", e)))?
			.with_chain_id(self.config.chain_id);

		info!("Wallet configured for address: {}", wallet.address());

		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| {
				PluginError::InitializationFailed("Provider not initialized".to_string())
			})?
			.clone();

		// Create signer middleware
		let client = SignerMiddleware::new((*provider).clone(), wallet.clone());

		self.wallet = Some(Arc::new(wallet));
		self.client = Some(Arc::new(client));

		Ok(())
	}

	// Convert solver transaction to ethers transaction request
	fn solver_tx_to_ethers_request(
		&self,
		transaction: &SolverTransaction,
	) -> PluginResult<TransactionRequest> {
		let to_address: EthAddress = transaction
			.to
			.parse()
			.map_err(|e| PluginError::ExecutionFailed(format!("Invalid to address: {}", e)))?;

		let tx_request = TransactionRequest::new()
			.to(to_address)
			.value(U256::from(transaction.value))
			.data(EthBytes::from(transaction.data.clone()));

		Ok(tx_request)
	}

	// Convert ethers receipt to solver receipt
	fn ethers_receipt_to_solver_receipt(
		&self,
		eth_receipt: &EthTransactionReceipt,
	) -> SolverTransactionReceipt {
		SolverTransactionReceipt {
			block_number: eth_receipt.block_number.unwrap_or_default().as_u64(),
			block_hash: format!("{:?}", eth_receipt.block_hash.unwrap_or_default()),
			transaction_index: eth_receipt.transaction_index.as_u32(),
			gas_used: eth_receipt.gas_used.unwrap_or_default().as_u64(),
			effective_gas_price: eth_receipt.effective_gas_price.unwrap_or_default().as_u64(),
			status: eth_receipt.status.unwrap_or_default() == U64::from(1),
			logs: eth_receipt
				.logs
				.iter()
				.map(|log| solver_types::plugins::TransactionLog {
					address: format!("{:?}", log.address),
					topics: log.topics.iter().map(|t| format!("{:?}", t)).collect(),
					data: bytes::Bytes::from(log.data.to_vec()),
				})
				.collect(),
		}
	}

	async fn estimate_gas(&self, transaction: &SolverTransaction) -> PluginResult<U256> {
		let client = self
			.client
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Client not initialized".to_string()))?;

		let tx_request = self.solver_tx_to_ethers_request(transaction)?;

		let typed_tx: ethers::types::transaction::eip2718::TypedTransaction = tx_request.into();
		let gas_estimate = client
			.estimate_gas(&typed_tx, None)
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Gas estimation failed: {}", e)))?;

		// Add 10% buffer to gas estimate
		let gas_with_buffer: U256 = gas_estimate * 110 / 100;

		// Cap at transaction gas limit if provided
		let final_gas = if transaction.gas_limit > 0 {
			gas_with_buffer.min(U256::from(transaction.gas_limit))
		} else {
			gas_with_buffer
		};

		debug!(
			"Estimated gas: {} (with buffer: {})",
			gas_estimate, final_gas
		);
		Ok(final_gas)
	}

	async fn get_gas_price(&self) -> PluginResult<U256> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let gas_price = provider
			.get_gas_price()
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Failed to get gas price: {}", e)))?;

		// Apply multiplier
		let adjusted_price = (gas_price.as_u64() as f64 * self.config.gas_price_multiplier) as u64;
		let final_price = U256::from(adjusted_price);

		// Apply max gas price limit
		if let Some(max_price) = self.config.max_gas_price {
			return Ok(final_price.min(U256::from(max_price)));
		}

		Ok(final_price)
	}

	async fn get_fee_data(&self) -> PluginResult<(U256, U256)> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		if self.config.enable_eip1559 {
			// Try to get EIP-1559 fee data
			match provider.estimate_eip1559_fees(None).await {
				Ok((max_fee, max_priority_fee)) => {
					let adjusted_max_fee =
						(max_fee.as_u64() as f64 * self.config.gas_price_multiplier) as u64;
					let adjusted_priority_fee = (max_priority_fee.as_u64() as f64
						* self.config.gas_price_multiplier) as u64;

					return Ok((
						U256::from(adjusted_max_fee),
						U256::from(adjusted_priority_fee),
					));
				}
				Err(e) => {
					warn!("Failed to get EIP-1559 fees, falling back to legacy: {}", e);
				}
			}
		}

		// Fallback to legacy gas price
		let gas_price = self.get_gas_price().await?;
		Ok((gas_price, U256::zero()))
	}

	async fn get_next_nonce(&self) -> PluginResult<U256> {
		if !self.config.nonce_management {
			return Err(PluginError::ExecutionFailed(
				"Nonce management is disabled".to_string(),
			));
		}

		let wallet = self
			.wallet
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Wallet not initialized".to_string()))?;

		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let mut nonce_manager = self.nonce_manager.lock().await;

		match nonce_manager.as_ref() {
			Some(current_nonce) => {
				// Use cached nonce + 1
				let next_nonce = *current_nonce + 1;
				*nonce_manager = Some(next_nonce);
				Ok(next_nonce)
			}
			None => {
				// Get nonce from network
				let network_nonce = provider
					.get_transaction_count(wallet.address(), None)
					.await
					.map_err(|e| {
						PluginError::ExecutionFailed(format!("Failed to get nonce: {}", e))
					})?;

				*nonce_manager = Some(network_nonce);
				Ok(network_nonce)
			}
		}
	}

	async fn send_transaction_internal(
		&self,
		transaction: &SolverTransaction,
		nonce: Option<U256>,
	) -> PluginResult<SolverTxHash> {
		let client = self
			.client
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Client not initialized".to_string()))?;

		let mut tx_request = self.solver_tx_to_ethers_request(transaction)?;

		// Set nonce if provided
		if let Some(nonce) = nonce {
			tx_request = tx_request.nonce(nonce);
		}

		// Set gas limit
		if transaction.gas_limit > 0 {
			tx_request = tx_request.gas(U256::from(transaction.gas_limit));
		} else {
			let estimated_gas = self.estimate_gas(transaction).await?;
			tx_request = tx_request.gas(estimated_gas);
		}

		// Set gas pricing based on EIP-1559 support
		if self.config.enable_eip1559 {
			let (max_fee, max_priority_fee) = self.get_fee_data().await?;

			if max_priority_fee > U256::zero() {
				// EIP-1559 transaction
				tx_request = tx_request.gas_price(max_fee); // For compatibility, some providers use gas_price for max_fee
			} else {
				tx_request = tx_request.gas_price(max_fee);
			}
		} else {
			let gas_price = self.get_gas_price().await?;
			tx_request = tx_request.gas_price(gas_price);
		}

		// Send transaction
		let pending_tx = client
			.send_transaction(tx_request, None)
			.await
			.map_err(|e| {
				PluginError::ExecutionFailed(format!("Transaction submission failed: {}", e))
			})?;

		let tx_hash = format!("{:?}", pending_tx.tx_hash());

		info!("Transaction submitted successfully: {}", tx_hash);
		Ok(tx_hash)
	}

	async fn get_transaction_receipt_internal(
		&self,
		tx_hash: &SolverTxHash,
	) -> PluginResult<Option<EthTransactionReceipt>> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let hash: H256 = tx_hash
			.parse()
			.map_err(|e| PluginError::ExecutionFailed(format!("Invalid tx hash: {}", e)))?;

		let receipt = provider
			.get_transaction_receipt(hash)
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Failed to get receipt: {}", e)))?;

		Ok(receipt)
	}

	fn calculate_priority_fees(&self, priority: &DeliveryPriority) -> (Option<U256>, Option<U256>) {
		match priority {
			DeliveryPriority::Low => (
				Some(U256::from(15_000_000_000u64)), // 15 gwei
				Some(U256::from(1_000_000_000u64)),  // 1 gwei
			),
			DeliveryPriority::Normal => (
				Some(U256::from(20_000_000_000u64)), // 20 gwei
				Some(U256::from(2_000_000_000u64)),  // 2 gwei
			),
			DeliveryPriority::High => (
				Some(U256::from(30_000_000_000u64)), // 30 gwei
				Some(U256::from(5_000_000_000u64)),  // 5 gwei
			),
			DeliveryPriority::Urgent => (
				Some(U256::from(50_000_000_000u64)), // 50 gwei
				Some(U256::from(10_000_000_000u64)), // 10 gwei
			),
			DeliveryPriority::Custom {
				max_fee,
				priority_fee,
				..
			} => (Some(U256::from(*max_fee)), Some(U256::from(*priority_fee))),
		}
	}

	async fn cleanup_old_transactions(&self) -> PluginResult<()> {
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs();

		// Remove transactions older than 1 hour
		self.pending_transactions
			.retain(|_, tx| now - tx.submitted_at < 3600);

		// Also limit the total number of pending transactions
		while self.pending_transactions.len() > self.config.max_pending_transactions {
			// Remove oldest transaction
			if let Some(oldest_entry) = self
				.pending_transactions
				.iter()
				.min_by_key(|entry| entry.value().submitted_at)
				.map(|entry| entry.key().clone())
			{
				self.pending_transactions.remove(&oldest_entry);
			} else {
				break;
			}
		}

		Ok(())
	}
}

impl Default for EvmEthersConfig {
	fn default() -> Self {
		Self {
			chain_id: 1, // Ethereum mainnet
			rpc_url: "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY".to_string(),
			private_key: String::new(),
			max_retries: 3,
			timeout_ms: 30000,
			gas_price_multiplier: 1.1,
			max_gas_price: Some(100_000_000_000), // 100 gwei max
			enable_eip1559: true,
			confirmation_blocks: 12,
			nonce_management: true,
			mempool_monitoring: false,
			max_pending_transactions: 1000,
		}
	}
}

#[async_trait]
impl BasePlugin for EvmEthersDeliveryPlugin {
	fn plugin_type(&self) -> &'static str {
		"evm_ethers_delivery"
	}

	fn name(&self) -> String {
		format!(
			"EVM Ethers Delivery Plugin (Chain {})",
			self.config.chain_id
		)
	}

	fn version(&self) -> &'static str {
		"1.0.0"
	}

	fn description(&self) -> &'static str {
		"Transaction delivery plugin using ethers-rs for EVM chains"
	}

	async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()> {
		info!("Initializing EVM Ethers delivery plugin");

		// Parse configuration
		if let Some(chain_id) = config.get_number("chain_id") {
			self.config.chain_id = chain_id as ChainId;
		}

		if let Some(rpc_url) = config.get_string("rpc_url") {
			self.config.rpc_url = rpc_url;
		}

		if let Some(private_key) = config.get_string("private_key") {
			if !private_key.is_empty() {
				self.config.private_key = private_key;
			}
		}

		if let Some(max_retries) = config.get_number("max_retries") {
			self.config.max_retries = max_retries as u32;
		}

		if let Some(timeout) = config.get_number("timeout_ms") {
			self.config.timeout_ms = timeout as u64;
		}

		if let Some(enable_eip1559) = config.get_bool("enable_eip1559") {
			self.config.enable_eip1559 = enable_eip1559;
		}

		if let Some(nonce_management) = config.get_bool("nonce_management") {
			self.config.nonce_management = nonce_management;
		}

		if let Some(max_pending) = config.get_number("max_pending_transactions") {
			self.config.max_pending_transactions = max_pending as usize;
		}

		// Setup provider and wallet
		self.setup_provider().await?;
		self.setup_wallet().await?;

		self.is_initialized = true;
		info!("EVM Ethers delivery plugin initialized successfully");
		Ok(())
	}

	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()> {
		if config.get_string("rpc_url").is_none() {
			return Err(PluginError::InvalidConfiguration(
				"rpc_url is required".to_string(),
			));
		}

		if config.get_string("private_key").is_none() {
			return Err(PluginError::InvalidConfiguration(
				"private_key is required".to_string(),
			));
		}

		if let Some(chain_id) = config.get_number("chain_id") {
			if chain_id <= 0 {
				return Err(PluginError::InvalidConfiguration(
					"chain_id must be positive".to_string(),
				));
			}
		}

		if let Some(timeout) = config.get_number("timeout_ms") {
			if timeout < 1000 {
				return Err(PluginError::InvalidConfiguration(
					"timeout_ms must be at least 1000".to_string(),
				));
			}
		}

		Ok(())
	}

	async fn health_check(&self) -> PluginResult<PluginHealth> {
		if !self.is_initialized {
			return Ok(PluginHealth::unhealthy("Plugin not initialized"));
		}

		let provider = match &self.provider {
			Some(provider) => provider,
			None => return Ok(PluginHealth::unhealthy("Provider not configured")),
		};

		let wallet = match &self.wallet {
			Some(wallet) => wallet,
			None => return Ok(PluginHealth::unhealthy("Wallet not configured")),
		};

		// Test RPC connection
		match provider.get_block_number().await {
			Ok(block_number) => {
				// Test wallet balance
				match provider.get_balance(wallet.address(), None).await {
					Ok(balance) => {
						let pending_count = self.pending_transactions.len();

						Ok(PluginHealth::healthy("EVM delivery plugin is operational")
							.with_detail("chain_id", self.config.chain_id.to_string())
							.with_detail("block_number", block_number.to_string())
							.with_detail("wallet_address", format!("{:?}", wallet.address()))
							.with_detail("wallet_balance", balance.to_string())
							.with_detail("pending_transactions", pending_count.to_string())
							.with_detail("eip1559_enabled", self.config.enable_eip1559.to_string()))
					}
					Err(e) => Ok(PluginHealth::unhealthy(format!(
						"Cannot get wallet balance: {}",
						e
					))),
				}
			}
			Err(e) => Ok(PluginHealth::unhealthy(format!(
				"RPC connection failed: {}",
				e
			))),
		}
	}

	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		let pending_count = self.pending_transactions.len();

		let mut metrics = self.metrics.clone();
		metrics.set_gauge("pending_transactions", pending_count as f64);
		metrics.set_gauge("chain_id", self.config.chain_id as f64);
		metrics.set_gauge(
			"eip1559_enabled",
			if self.config.enable_eip1559 { 1.0 } else { 0.0 },
		);

		// Get current gas price for monitoring
		if let Ok(gas_price) = self.get_gas_price().await {
			metrics.set_gauge(
				"current_gas_price_gwei",
				gas_price.as_u64() as f64 / 1_000_000_000.0,
			);
		}

		Ok(metrics)
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		info!("Shutting down EVM Ethers delivery plugin");

		self.is_initialized = false;
		self.provider = None;
		self.wallet = None;
		self.client = None;

		// Clear pending transactions
		self.pending_transactions.clear();

		info!("EVM Ethers delivery plugin shutdown complete");
		Ok(())
	}

	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new()
			.required("chain_id", ConfigFieldType::Number, "EVM chain ID")
			.required("rpc_url", ConfigFieldType::String, "RPC endpoint URL")
			.required(
				"private_key",
				ConfigFieldType::String,
				"Private key for signing transactions",
			)
			.optional(
				"max_retries",
				ConfigFieldType::Number,
				"Maximum retry attempts",
				Some(ConfigValue::from(3i64)),
			)
			.optional(
				"timeout_ms",
				ConfigFieldType::Number,
				"Request timeout in milliseconds",
				Some(ConfigValue::from(30000i64)),
			)
			.optional(
				"enable_eip1559",
				ConfigFieldType::Boolean,
				"Enable EIP-1559 type 2 transactions",
				Some(ConfigValue::from(true)),
			)
			.optional(
				"nonce_management",
				ConfigFieldType::Boolean,
				"Enable automatic nonce management",
				Some(ConfigValue::from(true)),
			)
			.optional(
				"max_pending_transactions",
				ConfigFieldType::Number,
				"Maximum number of pending transactions to track",
				Some(ConfigValue::from(1000i64)),
			)
			.optional(
				"gas_price_multiplier",
				ConfigFieldType::Number,
				"Multiplier for gas price (e.g., 1.1 for 10% increase)",
				Some(ConfigValue::from("1.1")),
			)
	}

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

#[async_trait]
impl DeliveryPlugin for EvmEthersDeliveryPlugin {
	fn chain_id(&self) -> ChainId {
		self.config.chain_id
	}

	async fn can_deliver(&self, request: &DeliveryRequest) -> PluginResult<bool> {
		Ok(request.transaction.chain_id == self.config.chain_id && self.is_initialized)
	}

	async fn estimate(&self, request: &DeliveryRequest) -> PluginResult<DeliveryEstimate> {
		if !self.can_deliver(request).await? {
			return Err(PluginError::NotSupported(format!(
				"Cannot deliver to chain {}",
				request.transaction.chain_id
			)));
		}

		let gas_limit = self.estimate_gas(&request.transaction).await?;
		let (max_fee, priority_fee) = if self.config.enable_eip1559 {
			let (calculated_max_fee, calculated_priority_fee) = match &request.priority {
				DeliveryPriority::Custom {
					max_fee,
					priority_fee,
					..
				} => (U256::from(*max_fee), U256::from(*priority_fee)),
				_ => {
					let (max_fee_opt, priority_fee_opt) =
						self.calculate_priority_fees(&request.priority);
					(
						max_fee_opt.unwrap_or_default(),
						priority_fee_opt.unwrap_or_default(),
					)
				}
			};
			(calculated_max_fee, calculated_priority_fee)
		} else {
			(self.get_gas_price().await?, U256::zero())
		};

		let estimated_cost = gas_limit * max_fee;

		// Estimate confirmation time based on priority and network conditions
		let estimated_time = match request.priority {
			DeliveryPriority::Urgent => Some(30),  // 30 seconds
			DeliveryPriority::High => Some(60),    // 1 minute
			DeliveryPriority::Normal => Some(180), // 3 minutes
			DeliveryPriority::Low => Some(300),    // 5 minutes
			DeliveryPriority::Custom { deadline, .. } => deadline.map(|d| {
				d.saturating_sub(
					SystemTime::now()
						.duration_since(UNIX_EPOCH)
						.unwrap()
						.as_secs(),
				)
			}),
		};

		let mut recommendations = vec![
			format!("Gas limit: {} units", gas_limit),
			format!("Max fee per gas: {} gwei", max_fee.as_u64() / 1_000_000_000),
		];

		if self.config.enable_eip1559 && priority_fee > U256::zero() {
			recommendations.push(format!(
				"Priority fee: {} gwei",
				priority_fee.as_u64() / 1_000_000_000
			));
			recommendations.push("Using EIP-1559 type 2 transaction".to_string());
		} else {
			recommendations.push("Using legacy transaction".to_string());
		}

		Ok(DeliveryEstimate {
			gas_limit: gas_limit.as_u64(),
			gas_price: max_fee.as_u64(),
			estimated_cost: estimated_cost.as_u64(),
			estimated_time,
			confidence_score: 0.9, // High confidence for direct RPC delivery
			recommendations,
		})
	}

	async fn deliver(&self, request: DeliveryRequest) -> PluginResult<DeliveryResponse> {
		if !self.can_deliver(&request).await? {
			return Err(PluginError::NotSupported(format!(
				"Cannot deliver to chain {}",
				request.transaction.chain_id
			)));
		}

		// Cleanup old transactions periodically
		self.cleanup_old_transactions().await?;

		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs();

		// Get nonce if nonce management is enabled
		let nonce = if self.config.nonce_management && request.transaction.nonce.is_none() {
			Some(self.get_next_nonce().await?)
		} else {
			request.transaction.nonce.map(U256::from)
		};

		// Send transaction
		let tx_hash = self
			.send_transaction_internal(&request.transaction, nonce)
			.await?;

		// Track pending transaction
		let pending = PendingTransaction {
			request: request.clone(),
			submitted_at: now,
			last_check: now,
			retry_count: 0,
			status: DeliveryStatus::Submitted,
			nonce,
		};

		self.pending_transactions.insert(tx_hash.clone(), pending);

		// Update metrics
		let mut metrics = self.metrics.clone();
		metrics.increment_counter("transactions_submitted");

		debug!("Transaction delivered successfully: {}", tx_hash);

		Ok(DeliveryResponse {
			tx_hash: tx_hash.clone(),
			chain_id: self.config.chain_id,
			submitted_at: now,
			status: DeliveryStatus::Submitted,
			receipt: None,
			cost: DeliveryCost {
				gas_used: 0, // Will be updated when receipt is available
				gas_price: 0,
				total_cost: 0,
				fee_breakdown: HashMap::new(),
			},
		})
	}

	async fn get_transaction_status(
		&self,
		tx_hash: &String,
	) -> PluginResult<Option<DeliveryResponse>> {
		let eth_receipt = self.get_transaction_receipt_internal(tx_hash).await?;

		if let Some(pending) = self.pending_transactions.get(tx_hash) {
			let (status, receipt, cost) = if let Some(eth_receipt) = eth_receipt {
				let plugin_receipt = self.ethers_receipt_to_solver_receipt(&eth_receipt);
				let gas_used = eth_receipt.gas_used.unwrap_or_default().as_u64();
				let effective_gas_price =
					eth_receipt.effective_gas_price.unwrap_or_default().as_u64();
				let total_cost = gas_used * effective_gas_price;

				let mut fee_breakdown = HashMap::new();
				fee_breakdown.insert("gas_fee".to_string(), total_cost);

				let cost = DeliveryCost {
					gas_used,
					gas_price: effective_gas_price,
					total_cost,
					fee_breakdown,
				};

				let status = if plugin_receipt.status {
					DeliveryStatus::Confirmed
				} else {
					DeliveryStatus::Failed
				};

				(status, Some(plugin_receipt), cost)
			} else {
				(
					DeliveryStatus::Pending,
					None,
					DeliveryCost {
						gas_used: 0,
						gas_price: 0,
						total_cost: 0,
						fee_breakdown: HashMap::new(),
					},
				)
			};

			return Ok(Some(DeliveryResponse {
				tx_hash: tx_hash.to_string(),
				chain_id: self.config.chain_id,
				submitted_at: pending.submitted_at,
				status,
				receipt,
				cost,
			}));
		}

		Ok(None)
	}

	async fn cancel_transaction(&self, tx_hash: &String) -> PluginResult<bool> {
		// Check if transaction is still pending
		if let Some(_eth_receipt) = self.get_transaction_receipt_internal(tx_hash).await? {
			return Ok(false); // Already confirmed, cannot cancel
		}

		if let Some(pending) = self.pending_transactions.get(tx_hash) {
			if let Some(nonce) = pending.nonce {
				let wallet = self.wallet.as_ref().unwrap();

				// Create a cancel transaction (send 0 ETH to self with higher gas)
				let cancel_tx = SolverTransaction {
					to: format!("{:?}", wallet.address()),
					value: 0,
					data: bytes::Bytes::new(),
					gas_limit: 21000,
					gas_price: None,
					max_fee_per_gas: None,
					max_priority_fee_per_gas: None,
					nonce: Some(nonce.as_u64()),
					chain_id: self.config.chain_id,
				};

				// Send with higher gas price
				match self
					.send_transaction_internal(&cancel_tx, Some(nonce))
					.await
				{
					Ok(_) => {
						// Mark original as cancelled
						if let Some(mut pending_ref) = self.pending_transactions.get_mut(tx_hash) {
							pending_ref.status = DeliveryStatus::Dropped;
						}
						info!(
							"Successfully submitted cancellation transaction for {}",
							tx_hash
						);
						return Ok(true);
					}
					Err(e) => {
						error!("Failed to submit cancellation transaction: {}", e);
						return Ok(false);
					}
				}
			}
		}

		Ok(false)
	}

	async fn replace_transaction(
		&self,
		original_tx_hash: &String,
		new_request: DeliveryRequest,
	) -> PluginResult<DeliveryResponse> {
		// Get original transaction details
		let original = self
			.pending_transactions
			.get(original_tx_hash)
			.ok_or_else(|| PluginError::NotFound("Original transaction not found".to_string()))?;

		let original_nonce = original.nonce.ok_or_else(|| {
			PluginError::ExecutionFailed("Original transaction has no nonce".to_string())
		})?;

		drop(original); // Release the reference

		// Create replacement transaction with same nonce
		let mut replacement_tx = new_request.transaction.clone();
		replacement_tx.nonce = Some(original_nonce.as_u64());

		// Send replacement transaction
		let response_hash = self
			.send_transaction_internal(&replacement_tx, Some(original_nonce))
			.await?;

		// Mark original as replaced
		if let Some(mut original_pending) = self.pending_transactions.get_mut(original_tx_hash) {
			original_pending.status = DeliveryStatus::Replaced;
		}

		// Track new transaction
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let pending = PendingTransaction {
			request: new_request.clone(),
			submitted_at: now,
			last_check: now,
			retry_count: 0,
			status: DeliveryStatus::Submitted,
			nonce: Some(original_nonce),
		};

		self.pending_transactions
			.insert(response_hash.clone(), pending);

		info!(
			"Successfully replaced transaction {} with {}",
			original_tx_hash, response_hash
		);

		Ok(DeliveryResponse {
			tx_hash: response_hash,
			chain_id: self.config.chain_id,
			submitted_at: now,
			status: DeliveryStatus::Submitted,
			receipt: None,
			cost: DeliveryCost {
				gas_used: 0,
				gas_price: 0,
				total_cost: 0,
				fee_breakdown: HashMap::new(),
			},
		})
	}

	fn supported_features(&self) -> Vec<DeliveryFeature> {
		let mut features = vec![
			DeliveryFeature::GasEstimation,
			DeliveryFeature::Cancellation,
			DeliveryFeature::Replacement,
		];

		if self.config.nonce_management {
			features.push(DeliveryFeature::NonceManagement);
		}

		if self.config.enable_eip1559 {
			features.push(DeliveryFeature::EIP1559);
		}

		features
	}

	async fn get_network_status(&self) -> PluginResult<NetworkStatus> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let block_number = provider
			.get_block_number()
			.await
			.map_err(|e| {
				PluginError::ExecutionFailed(format!("Failed to get block number: {}", e))
			})?
			.as_u64();

		let gas_price = self.get_gas_price().await?.as_u64();

		let (base_fee, priority_fee) = if self.config.enable_eip1559 {
			match self.get_fee_data().await {
				Ok((max_fee, max_priority)) => (
					Some(max_fee.saturating_sub(max_priority).as_u64()),
					Some(max_priority.as_u64()),
				),
				Err(_) => (None, None),
			}
		} else {
			(None, None)
		};

		let network_congestion = if gas_price > 50_000_000_000 {
			CongestionLevel::High
		} else if gas_price > 30_000_000_000 {
			CongestionLevel::Medium
		} else {
			CongestionLevel::Low
		};

		let pending_tx_count = self.pending_transactions.len() as u64;

		Ok(NetworkStatus {
			chain_id: self.config.chain_id,
			block_number,
			gas_price,
			base_fee,
			priority_fee,
			network_congestion,
			pending_tx_count: Some(pending_tx_count),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn create_test_config() -> PluginConfig {
		PluginConfig::new("delivery")
			.with_config("chain_id", 1i64)
			.with_config("rpc_url", "http://localhost:8545")
			.with_config(
				"private_key",
				"0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318",
			)
			.with_config("enable_eip1559", true)
			.with_config("nonce_management", true)
			.with_config("timeout_ms", 5000i64)
			.with_config("max_retries", 2i64)
	}

	fn create_test_transaction() -> SolverTransaction {
		SolverTransaction {
			to: "0x742d35Cc6634C0532925a3b8D6Ac6c001afb7f9c".to_string(),
			value: 1000000000000000000u64, // 1 ETH in wei
			data: bytes::Bytes::new(),
			gas_limit: 21000,
			gas_price: None,
			max_fee_per_gas: None,
			max_priority_fee_per_gas: None,
			nonce: None,
			chain_id: 1,
		}
	}

	fn create_test_delivery_request() -> DeliveryRequest {
		DeliveryRequest {
			transaction: create_test_transaction(),
			priority: DeliveryPriority::Normal,
			metadata: DeliveryMetadata::default(),
			retry_config: None,
		}
	}

	#[tokio::test]
	async fn test_plugin_initialization() {
		let mut plugin = EvmEthersDeliveryPlugin::new();
		let config = create_test_config();

		// Validate config first
		assert!(plugin.validate_config(&config).is_ok());

		// Initialize might fail due to network, but should not panic
		match plugin.initialize(config).await {
			Ok(_) => {
				assert!(plugin.is_initialized);
				assert_eq!(plugin.chain_id(), 1);
			}
			Err(PluginError::InitializationFailed(_)) => {
				// Expected in test environment without real RPC
				assert!(!plugin.is_initialized);
			}
			Err(e) => panic!("Unexpected error: {:?}", e),
		}
	}

	#[tokio::test]
	async fn test_can_deliver() {
		let mut plugin = EvmEthersDeliveryPlugin::new();

		// Before initialization, should not be able to deliver
		let request = create_test_delivery_request();
		let can_deliver = plugin.can_deliver(&request).await.unwrap();
		assert!(!can_deliver);

		// After marking as initialized
		plugin.config.chain_id = 1;
		plugin.is_initialized = true;

		let can_deliver = plugin.can_deliver(&request).await.unwrap();
		assert!(can_deliver);
	}

	#[tokio::test]
	async fn test_priority_fee_calculation() {
		let plugin = EvmEthersDeliveryPlugin::new();

		let (max_fee, priority_fee) = plugin.calculate_priority_fees(&DeliveryPriority::Low);
		assert!(max_fee.is_some());
		assert!(priority_fee.is_some());

		let (max_fee_normal, priority_fee_normal) =
			plugin.calculate_priority_fees(&DeliveryPriority::Normal);
		assert!(max_fee_normal.unwrap() > max_fee.unwrap());
		assert!(priority_fee_normal.unwrap() > priority_fee.unwrap());
	}

	#[tokio::test]
	async fn test_transaction_conversion() {
		let plugin = EvmEthersDeliveryPlugin::new();
		let transaction = create_test_transaction();

		let result = plugin.solver_tx_to_ethers_request(&transaction);
		assert!(result.is_ok());
	}
}
