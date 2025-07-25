//! # Discovery Plugin Implementations
//!
//! Provides concrete implementations of order discovery plugins.
//!
//! This module contains implementations of the discovery plugin trait for various
//! sources including on-chain monitoring, off-chain APIs, and protocol-specific
//! discovery mechanisms for finding and tracking order events.

pub mod offchain;
pub mod onchain;

pub use onchain::eip7683::{Eip7683OnchainConfig, Eip7683OnchainDiscoveryPlugin};
