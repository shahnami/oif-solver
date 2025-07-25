//! Execution strategy implementations for the solver service.
//!
//! This module re-exports strategy implementations from the order module
//! to maintain a consistent module structure.

/// Re-export the strategy factory function from the order module.
pub use super::order::create_strategy;
