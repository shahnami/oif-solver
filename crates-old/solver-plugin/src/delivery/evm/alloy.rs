//! # Alloy-based EVM Delivery Plugin
//!
//! Provides transaction delivery for EVM-compatible blockchains using Alloy.
//!
//! This plugin implements transaction submission, monitoring, and management for
//! Ethereum and EVM-compatible chains using the Alloy library. It supports
//! features like EIP-1559, nonce management, gas price optimization, and
//! transaction status tracking.

use alloy::network::{Ethereum, EthereumWallet, TransactionBuilder};
use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{TransactionReceipt, TransactionRequest};
use alloy::signers::local::PrivateKeySigner;
use async_trait::async_trait;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use solver_types::plugins::*;
use std::any::Any;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

/// Utility function to truncate hashes for display purposes.
fn truncate_hash(hash: &str) -> String {
	if hash.len() <= 12 {
		hash.to_string()
	} else {
		format!("{}...{}", &hash[..6], &hash[hash.len() - 4..])
	}
}

// Type aliases to resolve naming conflicts between alloy and solver types
type SolverTransaction = solver_types::plugins::Transaction;
type SolverTxHash = String;
type SolverTransactionReceipt = solver_types::plugins::TransactionReceipt;

/// EVM delivery plugin implementation using Alloy library.
///
/// Manages transaction submission and monitoring for EVM-compatible blockchains
/// with support for advanced features like gas price management, nonce tracking,
/// retry logic, and transaction replacement strategies.
pub struct EvmAlloyDeliveryPlugin {
	/// Plugin configuration parameters
	config: EvmAlloyConfig,
	/// Alloy provider with transaction filling capabilities
	provider: Option<Box<dyn Provider<Ethereum>>>,
	/// Performance metrics tracking
	metrics: PluginMetrics,
	/// Initialization status flag
	is_initialized: bool,
	/// Cache of pending transactions being monitored
	pending_transactions: Arc<DashMap<SolverTxHash, PendingTransaction>>,
}

impl fmt::Debug for EvmAlloyDeliveryPlugin {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("EvmAlloyDeliveryPlugin")
			.field("config", &self.config)
			.field("is_initialized", &self.is_initialized)
			.field("provider", &"<Provider>")
			.field("metrics", &self.metrics)
			.field("pending_transactions", &self.pending_transactions.len())
			.finish()
	}
}

/// Configuration for the EVM Alloy delivery plugin.
///
/// Defines connection parameters, transaction settings, and operational
/// preferences for interacting with EVM-compatible blockchains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmAlloyConfig {
	/// Target blockchain network ID
	pub chain_id: ChainId,
	/// JSON-RPC endpoint URL
	pub rpc_url: String,
	/// Private key for transaction signing (should be secured)
	pub private_key: String,
	/// Maximum retry attempts for failed transactions
	pub max_retries: u32,
	/// Transaction timeout in milliseconds
	pub timeout_ms: u64,
	/// Multiplier for gas price adjustments (e.g., 1.1 for 10% increase)
	pub gas_price_multiplier: f64,
	/// Maximum allowed gas price in wei
	pub max_gas_price: Option<u64>,
	/// Whether to use EIP-1559 transaction format
	pub enable_eip1559: bool,
	/// Number of blocks to wait for transaction confirmation
	pub confirmation_blocks: u32,
	/// Whether to manage nonces internally
	pub nonce_management: bool,
	/// Maximum number of pending transactions to track
	pub max_pending_transactions: usize,
}

/// Represents a transaction that has been submitted but not yet confirmed.
///
/// Tracks the submission time, current status, and nonce for pending
/// transactions to enable monitoring and potential replacement.
#[derive(Debug, Clone)]
struct PendingTransaction {
	/// Unix timestamp when the transaction was submitted
	pub submitted_at: Timestamp,
	/// Current delivery status of the transaction
	pub _status: DeliveryStatus,
}

impl Default for EvmAlloyDeliveryPlugin {
	fn default() -> Self {
		Self::new()
	}
}

