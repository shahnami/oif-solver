//! Ethers-based EVM chain adapter implementation.
//!
//! This module provides an adapter for interacting with EVM-compatible blockchains
//! using the ethers-rs library. The adapter supports standard JSON-RPC operations
//! including querying blockchain state, submitting transactions, and monitoring events.
//!
//! The adapter includes built-in retry logic for handling transient network failures
//! and validates chain IDs to ensure connection to the correct network.

use async_trait::async_trait;
use ethers::{
	providers::{Http, Middleware, Provider},
	signers::{LocalWallet, Signer},
	types::{transaction::eip2718::TypedTransaction, Filter, Log as EthersLog},
};
use solver_types::{
	chains::{ChainAdapter, ChainId, Log, Transaction, TransactionReceipt},
	common::{Address, BlockNumber, Bytes32, TxHash, U256},
	errors::{Result, SolverError},
};
use std::{str::FromStr, sync::Arc};
use tracing::{debug, error, info};

use crate::utils::RetryClient;

/// Gas pricing strategy for EVM transactions.
#[derive(Debug, Clone)]
pub enum GasStrategy {
	/// Use the network's standard gas price recommendation.
	Standard,
	/// Use a higher gas price for faster inclusion.
	Fast,
	/// Apply a custom multiplier to the base gas price.
	Custom { multiplier: f64 },
	/// Use EIP-1559 dynamic fee mechanism.
	Eip1559 { max_priority_fee: u64 },
}

/// EVM chain adapter using the ethers-rs library.
///
/// This adapter provides a high-level interface for interacting with Ethereum
/// and EVM-compatible blockchains. It wraps an ethers Provider with retry logic
/// and handles type conversions between solver types and ethers types.
///
/// The adapter validates the chain ID on connection to prevent accidental
/// connections to the wrong network.
pub struct EthersAdapter {
	chain_id: ChainId,
	provider: Arc<Provider<RetryClient<Http>>>,
	confirmations: u64,
	wallet: Option<LocalWallet>,
	gas_strategy: GasStrategy,
	from_address: Address,
}

/// Builder for creating EthersAdapter instances.
pub struct EthersAdapterBuilder {
	chain_id: ChainId,
	endpoint: String,
	confirmations: u64,
	wallet: Option<LocalWallet>,
	gas_strategy: GasStrategy,
	from_address: Address,
	max_retries: u32,
}

impl EthersAdapter {
	/// Creates a new EthersAdapter builder.
	pub fn builder(
		chain_id: ChainId,
		endpoint: &str,
		confirmations: u64,
		wallet: Option<LocalWallet>,
		gas_strategy: GasStrategy,
		from_address: Address,
	) -> EthersAdapterBuilder {
		EthersAdapterBuilder {
			chain_id,
			endpoint: endpoint.to_string(),
			confirmations,
			wallet,
			gas_strategy,
			from_address,
			max_retries: 3, // Default retry count
		}
	}

	/// Creates a new EthersAdapter instance with custom retry configuration.
	///
	/// This method is used internally and for backward compatibility.
	pub async fn with_max_retries(
		chain_id: ChainId,
		endpoint: &str,
		confirmations: u64,
		wallet: Option<LocalWallet>,
		gas_strategy: GasStrategy,
		from_address: Address,
		max_retries: u32,
	) -> Result<Self> {
		Self::create_adapter(
			chain_id,
			endpoint,
			confirmations,
			wallet,
			gas_strategy,
			from_address,
			max_retries,
		)
		.await
	}

