//! Ethereum Virtual Machine (EVM) compatible chain adapters.
//!
//! This module provides adapters for interacting with Ethereum and other EVM-compatible
//! blockchains (such as Polygon, Arbitrum, Optimism, BSC, etc.).
//!
//! The adapters handle:
//! - JSON-RPC communication with EVM nodes
//! - Transaction submission and monitoring
//! - Event log queries and filtering
//! - Block information retrieval
//! - Balance and contract state queries
//!
//! Available implementations:
//! - `EthersAdapter`: Uses the ethers-rs library

mod ethers_adapter;

pub use ethers_adapter::{EthersAdapter, GasStrategy};
