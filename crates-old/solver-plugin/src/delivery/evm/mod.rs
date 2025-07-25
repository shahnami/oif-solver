//! # EVM-Compatible Delivery Implementations
//!
//! Provides delivery plugins for Ethereum Virtual Machine compatible blockchains.
//!
//! This module contains delivery implementations for EVM-based networks including
//! Ethereum, Polygon, BSC, and other compatible chains using various providers
//! like Alloy for transaction management.

pub mod alloy;

pub use alloy::{EvmAlloyConfig, EvmAlloyDeliveryPlugin};
