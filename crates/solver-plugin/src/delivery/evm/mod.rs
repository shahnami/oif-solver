//! # EVM-Compatible Delivery Implementations
//!
//! Provides delivery plugins for Ethereum Virtual Machine compatible blockchains.
//!
//! This module contains delivery implementations for EVM-based networks including
//! Ethereum, Polygon, BSC, and other compatible chains using various providers
//! like Ethers.rs for transaction management.

pub mod ethers;

pub use ethers::{EvmEthersConfig, EvmEthersDeliveryPlugin};
