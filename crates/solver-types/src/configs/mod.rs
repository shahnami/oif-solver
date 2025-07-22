//! # Configuration Types
//!
//! Configuration structures for all solver components and plugins.
//!
//! This module defines the configuration schema for the entire solver system,
//! including settings for individual services, plugin configurations, and
//! operational parameters that control system behavior.

use crate::{plugins::PluginConfig, DeliveryStrategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main solver configuration structure.
///
/// This is the root configuration object that contains settings for all
/// solver components, including core solver settings, plugin configurations,
/// and service-specific parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverConfig {
	/// Core solver settings like name, ports, and logging
	pub solver: SolverSettings,
	/// Plugin configurations for all plugin types
	pub plugins: PluginsConfig,
	/// Delivery service configuration
	pub delivery: DeliveryConfig,
	/// Settlement service configuration
	pub settlement: SettlementConfig,
	/// Discovery service configuration
	pub discovery: DiscoveryConfig,
	/// State service configuration
	pub state: StateConfig,
}

/// Core solver service settings.
///
/// Contains basic operational parameters for the solver service including
/// identification, logging configuration, and network ports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverSettings {
	/// Unique name for this solver instance
	pub name: String,
	/// Logging level for the service
	pub log_level: String,
	/// HTTP API server port
	pub http_port: u16,
	/// Metrics server port
	pub metrics_port: u16,
}

/// Plugin configurations for all plugin types.
///
/// Contains named configurations for each type of plugin supported by
/// the solver system. Each plugin type can have multiple named instances
/// with different configurations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
	/// Order processor plugin configurations
	pub order: HashMap<String, PluginConfig>,
	/// Delivery plugin configurations
	pub delivery: HashMap<String, PluginConfig>,
	/// Settlement plugin configurations
	pub settlement: HashMap<String, PluginConfig>,
	/// State storage plugin configurations
	pub state: HashMap<String, PluginConfig>,
	/// Discovery plugin configurations
	pub discovery: HashMap<String, PluginConfig>,
}

/// Configuration for the delivery service.
///
/// Controls how orders are processed and delivered, including strategy
/// selection, fallback behavior, and concurrency limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryConfig {
	/// Primary delivery strategy to use
	pub strategy: DeliveryStrategy,
	/// Whether to enable fallback delivery mechanisms
	pub fallback_enabled: bool,
	/// Maximum number of parallel delivery attempts
	pub max_parallel_attempts: usize,
}

/// Configuration for the settlement service.
///
/// Defines settlement behavior including strategies, profit thresholds,
/// and monitoring intervals for cross-chain settlement operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementConfig {
	/// Default settlement strategy to use
	pub default_strategy: String,
	/// Alternative strategies to try if default fails
	pub fallback_strategies: Vec<String>,
	/// Minimum profit threshold for settlement execution
	pub profit_threshold_wei: String,
	/// Interval for monitoring settlement transactions
	#[serde(default = "default_monitor_interval_seconds")]
	pub monitor_interval_seconds: u64,
}

/// Configuration for the discovery service.
///
/// Controls order discovery behavior including monitoring settings
/// and rate limiting for discovery plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
	/// Whether to enable real-time monitoring of order sources
	pub realtime_monitoring: bool,
	/// Maximum events to process per second
	#[serde(default = "default_max_events_per_second")]
	pub max_events_per_second: u64,
	/// Maximum concurrent discovery sources
	#[serde(default = "default_max_concurrent_sources")]
	pub max_concurrent_sources: usize,
}

/// Configuration for the state service.
///
/// Defines state storage backend settings, cleanup policies,
/// and performance parameters for state management.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateConfig {
	/// Default state storage backend to use
	pub default_backend: String,
	/// Whether to enable metrics collection for state operations
	pub enable_metrics: bool,
	/// Interval between state cleanup operations
	pub cleanup_interval_seconds: u64,
	/// Maximum concurrent state operations
	pub max_concurrent_operations: usize,
}

/// Default maximum events per second for discovery service.
fn default_max_events_per_second() -> u64 {
	1000
}

/// Default maximum concurrent discovery sources.
fn default_max_concurrent_sources() -> usize {
	10
}

/// Default settlement monitor interval in seconds.
fn default_monitor_interval_seconds() -> u64 {
	10
}