	/// Creates a new EthersAdapter instance with the specified configuration.
	async fn create_adapter(
		chain_id: ChainId,
		endpoint: &str,
		confirmations: u64,
		wallet: Option<LocalWallet>,
		gas_strategy: GasStrategy,
		from_address: Address,
		max_retries: u32,
	) -> Result<Self> {
		info!(
			"Creating Ethers adapter for chain {} at {} with max_retries={}",
			chain_id, endpoint, max_retries
		);

		// Create HTTP provider with retry logic
		let http_client = Http::from_str(endpoint)
			.map_err(|e| SolverError::Chain(format!("Failed to create HTTP client: {}", e)))?;

		let retry_client = RetryClient::new(http_client).with_max_retries(max_retries);
		let provider = Arc::new(Provider::new(retry_client));

		// Verify chain ID matches
		let actual_chain_id = provider
			.get_chainid()
			.await
			.map_err(|e| SolverError::Chain(format!("Failed to get chain ID: {}", e)))?;

		if actual_chain_id != chain_id.0.into() {
			return Err(SolverError::Chain(format!(
				"Chain ID mismatch: expected {}, got {}",
				chain_id.0, actual_chain_id
			)));
		}

		Ok(Self {
			chain_id,
			provider,
			confirmations,
			wallet,
			gas_strategy,
			from_address,
		})
	}

	/// Converts a solver Transaction to an ethers TransactionRequest.
	///
	/// Maps all available transaction fields including optional gas parameters
	/// and nonce. The resulting transaction request can be used with ethers
	/// provider methods.
	fn to_ethers_tx(tx: &Transaction) -> ethers::types::TransactionRequest {
		let mut eth_tx = ethers::types::TransactionRequest::new()
			.to(tx.to)
			.value(tx.value)
			.data(tx.data.clone());

		if let Some(gas) = tx.gas_limit {
			eth_tx = eth_tx.gas(gas);
		}

		if let Some(gas_price) = tx.gas_price {
			eth_tx = eth_tx.gas_price(gas_price);
		}

		if let Some(nonce) = tx.nonce {
			eth_tx = eth_tx.nonce(nonce);
		}

		eth_tx
	}

	/// Converts an ethers Log to a solver Log.
	///
	/// Extracts all log fields and handles missing optional values by using
	/// appropriate defaults (0 for missing block numbers and indices).
	fn from_ethers_log(log: EthersLog) -> Log {
		Log {
			address: log.address,
			topics: log.topics,
			data: log.data.to_vec(),
			block_number: log.block_number.unwrap_or_default().as_u64(),
			transaction_hash: log.transaction_hash.unwrap_or_default(),
			log_index: log.log_index.unwrap_or_default().as_u64(),
		}
	}

	/// Calculates gas price based on the configured gas strategy.
	///
	/// # Errors
	///
	/// Returns an error if gas price estimation fails.
	async fn calculate_gas_price(&self) -> Result<U256> {
		let base_gas_price = self
			.provider
			.get_gas_price()
			.await
			.map_err(|e| SolverError::Chain(format!("Failed to get gas price: {}", e)))?;

		let gas_price = match &self.gas_strategy {
			GasStrategy::Standard => base_gas_price,
			GasStrategy::Fast => {
				// Apply 1.2x multiplier for fast transactions
				base_gas_price * 12 / 10
			}
			GasStrategy::Custom { multiplier } => {
				let multiplier_scaled = (*multiplier * 1000.0) as u64;
				base_gas_price * multiplier_scaled / 1000
			}
			GasStrategy::Eip1559 { max_priority_fee } => {
				// For EIP-1559, we need to get the base fee and add priority fee
				// For now, we'll use a simple approach similar to Fast
				base_gas_price + U256::from(*max_priority_fee)
			}
		};

		debug!(
			"Calculated gas price: {} (strategy: {:?})",
			gas_price, self.gas_strategy
		);

		Ok(gas_price)
	}
}

impl EthersAdapterBuilder {
	/// Sets the maximum number of retry attempts.
	pub fn with_max_retries(mut self, max_retries: u32) -> Self {
		self.max_retries = max_retries;
		self
	}

	/// Builds the EthersAdapter instance.
	pub async fn build(self) -> Result<EthersAdapter> {
		EthersAdapter::create_adapter(
			self.chain_id,
			&self.endpoint,
			self.confirmations,
			self.wallet,
			self.gas_strategy,
			self.from_address,
			self.max_retries,
		)
		.await
	}
}