impl EvmAlloyDeliveryPlugin {
	/// Create a new EVM Alloy delivery plugin with default configuration.
	pub fn new() -> Self {
		Self {
			config: EvmAlloyConfig::default(),
			provider: None,
			metrics: PluginMetrics::new(),
			is_initialized: false,
			pending_transactions: Arc::new(DashMap::new()),
		}
	}

	/// Create a new EVM Alloy delivery plugin with custom configuration.
	///
	/// # Arguments
	/// * `config` - Configuration parameters for the plugin
	pub fn with_config(config: EvmAlloyConfig) -> Self {
		Self {
			config,
			provider: None,
			metrics: PluginMetrics::new(),
			is_initialized: false,
			pending_transactions: Arc::new(DashMap::new()),
		}
	}

	async fn setup_provider(&mut self) -> PluginResult<()> {
		debug!(
			"Setting up alloy provider for chain {}",
			self.config.chain_id
		);

		if self.config.private_key.is_empty() {
			return Err(PluginError::InvalidConfiguration(
				"Private key is required for delivery".to_string(),
			));
		}

		// Parse private key and create wallet
		let signer = self
			.config
			.private_key
			.parse::<PrivateKeySigner>()
			.map_err(|e| {
				PluginError::InvalidConfiguration(format!("Invalid private key: {}", e))
			})?;

		debug!("Wallet configured for address: {}", signer.address());
		let wallet = EthereumWallet::from(signer);

		// Build provider with all necessary layers
		let provider = ProviderBuilder::new()
			.with_gas_estimation()
			.with_cached_nonce_management()
			.with_chain_id(self.config.chain_id)
			.wallet(wallet)
			.connect_http(self.config.rpc_url.parse().map_err(|e| {
				PluginError::InvalidConfiguration(format!("Invalid RPC URL: {}", e))
			})?);

		// Verify chain ID matches
		let chain_id = provider.get_chain_id().await.map_err(|e| {
			PluginError::InitializationFailed(format!("Failed to get chain ID: {}", e))
		})?;

		if chain_id != self.config.chain_id {
			return Err(PluginError::InitializationFailed(format!(
				"Chain ID mismatch: expected {}, got {}",
				self.config.chain_id, chain_id
			)));
		}

		self.provider = Some(Box::new(provider));
		Ok(())
	}

	// Convert solver transaction to alloy transaction request
	fn solver_tx_to_alloy_request(
		&self,
		transaction: &SolverTransaction,
	) -> PluginResult<TransactionRequest> {
		let to_address: Address = transaction
			.to
			.parse()
			.map_err(|e| PluginError::ExecutionFailed(format!("Invalid to address: {}", e)))?;

		debug!(
			"Building transaction request to: {:?}, value: {}, data_len: {}",
			to_address,
			transaction.value,
			transaction.data.len()
		);

		let mut tx_request = TransactionRequest::default()
			.with_to(to_address)
			.with_value(U256::from(transaction.value))
			.with_input(transaction.data.clone());

		// Set chain ID
		tx_request = tx_request.with_chain_id(self.config.chain_id);

		Ok(tx_request)
	}

	// Convert alloy receipt to solver receipt
	fn alloy_receipt_to_solver_receipt(
		&self,
		alloy_receipt: &TransactionReceipt,
	) -> SolverTransactionReceipt {
		// In Alloy, TransactionReceipt has a status() method that returns bool
		let status = alloy_receipt.status();

		SolverTransactionReceipt {
			block_number: alloy_receipt.block_number.unwrap_or(0),
			block_hash: format!("{:?}", alloy_receipt.block_hash.unwrap_or_default()),
			transaction_index: alloy_receipt.transaction_index.unwrap_or(0) as u32,
			gas_used: alloy_receipt.gas_used,
			effective_gas_price: alloy_receipt.effective_gas_price,
			status,
			logs: vec![],
		}
	}

	async fn estimate_gas(&self, transaction: &SolverTransaction) -> PluginResult<U256> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let tx_request = self.solver_tx_to_alloy_request(transaction)?;

