// solver-discovery/src/mod.rs

//! # Solver Discovery Module
//!
//! This module provides order discovery capabilities for the OIF Solver.
//! It orchestrates multiple discovery plugins to monitor various sources
//! for order events across different chains and protocols.
//!
//! ## Key Components
//!
//! - [`DiscoveryService`] - Low-level service that manages discovery plugins
//! - [`DiscoveryServiceBuilder`] - Builder for configuring discovery services

mod lib;

// Re-export main types and traits
pub use lib::{DiscoveryService, DiscoveryServiceBuilder, DiscoverySource, SourceStatus};

// Re-export commonly used types from solver-types
pub use solver_types::plugins::{
	ChainId, DiscoveryConfig, DiscoveryEvent, DiscoveryPlugin, DiscoveryStatus, EventFilter,
	EventSink, EventType, HistoricalDiscovery, HistoricalResult, PluginError, PluginResult,
	Timestamp,
};

// Common error types for external use
pub type Result<T> = std::result::Result<T, PluginError>;
