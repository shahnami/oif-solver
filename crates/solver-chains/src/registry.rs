//! Registry for managing chain adapters.
//!
//! The `ChainRegistry` provides a centralized place to register and retrieve
//! blockchain adapters. This allows the solver to work with multiple chains
//! simultaneously and dynamically add support for new chains at runtime.
//!
//! # Thread Safety
//!
//! The registry itself is not thread-safe. If you need to share it across
//! threads, wrap it in an appropriate synchronization primitive (e.g., `Arc<Mutex<_>>`).
//! The adapters stored in the registry are already wrapped in `Arc` for safe sharing.

use crate::implementations::evm::{EthersAdapter, GasStrategy};
use solver_types::common::Address;
use solver_types::{
	chains::{ChainAdapter, ChainId},
	errors::{Result, SolverError},
};
use std::{collections::HashMap, fmt, sync::Arc};
use tracing::{debug, info};

/// Registry for managing different chain adapters.
///
/// The registry maintains a collection of chain adapters indexed by their chain ID.
/// Each adapter is stored as an `Arc<dyn ChainAdapter>` to allow sharing across
/// multiple components of the solver.
pub struct ChainRegistry {
	adapters: HashMap<ChainId, Arc<dyn ChainAdapter>>,
}

impl ChainRegistry {
	/// Creates a new empty registry.
	pub fn new() -> Self {
		Self {
			adapters: HashMap::new(),
		}
	}

	/// Registers a chain adapter in the registry.
	///
	/// # Arguments
	///
	/// * `adapter` - The adapter to register, already wrapped in `Arc`
	///
	/// # Errors
	///
	/// Returns an error if an adapter for the same chain ID is already registered.
	pub fn register(&mut self, adapter: Arc<dyn ChainAdapter>) -> Result<()> {
		let chain_id = adapter.chain_id();
		info!("Registering chain adapter for chain {}", chain_id);

		if self.adapters.contains_key(&chain_id) {
			return Err(SolverError::Config(format!(
				"Chain {} already registered",
				chain_id
			)));
		}

		self.adapters.insert(chain_id, adapter);
		Ok(())
	}

	/// Retrieves an adapter for a specific chain.
	///
	/// # Arguments
	///
	/// * `chain_id` - The ID of the chain to get the adapter for
	///
	/// # Returns
	///
	/// * `Some(adapter)` if the chain is registered
	/// * `None` if the chain is not registered
	pub fn get(&self, chain_id: &ChainId) -> Option<Arc<dyn ChainAdapter>> {
		self.adapters.get(chain_id).cloned()
	}

	/// Retrieves an adapter for a specific chain, returning an error if not found.
	///
	/// This is a convenience method that returns a proper error instead of `None`
	/// when the chain is not registered.
	///
	/// # Arguments
	///
	/// * `chain_id` - The ID of the chain to get the adapter for
	///
	/// # Errors
	///
	/// Returns `SolverError::Chain` if the chain is not registered.
	pub fn get_required(&self, chain_id: &ChainId) -> Result<Arc<dyn ChainAdapter>> {
		self.get(chain_id)
			.ok_or_else(|| SolverError::Chain(format!("Chain {} not configured", chain_id)))
	}

	/// Returns a list of all registered chain IDs.
	///
	/// The order of the returned chain IDs is not guaranteed.
	pub fn chains(&self) -> Vec<ChainId> {
		self.adapters.keys().cloned().collect()
	}

	/// Creates a registry with default chain adapters based on provided configurations.
	///
	/// This factory method automatically creates and registers adapters for the chains
	/// specified in the RPC endpoints map. Currently supports EVM chains
	///
	/// # Arguments
	///
	/// * `rpc_endpoints` - Map of chain IDs to their RPC endpoint URLs
	/// * `confirmations` - Map of chain IDs to their required confirmation counts (defaults to 1 if not specified)
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - Any adapter fails to connect to its RPC endpoint
	/// - The connected chain ID doesn't match the expected chain ID
	/// - Registration of any adapter fails
	pub async fn with_defaults(
		rpc_endpoints: HashMap<ChainId, String>,
		confirmations: HashMap<ChainId, u64>,
	) -> Result<Self> {
		let mut registry = Self::new();

		for (chain_id, endpoint) in rpc_endpoints {
			debug!("Connecting to chain {} at {}", chain_id, endpoint);
			let chain_confirmations = confirmations.get(&chain_id).copied().unwrap_or(1);

			{
				// For now, create adapters without wallet support (read-only)
				let adapter = EthersAdapter::builder(
					chain_id,
					&endpoint,
					chain_confirmations,
					None, // No wallet for read-only operations
					GasStrategy::Standard,
					Address::zero(), // Placeholder address
				)
				.build()
				.await?;
				registry.register(Arc::new(adapter))?;
			}
		}

		Ok(registry)
	}