#[async_trait]
impl ChainAdapter for EthersAdapter {
	/// Returns the chain ID of this adapter.
	fn chain_id(&self) -> ChainId {
		self.chain_id
	}

	/// Returns the number of confirmations required for transaction finality.
	fn confirmations(&self) -> u64 {
		self.confirmations
	}

	/// Retrieves the current block number from the chain.
	///
	/// # Errors
	///
	/// Returns an error if the RPC call fails or the node is unreachable.
	async fn get_block_number(&self) -> Result<BlockNumber> {
		debug!("Getting block number for chain {}", self.chain_id);

		self.provider
			.get_block_number()
			.await
			.map(|n| n.as_u64())
			.map_err(|e| SolverError::Chain(format!("Failed to get block number: {}", e)))
	}

	/// Retrieves the block timestamp for a given block number.
	///
	/// # Arguments
	///
	/// * `block_number` - The block number to get the timestamp for
	///
	/// # Errors
	///
	/// Returns an error if the RPC call fails or the block doesn't exist.
	async fn get_block_timestamp(&self, block_number: BlockNumber) -> Result<u64> {
		debug!(
			"Getting block timestamp for block {} on chain {}",
			block_number, self.chain_id
		);

		let block = self
			.provider
			.get_block(block_number)
			.await
			.map_err(|e| SolverError::Chain(format!("Failed to get block: {}", e)))?;

		match block {
			Some(b) => Ok(b.timestamp.as_u64()),
			None => Err(SolverError::Chain(format!(
				"Block {} not found",
				block_number
			))),
		}
	}

	/// Retrieves the native token balance for an address.
	///
	/// # Arguments
	///
	/// * `address` - The address to query the balance for
	///
	/// # Errors
	///
	/// Returns an error if the RPC call fails or the address is invalid.
	async fn get_balance(&self, address: Address) -> Result<U256> {
		debug!("Getting balance for {} on chain {}", address, self.chain_id);

		self.provider
			.get_balance(address, None)
			.await
			.map_err(|e| SolverError::Chain(format!("Failed to get balance: {}", e)))
	}

