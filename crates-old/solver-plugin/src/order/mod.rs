//! # Order Plugin Implementations
//!
//! Provides concrete implementations of order processing plugins.
//!
//! This module contains implementations for processing different order formats
//! and protocols, including validation, parsing, and conversion to executable
//! transactions for various cross-chain order types.

pub mod eip7683;
pub mod processor;

pub use eip7683::{create_eip7683_processor, Eip7683Config, Eip7683OrderPlugin};
