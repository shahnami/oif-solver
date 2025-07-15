//! Configuration management for the OIF solver.

pub mod loader;
pub mod serde_helpers;
pub mod types;

pub use loader::{load_config, ConfigLoader};
pub use types::*;

/// Re-export for convenience
pub use anyhow::Result;