	/// Submits a transaction to the network.
	///
	/// Signs the transaction using the configured wallet and submits it to the network.
	/// Gas price is calculated based on the configured gas strategy.
	///
	/// # Arguments
	///
	/// * `tx` - The transaction to submit
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - No wallet is configured for signing
	/// - Gas price estimation fails
	/// - Transaction submission fails
	async fn submit_transaction(&self, tx: Transaction) -> Result<TxHash> {
		debug!("Submitting transaction on chain {}", self.chain_id);

		let wallet = self.wallet.as_ref().ok_or_else(|| {
			SolverError::Chain("No wallet configured for transaction signing".to_string())
		})?;

		let mut eth_tx = Self::to_ethers_tx(&tx);

		// Set from address
		eth_tx = eth_tx.from(self.from_address);

		// Set chain_id for EIP-155 compatibility
		eth_tx = eth_tx.chain_id(self.chain_id.0);

		// Calculate and set gas price if not provided
		if eth_tx.gas_price.is_none() {
			let gas_price = self.calculate_gas_price().await?;
			eth_tx = eth_tx.gas_price(gas_price);
		}

		// Estimate gas if not provided
		if eth_tx.gas.is_none() {
			let gas_estimate = self
				.provider
				.estimate_gas(&eth_tx.clone().into(), None)
				.await
				.map_err(|e| SolverError::Chain(format!("Gas estimation failed: {}", e)))?;
			eth_tx = eth_tx.gas(gas_estimate);
		}

		// Get nonce if not provided
		if eth_tx.nonce.is_none() {
			let nonce = self
				.provider
				.get_transaction_count(self.from_address, None)
				.await
				.map_err(|e| SolverError::Chain(format!("Failed to get nonce: {}", e)))?;
			eth_tx = eth_tx.nonce(nonce);
		}

		// Sign and send transaction
		let typed_tx: TypedTransaction = eth_tx.into();
		debug!("Constructed transaction: {:?}", typed_tx);

		let signature = wallet
			.sign_transaction(&typed_tx)
			.await
			.map_err(|e| SolverError::Chain(format!("Transaction signing failed: {}", e)))?;
		debug!("Transaction signed successfully");

		let signed_tx = typed_tx.rlp_signed(&signature);
		debug!("Signed transaction RLP length: {} bytes", signed_tx.len());
		debug!(
			"Signed transaction RLP (first 100 bytes): {:?}",
			&signed_tx[..std::cmp::min(100, signed_tx.len())]
		);

		debug!("Submitting raw transaction to provider...");
		let pending_tx = self
			.provider
			.send_raw_transaction(signed_tx.clone())
			.await
			.map_err(|e| {
				error!("Transaction submission failed: {}", e);
				error!("Failed transaction RLP: {:?}", signed_tx);
				SolverError::Chain(format!("Transaction submission failed: {}", e))
			})?;
		debug!("send_raw_transaction returned successfully");

		let tx_hash = pending_tx.tx_hash();
		info!(
			"Transaction submitted successfully: {} (full hash: 0x{:x})",
			tx_hash, tx_hash
		);

		// Verify the transaction exists in the node
		debug!("Verifying transaction exists in node...");
		let tx_exists = self.provider.get_transaction(tx_hash).await;
		match tx_exists {
			Ok(Some(tx)) => {
				debug!("Transaction found in node: {:?}", tx);
				debug!(
					"Transaction status: hash={}, nonce={:?}, gas_price={:?}",
					tx.hash, tx.nonce, tx.gas_price
				);
			}
			Ok(None) => {
				error!("Transaction not found in node immediately after submission");
				error!("This indicates the transaction was rejected but no error was returned");
			}
			Err(e) => error!("Error checking transaction existence: {}", e),
		}

		// Also check the mempool
		debug!("Checking mempool status...");
		if let Ok(response) = self
			.provider
			.request::<_, serde_json::Value>("txpool_status", ())
			.await
		{
			debug!("Mempool status: {:?}", response);
		}

		Ok(tx_hash)
	}

	/// Retrieves a transaction receipt by its hash.
	///
	/// # Arguments
	///
	/// * `tx_hash` - The transaction hash to query
	///
	/// # Returns
	///
	/// * `Ok(Some(receipt))` - If the transaction was mined
	/// * `Ok(None)` - If the transaction is not yet mined or doesn't exist
	/// * `Err(_)` - If the RPC call fails
	async fn get_transaction_receipt(&self, tx_hash: TxHash) -> Result<Option<TransactionReceipt>> {
		debug!(
			"Getting receipt for tx {} on chain {}",
			tx_hash, self.chain_id
		);

		let receipt = self
			.provider
			.get_transaction_receipt(tx_hash)
			.await
			.map_err(|e| {
				error!(
					"Failed to get receipt for tx {} on chain {}: {}",
					tx_hash, self.chain_id, e
				);
				SolverError::Chain(format!("Failed to get receipt: {}", e))
			})?;

		match &receipt {
			Some(r) => {
				debug!(
                    "Receipt found for tx {} on chain {}: block_number={:?}, status={:?}, gas_used={:?}",
                    tx_hash, self.chain_id, r.block_number, r.status, r.gas_used
                );
			}
			None => {
				debug!(
					"No receipt found for tx {} on chain {} (transaction may not be mined yet)",
					tx_hash, self.chain_id
				);
			}
		}

		Ok(receipt.map(|r| TransactionReceipt {
			transaction_hash: r.transaction_hash,
			block_number: r.block_number.unwrap_or_default().as_u64(),
			gas_used: r.gas_used.unwrap_or_default(),
			status: r.status.map(|s| s.as_u64() == 1).unwrap_or(false),
			timestamp: None, // Will be populated later if needed
		}))
	}

