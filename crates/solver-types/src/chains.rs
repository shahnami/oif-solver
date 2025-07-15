//! Chain-related types and traits.

use crate::{common::*, errors::Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Chain identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChainId(pub u64);

impl ChainId {
	pub const ETHEREUM: Self = Self(1);
	pub const ARBITRUM: Self = Self(42161);
	pub const OPTIMISM: Self = Self(10);
	pub const POLYGON: Self = Self(137);
	pub const BASE: Self = Self(8453);
}

impl fmt::Display for ChainId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.0)
	}
}

impl FromStr for ChainId {
	type Err = std::num::ParseIntError;

	fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
		Ok(ChainId(s.parse()?))
	}
}

/// Transaction request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
	pub to: Address,
	pub value: U256,
	pub data: Vec<u8>,
	pub gas_limit: Option<U256>,
	pub gas_price: Option<U256>,
	pub nonce: Option<U256>,
}

/// Transaction receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionReceipt {
	pub transaction_hash: TxHash,
	pub block_number: BlockNumber,
	pub gas_used: U256,
	pub status: bool,
	pub timestamp: Option<u64>,
}

/// Basic log structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
	pub address: Address,
	pub topics: Vec<Bytes32>,
	pub data: Vec<u8>,
	pub block_number: BlockNumber,
	pub transaction_hash: TxHash,
	pub log_index: u64,
}

/// Chain adapter trait for interacting with different blockchains
#[async_trait]
pub trait ChainAdapter: Send + Sync {
	/// Get the chain ID
	fn chain_id(&self) -> ChainId;

	/// Get required confirmations for this chain
	fn confirmations(&self) -> u64;

	/// Get current block number
	async fn get_block_number(&self) -> Result<BlockNumber>;

	/// Get block timestamp
	async fn get_block_timestamp(&self, block_number: BlockNumber) -> Result<u64>;

	/// Get account balance
	async fn get_balance(&self, address: Address) -> Result<U256>;

	/// Submit a transaction
	async fn submit_transaction(&self, tx: Transaction) -> Result<TxHash>;

	/// Get transaction receipt
	async fn get_transaction_receipt(&self, tx_hash: TxHash) -> Result<Option<TransactionReceipt>>;

	/// Call a contract function (read-only)
	async fn call(&self, tx: Transaction, block: Option<BlockNumber>) -> Result<Vec<u8>>;

	/// Get logs for a specific block range
	async fn get_logs(
		&self,
		address: Option<Address>,
		topics: Vec<Option<Bytes32>>,
		from_block: BlockNumber,
		to_block: BlockNumber,
	) -> Result<Vec<Log>>;

	/// Estimate gas for a transaction
	async fn estimate_gas(&self, tx: &Transaction) -> Result<U256>;

	/// Get current gas price
	async fn get_gas_price(&self) -> Result<U256>;
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_chain_id_constants() {
		assert_eq!(ChainId::ETHEREUM.0, 1);
		assert_eq!(ChainId::ARBITRUM.0, 42161);
		assert_eq!(ChainId::OPTIMISM.0, 10);
		assert_eq!(ChainId::POLYGON.0, 137);
		assert_eq!(ChainId::BASE.0, 8453);
	}

	#[test]
	fn test_chain_id_display() {
		assert_eq!(ChainId(1).to_string(), "1");
		assert_eq!(ChainId(42161).to_string(), "42161");
	}

	#[test]
	fn test_transaction_construction() {
		let tx = Transaction {
			to: Address::zero(),
			value: U256::from(1000),
			data: vec![1, 2, 3],
			gas_limit: Some(U256::from(21000)),
			gas_price: None,
			nonce: None,
		};

		assert_eq!(tx.to, Address::zero());
		assert_eq!(tx.value, U256::from(1000));
		assert_eq!(tx.data, vec![1, 2, 3]);
		assert_eq!(tx.gas_limit, Some(U256::from(21000)));
		assert!(tx.gas_price.is_none());
		assert!(tx.nonce.is_none());
	}
}
