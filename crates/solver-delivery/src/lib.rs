//! Transaction delivery mechanisms for the OIF solver.
//!
//! This crate provides abstractions and implementations for submitting transactions
//! to various blockchain networks. It handles the complexity of gas estimation,
//! nonce management, transaction formatting, and confirmation tracking.
//!
//! # Architecture
//!
//! The delivery system is built around the `DeliveryService` trait, which defines
//! a standard interface for transaction submission. Different implementations can
//! provide various strategies for delivering transactions:
//!
//! - Direct RPC submission for self-managed nodes
//! - Relayer services for gasless transactions
//! - MEV-protected submission channels
//! - Batch transaction submission
//!
//! # Key Components
//!
//! - `DeliveryService`: Core trait defining the delivery interface
//! - `DeliveryConfig`: Configuration for delivery services including endpoints and strategies
//! - `GasStrategy`: Different approaches to gas price calculation
//! - `DeliveryServiceImpl`: Enum wrapper allowing polymorphic use of implementations
//!
//! # Gas Management
//!
//! The crate supports multiple gas pricing strategies:
//! - Standard: Uses the network's suggested gas price
//! - Fast: Applies a multiplier for faster inclusion
//! - Custom: Allows explicit gas price specification
//! - EIP-1559: Dynamic fee market support for compatible chains

pub mod implementations;
pub mod types;

pub use implementations::rpc::RpcDelivery;
pub use types::{DeliveryConfig, GasStrategy};

use async_trait::async_trait;
use solver_types::{
	chains::{ChainId, Transaction, TransactionReceipt},
	common::{Address, TxHash, U256},
	errors::Result,
};

/// Transaction delivery service trait.
///
/// Defines the interface for services that handle transaction submission and
/// monitoring on blockchain networks. Implementations may use different strategies
/// such as direct RPC calls, relayer services, or specialized submission channels.
///
/// All methods are asynchronous to handle network I/O and potential delays in
/// transaction processing. The trait requires `Send + Sync` to allow safe usage
/// across thread boundaries.
#[async_trait]
pub trait DeliveryService: Send + Sync {
	/// Returns the name of this delivery service.
	///
	/// Used for logging, monitoring, and service identification.
	fn name(&self) -> &str;

	/// Checks if this delivery service supports the specified blockchain.
	///
	/// # Arguments
	///
	/// * `chain_id` - The blockchain identifier to check
	///
	/// # Returns
	///
	/// `true` if the service can submit transactions to this chain, `false` otherwise.
	fn supports_chain(&self, chain_id: ChainId) -> bool;

	/// Submits a transaction to the blockchain.
	///
	/// This method handles all aspects of transaction submission including:
	/// - Gas price calculation based on configured strategy
	/// - Nonce assignment if not provided
	/// - Transaction signing (implementation-specific)
	/// - Broadcasting to the network
	///
	/// # Arguments
	///
	/// * `chain_id` - Target blockchain for the transaction
	/// * `tx` - Transaction details to submit
	///
	/// # Returns
	///
	/// The transaction hash upon successful submission
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The chain is not supported
	/// - Gas estimation fails
	/// - Transaction submission fails
	/// - Network connectivity issues occur
	async fn submit_transaction(&self, chain_id: ChainId, tx: Transaction) -> Result<TxHash>;

	/// Waits for a transaction to be confirmed on the blockchain.
	///
	/// Monitors the transaction until it reaches the specified number of
	/// confirmations. This ensures the transaction is sufficiently deep
	/// in the blockchain to be considered final.
	///
	/// # Arguments
	///
	/// * `chain_id` - Blockchain where the transaction was submitted
	/// * `tx_hash` - Hash of the transaction to monitor
	/// * `confirmations` - Required number of block confirmations
	///
	/// # Returns
	///
	/// The transaction receipt once confirmed
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The transaction is not found
	/// - The transaction fails (reverts)
	/// - Timeout occurs while waiting for confirmations
	/// - Network connectivity issues occur
	async fn wait_for_confirmation(
		&self,
		chain_id: ChainId,
		tx_hash: TxHash,
		confirmations: u64,
	) -> Result<TransactionReceipt>;

