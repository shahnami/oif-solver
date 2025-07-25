//! # Settlement Plugin Implementations
//!
//! Provides concrete implementations of settlement strategy plugins.
//!
//! This module contains implementations for various settlement strategies
//! including direct settlement, oracle-based settlement, and dispute resolution
//! mechanisms for cross-chain order settlement.

pub mod direct;

pub use direct::*;
