// solver-types/src/plugins/base.rs

use super::{ConfigValue, PluginConfig, PluginResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

/// Base trait that all plugins must implement
/// Provides common functionality for plugin management, configuration, and lifecycle
#[async_trait]
pub trait BasePlugin: Send + Sync + Debug {
	/// Unique identifier for this plugin type
	fn plugin_type(&self) -> &'static str;

	/// Human-readable name for this plugin
	fn name(&self) -> String;

	/// Plugin version
	fn version(&self) -> &'static str;

	/// Plugin description
	fn description(&self) -> &'static str;

	/// Initialize the plugin with the given configuration
	async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()>;

	/// Validate the plugin configuration
	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()>;

	/// Check if the plugin is healthy and ready to use
	async fn health_check(&self) -> PluginResult<PluginHealth>;

	/// Get plugin metrics and statistics
	async fn get_metrics(&self) -> PluginResult<PluginMetrics>;

	/// Shutdown the plugin gracefully
	async fn shutdown(&mut self) -> PluginResult<()>;

	/// Get plugin configuration schema for validation
	fn config_schema(&self) -> PluginConfigSchema;

	/// Cast to Any for downcasting to concrete types
	fn as_any(&self) -> &dyn Any;

	/// Cast to mutable Any for downcasting to concrete types
	fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Plugin health status
#[derive(Debug, Clone)]
pub struct PluginHealth {
	pub status: HealthStatus,
	pub message: String,
	pub last_check: u64, // Unix timestamp
	pub details: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
	Healthy,
	Degraded,
	Unhealthy,
	Unknown,
}

impl PluginHealth {
	pub fn healthy(message: impl Into<String>) -> Self {
		Self {
			status: HealthStatus::Healthy,
			message: message.into(),
			last_check: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			details: HashMap::new(),
		}
	}

	pub fn unhealthy(message: impl Into<String>) -> Self {
		Self {
			status: HealthStatus::Unhealthy,
			message: message.into(),
			last_check: std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			details: HashMap::new(),
		}
	}

	pub fn is_healthy(&self) -> bool {
		self.status == HealthStatus::Healthy
	}

	pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
		self.details.insert(key.into(), value.into());
		self
	}
}

/// Plugin metrics and statistics
#[derive(Debug, Clone, Default)]
pub struct PluginMetrics {
	pub counters: HashMap<String, u64>,
	pub gauges: HashMap<String, f64>,
	pub histograms: HashMap<String, Vec<f64>>,
	pub metadata: HashMap<String, String>,
}

impl PluginMetrics {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn counter(mut self, name: impl Into<String>, value: u64) -> Self {
		self.counters.insert(name.into(), value);
		self
	}

	pub fn gauge(mut self, name: impl Into<String>, value: f64) -> Self {
		self.gauges.insert(name.into(), value);
		self
	}

	pub fn increment_counter(&mut self, name: &str) {
		*self.counters.entry(name.to_string()).or_insert(0) += 1;
	}

	pub fn set_gauge(&mut self, name: impl Into<String>, value: f64) {
		self.gauges.insert(name.into(), value);
	}
}

/// Plugin configuration schema for validation
#[derive(Debug, Clone)]
pub struct PluginConfigSchema {
	pub required_fields: Vec<ConfigField>,
	pub optional_fields: Vec<ConfigField>,
}

#[derive(Debug, Clone)]
pub struct ConfigField {
	pub name: String,
	pub field_type: ConfigFieldType,
	pub description: String,
	pub default_value: Option<ConfigValue>,
}

#[derive(Debug, Clone)]
pub enum ConfigFieldType {
	String,
	Number,
	Boolean,
	Array(Box<ConfigFieldType>),
	Object,
}

impl Default for PluginConfigSchema {
	fn default() -> Self {
		Self::new()
	}
}

impl PluginConfigSchema {
	pub fn new() -> Self {
		Self {
			required_fields: Vec::new(),
			optional_fields: Vec::new(),
		}
	}

	pub fn required(
		mut self,
		name: impl Into<String>,
		field_type: ConfigFieldType,
		description: impl Into<String>,
	) -> Self {
		self.required_fields.push(ConfigField {
			name: name.into(),
			field_type,
			description: description.into(),
			default_value: None,
		});
		self
	}

	pub fn optional(
		mut self,
		name: impl Into<String>,
		field_type: ConfigFieldType,
		description: impl Into<String>,
		default: Option<ConfigValue>,
	) -> Self {
		self.optional_fields.push(ConfigField {
			name: name.into(),
			field_type,
			description: description.into(),
			default_value: default,
		});
		self
	}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
	pub max_attempts: u32,
	pub initial_delay_ms: u64,
	pub max_delay_ms: u64,
	pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
	fn default() -> Self {
		Self {
			max_attempts: 3,
			initial_delay_ms: 1000,
			max_delay_ms: 30000,
			backoff_multiplier: 2.0,
		}
	}
}
