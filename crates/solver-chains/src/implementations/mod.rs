//! Chain adapter implementations for various blockchain networks.
//!
//! This module contains concrete implementations of the `ChainAdapter` trait
//! for different blockchain types. Currently supports:
//!
//! - **EVM chains**: Ethereum and EVM-compatible blockchains via the `evm` module
//!
//! Each implementation is gated behind a feature flag to allow selective compilation
//! based on the blockchain networks your solver needs to support.

pub mod evm;