	/// Estimates the gas required for a transaction.
	///
	/// Simulates the transaction to determine the gas consumption without
	/// actually submitting it to the network.
	///
	/// # Arguments
	///
	/// * `chain_id` - Target blockchain for estimation
	/// * `tx` - Transaction to estimate gas for
	///
	/// # Returns
	///
	/// Estimated gas units required
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The transaction would revert
	/// - Estimation fails due to invalid parameters
	/// - Network connectivity issues occur
	async fn estimate_gas(&self, chain_id: ChainId, tx: &Transaction) -> Result<U256>;

	/// Retrieves the current gas price from the network.
	///
	/// The returned price may be adjusted based on the configured gas strategy
	/// (e.g., multiplied for faster inclusion).
	///
	/// # Arguments
	///
	/// * `chain_id` - Blockchain to query gas price for
	///
	/// # Returns
	///
	/// Current gas price in wei (or chain-specific base unit)
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The chain is not supported
	/// - Network query fails
	async fn get_gas_price(&self, chain_id: ChainId) -> Result<U256>;

	/// Retrieves the next available nonce for an address.
	///
	/// Returns the transaction count for the address, which serves as the
	/// next nonce for transaction submission.
	///
	/// # Arguments
	///
	/// * `chain_id` - Blockchain to query
	/// * `address` - Address to get nonce for
	///
	/// # Returns
	///
	/// The next sequential nonce value
	///
	/// # Errors
	///
	/// Returns an error if:
	/// - The chain is not supported
	/// - Network query fails
	async fn get_nonce(&self, chain_id: ChainId, address: Address) -> Result<U256>;
}

/// Enum wrapper for different delivery service implementations.
///
/// This enum provides a unified interface for various delivery strategies,
/// allowing the solver to switch between different delivery mechanisms
/// based on configuration or runtime conditions.
///
/// Each variant implements the `DeliveryService` trait through delegation,
/// maintaining consistent behavior while allowing implementation-specific
/// optimizations.
#[derive(Clone)]
pub enum DeliveryServiceImpl {
	/// Direct RPC-based transaction delivery
	Rpc(RpcDelivery),
	// Future implementations can be added here
	// OzRelayer(OzRelayerDelivery),
}

#[async_trait]
impl DeliveryService for DeliveryServiceImpl {
	fn name(&self) -> &str {
		match self {
			DeliveryServiceImpl::Rpc(rpc) => rpc.name(),
		}
	}

	fn supports_chain(&self, chain_id: ChainId) -> bool {
		match self {
			DeliveryServiceImpl::Rpc(rpc) => rpc.supports_chain(chain_id),
		}
	}

	async fn submit_transaction(&self, chain_id: ChainId, tx: Transaction) -> Result<TxHash> {
		match self {
			DeliveryServiceImpl::Rpc(rpc) => rpc.submit_transaction(chain_id, tx).await,
		}
	}

	async fn wait_for_confirmation(
		&self,
		chain_id: ChainId,
		tx_hash: TxHash,
		confirmations: u64,
	) -> Result<TransactionReceipt> {
		match self {
			DeliveryServiceImpl::Rpc(rpc) => {
				rpc.wait_for_confirmation(chain_id, tx_hash, confirmations)
					.await
			}
		}
	}

	async fn estimate_gas(&self, chain_id: ChainId, tx: &Transaction) -> Result<U256> {
		match self {
			DeliveryServiceImpl::Rpc(rpc) => rpc.estimate_gas(chain_id, tx).await,
		}
	}

	async fn get_gas_price(&self, chain_id: ChainId) -> Result<U256> {
		match self {
			DeliveryServiceImpl::Rpc(rpc) => rpc.get_gas_price(chain_id).await,
		}
	}

	async fn get_nonce(&self, chain_id: ChainId, address: Address) -> Result<U256> {
		match self {
			DeliveryServiceImpl::Rpc(rpc) => rpc.get_nonce(chain_id, address).await,
		}
	}
}