	/// Executes a call to a smart contract without creating a transaction.
	///
	/// This is used for read-only contract interactions and gas estimation.
	///
	/// # Arguments
	///
	/// * `tx` - The transaction data to execute
	/// * `block` - Optional block number to execute the call at (defaults to latest)
	///
	/// # Returns
	///
	/// The raw bytes returned by the contract call.
	///
	/// # Errors
	///
	/// Returns an error if the call reverts or the RPC call fails.
	async fn call(&self, tx: Transaction, block: Option<BlockNumber>) -> Result<Vec<u8>> {
		debug!("Calling contract on chain {}", self.chain_id);

		let eth_tx = Self::to_ethers_tx(&tx);
		let typed_tx: TypedTransaction = eth_tx.into();
		let block = block.map(|b| b.into());

		self.provider
			.call(&typed_tx, block)
			.await
			.map(|bytes| bytes.to_vec())
			.map_err(|e| SolverError::Chain(format!("Failed to call: {}", e)))
	}

	/// Retrieves logs matching the specified filter criteria.
	///
	/// # Arguments
	///
	/// * `address` - Optional contract address to filter logs from
	/// * `topics` - Topic filters (up to 4 topics, None means any value)
	/// * `from_block` - Starting block number (inclusive)
	/// * `to_block` - Ending block number (inclusive)
	///
	/// # Returns
	///
	/// A vector of logs matching the filter criteria.
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The block range is too large (provider-specific limit)
	/// - The RPC call fails
	async fn get_logs(
		&self,
		address: Option<Address>,
		topics: Vec<Option<Bytes32>>,
		from_block: BlockNumber,
		to_block: BlockNumber,
	) -> Result<Vec<Log>> {
		debug!("Getting logs from chain {}", self.chain_id);

		let mut filter = Filter::new().from_block(from_block).to_block(to_block);

		if let Some(addr) = address {
			filter = filter.address(addr);
		}

		// Set topics
		for (i, topic) in topics.iter().enumerate() {
			if let Some(t) = topic {
				match i {
					0 => filter = filter.topic0(*t),
					1 => filter = filter.topic1(*t),
					2 => filter = filter.topic2(*t),
					3 => filter = filter.topic3(*t),
					_ => {}
				}
			}
		}

		self.provider
			.get_logs(&filter)
			.await
			.map(|logs| logs.into_iter().map(Self::from_ethers_log).collect())
			.map_err(|e| SolverError::Chain(format!("Failed to get logs: {}", e)))
	}

	/// Estimates gas required for a transaction.
	///
	/// Uses the provider's gas estimation with the transaction parameters.
	/// The from address is set from the adapter's configured address.
	///
	/// # Arguments
	///
	/// * `tx` - The transaction to estimate gas for
	///
	/// # Returns
	///
	/// The estimated gas units required for the transaction.
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The transaction would revert
	/// - The RPC call fails
	async fn estimate_gas(&self, tx: &Transaction) -> Result<U256> {
		debug!("Estimating gas for transaction on chain {}", self.chain_id);

		let mut eth_tx = Self::to_ethers_tx(tx);

		// Set from address for gas estimation
		eth_tx = eth_tx.from(self.from_address);

		// Perform gas estimation
		let gas_estimate = self
			.provider
			.estimate_gas(&eth_tx.into(), None)
			.await
			.map_err(|e| SolverError::Chain(format!("Gas estimation failed: {}", e)))?;

		debug!(
			"Gas estimation complete: {} units for chain {}",
			gas_estimate, self.chain_id
		);

		Ok(gas_estimate)
	}

	/// Gets the current gas price according to the configured strategy.
	///
	/// This method uses the adapter's gas strategy configuration to determine
	/// the appropriate gas price for transactions.
	///
	/// # Returns
	///
	/// The gas price in wei.
	///
	/// # Errors
	///
	/// Returns an error if the RPC call to get gas price fails.
	async fn get_gas_price(&self) -> Result<U256> {
		self.calculate_gas_price().await
	}
}
