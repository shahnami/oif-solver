use crate::{plugins::PluginConfig, DeliveryStrategy};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Main solver configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverConfig {
	pub solver: SolverSettings,
	pub plugins: PluginsConfig,
	pub delivery: DeliveryConfig,
	pub settlement: SettlementConfig,
	pub discovery: DiscoveryConfig,
	pub state: StateConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverSettings {
	pub name: String,
	pub log_level: String,
	pub http_port: u16,
	pub metrics_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginsConfig {
	pub order: HashMap<String, PluginConfig>,
	pub delivery: HashMap<String, PluginConfig>,
	pub settlement: HashMap<String, PluginConfig>,
	pub state: HashMap<String, PluginConfig>,
	pub discovery: HashMap<String, PluginConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryConfig {
	pub strategy: DeliveryStrategy,
	pub fallback_enabled: bool,
	pub max_parallel_attempts: usize,
}

/// Unified settlement configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementConfig {
	pub default_strategy: String,
	pub fallback_strategies: Vec<String>,
	pub profit_threshold_wei: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
	// From strategy config
	pub historical_sync: bool,
	pub realtime_monitoring: bool,
	pub dedupe_events: bool,
	pub max_event_age_seconds: u64,
	#[serde(default = "default_max_events_per_second")]
	pub max_events_per_second: u64,
	#[serde(default = "default_event_buffer_size")]
	pub event_buffer_size: usize,
	#[serde(default = "default_deduplication_window_seconds")]
	pub deduplication_window_seconds: u64,
	#[serde(default = "default_max_concurrent_sources")]
	pub max_concurrent_sources: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateConfig {
	pub default_backend: String,
	pub enable_metrics: bool,
	pub cleanup_interval_seconds: u64,
	pub max_concurrent_operations: usize,
}

// Default values for discovery config
fn default_max_events_per_second() -> u64 {
	1000
}
fn default_event_buffer_size() -> usize {
	10000
}
fn default_deduplication_window_seconds() -> u64 {
	300
}
fn default_max_concurrent_sources() -> usize {
	10
}