		let gas_estimate = provider
			.estimate_gas(tx_request)
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Gas estimation failed: {}", e)))?;

		// Add 10% buffer to gas estimate
		let gas_with_buffer = U256::from(gas_estimate) * U256::from(110) / U256::from(100);

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
		// Convert gas_price (u128) to u64 safely
		let gas_price_u64 = gas_price.min(u128::from(u64::MAX)) as u64;
		let adjusted_price =
			U256::from((gas_price_u64 as f64 * self.config.gas_price_multiplier) as u64);

		// Apply max gas price limit
		if let Some(max_price) = self.config.max_gas_price {
			return Ok(adjusted_price.min(U256::from(max_price)));
		}

		Ok(adjusted_price)
	}

	async fn get_fee_data(&self) -> PluginResult<(U256, U256)> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		if self.config.enable_eip1559 {
			// Try to get EIP-1559 fee data
			match provider.estimate_eip1559_fees().await {
				Ok(fees) => {
					// Convert fees (u128) to u64 safely
					let max_fee_u64 = fees.max_fee_per_gas.min(u128::from(u64::MAX)) as u64;
					let priority_fee_u64 =
						fees.max_priority_fee_per_gas.min(u128::from(u64::MAX)) as u64;
					let adjusted_max_fee =
						U256::from((max_fee_u64 as f64 * self.config.gas_price_multiplier) as u64);
					let adjusted_priority_fee = U256::from(
						(priority_fee_u64 as f64 * self.config.gas_price_multiplier) as u64,
					);

					return Ok((adjusted_max_fee, adjusted_priority_fee));
				}
				Err(e) => {
					warn!("Failed to get EIP-1559 fees, falling back to legacy: {}", e);
				}
			}
		}

		// Fallback to legacy gas price
		let gas_price = self.get_gas_price().await?;
		Ok((gas_price, U256::ZERO))
	}

	async fn send_transaction_internal(
		&self,
		transaction: &SolverTransaction,
		nonce: Option<U256>,
	) -> PluginResult<SolverTxHash> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let mut tx_request = self.solver_tx_to_alloy_request(transaction)?;

		// Set nonce if explicitly provided
		if let Some(nonce) = nonce {
			tx_request = tx_request.with_nonce(nonce.to::<u64>());
			debug!("Set explicit nonce: {}", nonce);
		}

		// Set gas limit if specified, otherwise let the provider estimate
		if transaction.gas_limit > 0 {
			tx_request = tx_request.with_gas_limit(transaction.gas_limit);
			debug!("Set gas limit: {}", transaction.gas_limit);
		} else {
			// Estimate gas if not provided
			let estimated_gas = self.estimate_gas(transaction).await?;
			tx_request = tx_request.with_gas_limit(estimated_gas.to::<u64>());
			debug!("Using estimated gas: {}", estimated_gas);
		}

		// Set gas pricing if custom values are provided
		if let Some(gas_price) = transaction.gas_price {
			tx_request = tx_request.with_gas_price(gas_price.into());
			debug!("Set gas price: {}", gas_price);
		} else if let (Some(max_fee), Some(priority_fee)) = (
			transaction.max_fee_per_gas,
			transaction.max_priority_fee_per_gas,
		) {
			tx_request = tx_request
				.with_max_fee_per_gas(max_fee.into())
				.with_max_priority_fee_per_gas(priority_fee.into());
			debug!(
				"Set EIP-1559 fees - max: {}, priority: {}",
				max_fee, priority_fee
			);
		} else if self.config.enable_eip1559 {
			// Let the provider handle EIP-1559 pricing with our custom multipliers
			let (max_fee, max_priority_fee) = self.get_fee_data().await?;
			if max_priority_fee > U256::ZERO {
				tx_request = tx_request
					.with_max_fee_per_gas(max_fee.to::<u128>())
					.with_max_priority_fee_per_gas(max_priority_fee.to::<u128>());
				debug!(
					"Set calculated EIP-1559 fees - max: {}, priority: {}",
					max_fee, max_priority_fee
				);
			}
		}

		debug!("Sending transaction with request: {:?}", tx_request);

		// Send transaction using provider which handles filling and signing
		let pending_tx = provider.send_transaction(tx_request).await.map_err(|e| {
			error!("Transaction submission failed: {:?}", e);
			PluginError::ExecutionFailed(format!("Transaction submission failed: {}", e))
		})?;

		let tx_hash = format!("{:?}", pending_tx.tx_hash());

		info!(
			"Transaction submitted successfully: {}",
			truncate_hash(&tx_hash)
		);
		Ok(tx_hash)
	}

	async fn get_transaction_receipt_internal(
		&self,
		tx_hash: &SolverTxHash,
	) -> PluginResult<Option<TransactionReceipt>> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let hash: FixedBytes<32> = tx_hash
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