	/// Creates a registry with chain adapters that support transaction signing.
	///
	/// This method creates adapters with wallet support for transaction signing,
	/// using the provided configuration for gas strategy and from address.
	///
	/// # Arguments
	///
	/// * `rpc_endpoints` - Map of chain IDs to their RPC endpoint URLs
	/// * `confirmations` - Map of chain IDs to their required confirmation counts
	/// * `wallet` - Optional wallet for transaction signing (if None, adapters will be read-only)
	/// * `gas_strategy` - Gas pricing strategy to use
	/// * `from_address` - Address to use for transactions
	/// * `max_retries` - Maximum number of retry attempts for RPC calls
	///
	/// # Errors
	///
	/// Returns an error if adapter creation or registration fails.
	pub async fn with_signing_support(
		rpc_endpoints: HashMap<ChainId, String>,
		confirmations: HashMap<ChainId, u64>,
		wallet: Option<ethers::signers::LocalWallet>,
		gas_strategy: crate::implementations::evm::GasStrategy,
		from_address: solver_types::common::Address,
		max_retries: u32,
	) -> Result<Self> {
		let mut registry = Self::new();

		for (chain_id, endpoint) in rpc_endpoints {
			debug!(
				"Connecting to chain {} at {} with signing support",
				chain_id, endpoint
			);
			let chain_confirmations = confirmations.get(&chain_id).copied().unwrap_or(1);

			{
				let adapter = EthersAdapter::builder(
					chain_id,
					&endpoint,
					chain_confirmations,
					wallet.clone(),
					gas_strategy.clone(),
					from_address,
				)
				.with_max_retries(max_retries)
				.build()
				.await?;
				registry.register(Arc::new(adapter))?;
			}
		}

		Ok(registry)
	}
}

impl Default for ChainRegistry {
	fn default() -> Self {
		Self::new()
	}
}

impl fmt::Debug for ChainRegistry {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("ChainRegistry")
			.field("adapters", &self.adapters.keys().collect::<Vec<_>>())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use async_trait::async_trait;
	use solver_types::{
		chains::{Log, Transaction, TransactionReceipt},
		common::{Address, BlockNumber, Bytes32, TxHash, U256},
	};

	// Mock chain adapter for testing
	#[derive(Debug)]
	struct MockAdapter {
		chain_id: ChainId,
		confirmations: u64,
	}

	#[async_trait]
	impl ChainAdapter for MockAdapter {
		fn chain_id(&self) -> ChainId {
			self.chain_id
		}

		fn confirmations(&self) -> u64 {
			self.confirmations
		}
		async fn get_block_number(&self) -> Result<BlockNumber> {
			Ok(100)
		}
		async fn get_balance(&self, _: Address) -> Result<U256> {
			Ok(U256::zero())
		}
		async fn submit_transaction(&self, _: Transaction) -> Result<TxHash> {
			Err(SolverError::NotImplemented("mock".to_string()))
		}
		async fn get_transaction_receipt(&self, _: TxHash) -> Result<Option<TransactionReceipt>> {
			Ok(None)
		}
		async fn get_logs(
			&self,
			_: Option<Address>,
			_: Vec<Option<Bytes32>>,
			_: BlockNumber,
			_: BlockNumber,
		) -> Result<Vec<Log>> {
			Ok(vec![])
		}
		async fn call(&self, _: Transaction, _: Option<BlockNumber>) -> Result<Vec<u8>> {
			Ok(vec![])
		}
		async fn estimate_gas(&self, _tx: &Transaction) -> Result<U256> {
			Ok(U256::from(100_000))
		}

		async fn get_gas_price(&self) -> Result<U256> {
			Ok(U256::from(20_000_000_000u64)) // 20 gwei
		}

		async fn get_block_timestamp(&self, _block: BlockNumber) -> Result<u64> {
			Ok(0)
		}
	}

	#[test]
	fn test_registry_register_and_get() {
		let mut registry = ChainRegistry::new();
		let adapter = Arc::new(MockAdapter {
			chain_id: ChainId(1),
			confirmations: 1,
		});

		// Register should succeed
		registry.register(adapter.clone()).unwrap();

		// Should be able to get it back
		let retrieved = registry.get(&ChainId(1)).unwrap();
		assert_eq!(retrieved.chain_id(), ChainId(1));

		// Non-existent chain should return None
		assert!(registry.get(&ChainId(2)).is_none());
	}

	#[test]
	fn test_registry_duplicate_registration() {
		let mut registry = ChainRegistry::new();
		let adapter1 = Arc::new(MockAdapter {
			chain_id: ChainId(1),
			confirmations: 1,
		});
		let adapter2 = Arc::new(MockAdapter {
			chain_id: ChainId(1),
			confirmations: 1,
		});

		// First registration should succeed
		registry.register(adapter1).unwrap();

		// Second registration with same chain ID should fail
		let result = registry.register(adapter2);
		assert!(result.is_err());
	}

	#[test]
	fn test_get_required() {
		let mut registry = ChainRegistry::new();
		let adapter = Arc::new(MockAdapter {
			chain_id: ChainId(1),
			confirmations: 1,
		});
		registry.register(adapter).unwrap();

		// Should succeed for registered chain
		let result = registry.get_required(&ChainId(1));
		assert!(result.is_ok());

		// Should fail for non-registered chain
		let result = registry.get_required(&ChainId(2));
		assert!(result.is_err());
	}

	#[test]
	fn test_list_chains() {
		let mut registry = ChainRegistry::new();

		// Empty registry
		assert!(registry.chains().is_empty());

		// Add some chains
		registry
			.register(Arc::new(MockAdapter {
				chain_id: ChainId(1),
				confirmations: 1,
			}))
			.unwrap();
		registry
			.register(Arc::new(MockAdapter {
				chain_id: ChainId(42161),
				confirmations: 1,
			}))
			.unwrap();

		let chains = registry.chains();
		assert_eq!(chains.len(), 2);
		assert!(chains.contains(&ChainId(1)));
		assert!(chains.contains(&ChainId(42161)));
	}
}
