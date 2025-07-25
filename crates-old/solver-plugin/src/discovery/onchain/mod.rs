//! # On-chain Discovery Implementations
//!
//! Provides discovery plugins for monitoring blockchain events.
//!
//! This module contains implementations for discovering orders and events
//! directly from blockchain networks through RPC interfaces, event logs,
//! and smart contract interactions.

pub mod eip7683;

pub use eip7683::{Eip7683OnchainConfig, Eip7683OnchainDiscoveryPlugin};