impl Default for EvmAlloyConfig {
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
			max_pending_transactions: 1000,
		}
	}
}

#[async_trait]
impl BasePlugin for EvmAlloyDeliveryPlugin {
	fn plugin_type(&self) -> &'static str {
		"evm_alloy_delivery"
	}

	fn name(&self) -> String {
		format!("EVM Alloy Delivery Plugin (Chain {})", self.config.chain_id)
	}

	fn version(&self) -> &'static str {
		"1.0.0"
	}

	fn description(&self) -> &'static str {
		"Transaction delivery plugin using Alloy for EVM chains"
	}

	async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()> {
		debug!("Initializing EVM Alloy delivery plugin");
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

		if let Some(gas_price_multiplier) = config.get_number("gas_price_multiplier") {
			self.config.gas_price_multiplier = gas_price_multiplier as f64;
		}

		if let Some(max_gas_price) = config.get_number("max_gas_price") {
			self.config.max_gas_price = Some(max_gas_price as u64);
		}

		if let Some(enable_eip1559) = config.get_bool("enable_eip1559") {
			self.config.enable_eip1559 = enable_eip1559;
		}

		if let Some(confirmation_blocks) = config.get_number("confirmation_blocks") {
			self.config.confirmation_blocks = confirmation_blocks as u32;
		}

		if let Some(nonce_management) = config.get_bool("nonce_management") {
			self.config.nonce_management = nonce_management;
		}

		if let Some(max_pending) = config.get_number("max_pending_transactions") {
			self.config.max_pending_transactions = max_pending as usize;
		}

		// Setup provider with wallet
		self.setup_provider().await?;

		self.is_initialized = true;
		Ok(())
	}

	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()> {
		// Use schema validation
		let schema = self.config_schema();
		schema.validate(config)?;

		// Additional custom validation
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

		if let Some(gas_price_multiplier) = config.get_number("gas_price_multiplier") {
			if gas_price_multiplier <= 0 {
				return Err(PluginError::InvalidConfiguration(
					"gas_price_multiplier must be positive".to_string(),
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

		// Test RPC connection
		match provider.get_block_number().await {
			Ok(block_number) => {
				let pending_count = self.pending_transactions.len();

				Ok(PluginHealth::healthy("EVM delivery plugin is operational")
					.with_detail("chain_id", self.config.chain_id.to_string())
					.with_detail("block_number", block_number.to_string())
					.with_detail("pending_transactions", pending_count.to_string())
					.with_detail("eip1559_enabled", self.config.enable_eip1559.to_string()))
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
				gas_price.to::<u64>() as f64 / 1_000_000_000.0,
			);
		}

		Ok(metrics)
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		info!("Shutting down EVM Alloy delivery plugin");

		self.is_initialized = false;
		self.provider = None;

		// Clear pending transactions
		self.pending_transactions.clear();

		info!("EVM Alloy delivery plugin shutdown complete");
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
				"gas_price_multiplier",
				ConfigFieldType::Number,
				"Multiplier for gas price (e.g., 1.1 for 10% increase)",
				Some(ConfigValue::from("1.1")),
			)
			.optional(
				"max_gas_price",
				ConfigFieldType::Number,
				"Maximum allowed gas price in wei",
				None,
			)
			.optional(
				"enable_eip1559",
				ConfigFieldType::Boolean,
				"Enable EIP-1559 type 2 transactions",
				Some(ConfigValue::from(true)),
			)
			.optional(
				"confirmation_blocks",
				ConfigFieldType::Number,
				"Number of blocks to wait for transaction confirmation",
				Some(ConfigValue::from(0i64)),
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
	}

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

#[async_trait]
impl DeliveryPlugin for EvmAlloyDeliveryPlugin {
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
						max_fee_opt.unwrap_or(U256::ZERO),
						priority_fee_opt.unwrap_or(U256::ZERO),
					)
				}
			};
			(calculated_max_fee, calculated_priority_fee)
		} else {
			(self.get_gas_price().await?, U256::ZERO)
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
			format!(
				"Max fee per gas: {} gwei",
				max_fee.to::<u64>() / 1_000_000_000
			),
		];

		if self.config.enable_eip1559 && priority_fee > U256::ZERO {
			recommendations.push(format!(
				"Priority fee: {} gwei",
				priority_fee.to::<u64>() / 1_000_000_000
			));
			recommendations.push("Using EIP-1559 type 2 transaction".to_string());
		} else {
			recommendations.push("Using legacy transaction".to_string());
		}

		Ok(DeliveryEstimate {
			gas_limit: gas_limit.to::<u64>(),
			gas_price: max_fee.to::<u64>(),
			estimated_cost: estimated_cost.to::<u64>(),
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

		// Use provided nonce if available
		let nonce = request.transaction.nonce.map(U256::from);

		// Send transaction
		let tx_hash = self
			.send_transaction_internal(&request.transaction, nonce)
			.await?;

		// Track pending transaction
		let pending = PendingTransaction {
			submitted_at: now,
			_status: DeliveryStatus::Submitted,
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
		let alloy_receipt = self.get_transaction_receipt_internal(tx_hash).await?;

		if let Some(pending) = self.pending_transactions.get(tx_hash) {
			let (status, receipt, cost) = if let Some(alloy_receipt) = alloy_receipt {
				let plugin_receipt = self.alloy_receipt_to_solver_receipt(&alloy_receipt);
				let gas_used = alloy_receipt.gas_used;
				let effective_gas_price = alloy_receipt.effective_gas_price;
				let total_cost = gas_used as u128 * effective_gas_price;

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

	async fn cancel_transaction(&self, _tx_hash: &String) -> PluginResult<bool> {
		unimplemented!("Cancelling a transaction not supported for this plugin")
	}

	async fn replace_transaction(
		&self,
		_original_tx_hash: &String,
		_new_request: DeliveryRequest,
	) -> PluginResult<DeliveryResponse> {
		unimplemented!("Replacing a transaction not supported for this plugin")
	}

	fn supported_features(&self) -> Vec<DeliveryFeature> {
		let mut features = vec![DeliveryFeature::GasEstimation];

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

		let block_number = provider.get_block_number().await.map_err(|e| {
			PluginError::ExecutionFailed(format!("Failed to get block number: {}", e))
		})?;

		let gas_price = self.get_gas_price().await?.to::<u64>();

		let (base_fee, priority_fee) = if self.config.enable_eip1559 {
			match self.get_fee_data().await {
				Ok((max_fee, max_priority)) => (
					Some(max_fee.saturating_sub(max_priority).to::<u64>()),
					Some(max_priority.to::<u64>()),
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
		let mut plugin = EvmAlloyDeliveryPlugin::new();
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
		let mut plugin = EvmAlloyDeliveryPlugin::new();

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
		let plugin = EvmAlloyDeliveryPlugin::new();

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
		let plugin = EvmAlloyDeliveryPlugin::new();
		let transaction = create_test_transaction();

		let result = plugin.solver_tx_to_alloy_request(&transaction);
		assert!(result.is_ok());
	}
}
