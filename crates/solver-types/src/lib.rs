//! Core types and traits for the OIF solver system.
//!
//! This crate defines the fundamental abstractions that all solver components
//! implement and interact with.

pub mod chains;
pub mod common;
pub mod errors;
pub mod events;
pub mod oracles;
pub mod orders;
pub mod settlement;
pub mod standards;

// Re-export commonly used types
pub use chains::{ChainAdapter, ChainId};
pub use common::{Address, Bytes32, TxHash, U256};
pub use errors::{Result, SolverError};
pub use oracles::{Attestation, Oracle};
pub use orders::{Input, Order, OrderId, OrderSemantics, OrderStandard, Output};
pub use settlement::SettlementStrategy;
