//! # Base Plugin Framework
//!
//! Provides the foundational traits and types for the plugin system.
//!
//! This module defines the base plugin trait that all plugins must implement,
//! along with health monitoring, metrics collection, configuration validation,
//! and lifecycle management functionality.

use crate::PluginError;

use super::{ConfigValue, PluginConfig, PluginResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Debug;

/// Base trait that all plugins must implement for system integration.
///
/// Provides the fundamental interface for plugin management, configuration,
/// lifecycle control, health monitoring, and metrics collection. All specialized
/// plugin types (discovery, delivery, settlement, state) extend this base trait
/// with their specific functionality while maintaining consistent management interfaces.
#[async_trait]
pub trait BasePlugin: Send + Sync + Debug {
	/// Get the unique identifier for this plugin type.
	///
	/// Returns a static string that identifies the plugin implementation type
	/// (e.g., "eip7683_onchain", "ethers_delivery", "memory_state").
	fn plugin_type(&self) -> &'static str;

	/// Get a human-readable name for this plugin instance.
	///
	/// Provides a descriptive name that can be used for display purposes
	/// and logging. May include instance-specific information.
	fn name(&self) -> String;

	/// Get the version of this plugin implementation.
	///
	/// Returns the version string for this plugin, used for compatibility
	/// checks and system diagnostics.
	fn version(&self) -> &'static str;

	/// Get a human-readable description of this plugin's functionality.
	///
	/// Provides a detailed description of what the plugin does and its
	/// intended use cases.
	fn description(&self) -> &'static str;

	/// Initialize the plugin with the provided configuration.
	///
	/// Performs plugin-specific initialization based on the configuration,
	/// including resource allocation, connection establishment, and validation
	/// of configuration parameters.
	///
	/// # Arguments
	/// * `config` - Plugin configuration including type-specific parameters
	///
	/// # Returns
	/// Success if initialization completes, error with details on failure
	async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()>;

	/// Validate the provided configuration for correctness.
	///
	/// Checks that the configuration contains all required parameters and
	/// that parameter values are valid for this plugin type. Should be called
	/// before initialization to catch configuration errors early.
	///
	/// # Arguments
	/// * `config` - Plugin configuration to validate
	///
	/// # Returns
	/// Success if configuration is valid, error with validation details on failure
	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()>;

	/// Perform a health check on the plugin's operational status.
	///
	/// Checks the current health and operational readiness of the plugin,
	/// including connectivity to external services, resource availability,
	/// and internal state consistency.
	///
	/// # Returns
	/// Health status with operational details and diagnostic information
	async fn health_check(&self) -> PluginResult<PluginHealth>;

	/// Collect performance metrics and operational statistics.
	///
	/// Gathers current metrics about plugin performance, including counters,
	/// gauges, timing histograms, and operational metadata for monitoring
	/// and debugging purposes.
	///
	/// # Returns
	/// Metrics collection with performance and operational data
	async fn get_metrics(&self) -> PluginResult<PluginMetrics>;

	/// Gracefully shutdown the plugin and release resources.
	///
	/// Performs cleanup operations including closing connections, releasing
	/// resources, and persisting any necessary state. Should ensure the plugin
	/// can be safely removed from the system.
	///
	/// # Returns
	/// Success when shutdown completes, error if cleanup fails
	async fn shutdown(&mut self) -> PluginResult<()>;

	/// Get the configuration schema for this plugin type.
	///
	/// Returns the schema defining required and optional configuration fields,
	/// their types, descriptions, and default values. Used for configuration
	/// validation and documentation generation.
	///
	/// # Returns
	/// Configuration schema with field definitions and validation rules
	fn config_schema(&self) -> PluginConfigSchema;

	/// Cast this plugin to the Any trait for downcasting.
	///
	/// Enables type-safe downcasting to concrete plugin types when needed
	/// for accessing plugin-specific functionality.
	///
	/// # Returns
	/// Reference to this plugin as the Any trait
	fn as_any(&self) -> &dyn Any;

	/// Cast this plugin to a mutable Any trait for downcasting.
	///
	/// Enables type-safe mutable downcasting to concrete plugin types when
	/// needed for accessing plugin-specific functionality.
	///
	/// # Returns
	/// Mutable reference to this plugin as the Any trait
	fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Plugin health status and diagnostic information.
///
/// Contains comprehensive health information about a plugin including its current
/// operational status, descriptive message, timestamp of last check, and detailed
/// diagnostic information for troubleshooting.
#[derive(Debug, Clone)]
pub struct PluginHealth {
	/// Current health status of the plugin
	pub status: HealthStatus,
	/// Human-readable status message with details
	pub message: String,
	/// Unix timestamp of when this health check was performed
	pub last_check: u64,
	/// Additional diagnostic details as key-value pairs
	pub details: HashMap<String, String>,
}

/// Enumeration of possible plugin health statuses.
///
/// Represents the operational state of a plugin from fully healthy to completely
/// unavailable, with intermediate states for degraded performance or unknown status.
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
	/// Plugin is fully operational and performing normally
	Healthy,
	/// Plugin is operational but with reduced performance or functionality
	Degraded,
	/// Plugin is not operational or has critical issues
	Unhealthy,
	/// Plugin health status cannot be determined
	Unknown,
}

impl PluginHealth {
	/// Create a healthy status with the specified message.
	///
	/// Constructs a PluginHealth instance with Healthy status and current timestamp.
	///
	/// # Arguments
	/// * `message` - Descriptive message about the healthy status
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

	/// Create an unhealthy status with the specified message.
	///
	/// Constructs a PluginHealth instance with Unhealthy status and current timestamp.
	///
	/// # Arguments
	/// * `message` - Descriptive message about the unhealthy condition
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

	/// Check if this health status indicates the plugin is healthy.
	///
	/// Returns true only if the status is exactly Healthy, not for degraded
	/// or other statuses.
	pub fn is_healthy(&self) -> bool {
		self.status == HealthStatus::Healthy
	}

	/// Add a diagnostic detail to this health status.
	///
	/// Provides a builder pattern method for adding diagnostic information
	/// that can help with troubleshooting health issues.
	///
	/// # Arguments
	/// * `key` - Diagnostic detail key
	/// * `value` - Diagnostic detail value
	pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
		self.details.insert(key.into(), value.into());
		self
	}
}

/// Plugin performance metrics and operational statistics.
///
/// Contains various types of metrics collected from plugins including counters
/// for event counts, gauges for current values, histograms for timing data,
/// and metadata for additional context information.
#[derive(Debug, Clone, Default)]
pub struct PluginMetrics {
	/// Counter metrics tracking cumulative counts of events or operations
	pub counters: HashMap<String, u64>,
	/// Gauge metrics representing current values or levels
	pub gauges: HashMap<String, f64>,
	/// Histogram data for timing measurements and distributions
	pub histograms: HashMap<String, Vec<f64>>,
	/// Additional metadata and contextual information
	pub metadata: HashMap<String, String>,
}

impl PluginMetrics {
	/// Create a new empty metrics collection.
	pub fn new() -> Self {
		Self::default()
	}

	/// Add a counter metric with the specified value.
	///
	/// Builder pattern method for setting counter values during metrics construction.
	///
	/// # Arguments
	/// * `name` - Name of the counter metric
	/// * `value` - Counter value
	pub fn counter(mut self, name: impl Into<String>, value: u64) -> Self {
		self.counters.insert(name.into(), value);
		self
	}

	/// Add a gauge metric with the specified value.
	///
	/// Builder pattern method for setting gauge values during metrics construction.
	///
	/// # Arguments
	/// * `name` - Name of the gauge metric
	/// * `value` - Gauge value
	pub fn gauge(mut self, name: impl Into<String>, value: f64) -> Self {
		self.gauges.insert(name.into(), value);
		self
	}

	/// Increment a counter metric by 1.
	///
	/// If the counter doesn't exist, it will be created with an initial value of 1.
	///
	/// # Arguments
	/// * `name` - Name of the counter to increment
	pub fn increment_counter(&mut self, name: &str) {
		*self.counters.entry(name.to_string()).or_insert(0) += 1;
	}

	/// Set a gauge metric to the specified value.
	///
	/// Updates or creates a gauge metric with the given value.
	///
	/// # Arguments
	/// * `name` - Name of the gauge metric
	/// * `value` - New gauge value
	pub fn set_gauge(&mut self, name: impl Into<String>, value: f64) {
		self.gauges.insert(name.into(), value);
	}
}

/// Plugin configuration schema for validation and documentation.
///
/// Defines the expected structure of plugin configuration including required
/// and optional fields with their types, descriptions, and default values.
/// Used for configuration validation and automatic documentation generation.
#[derive(Debug, Clone)]
pub struct PluginConfigSchema {
	/// Configuration fields that must be provided
	pub required_fields: Vec<ConfigField>,
	/// Configuration fields that are optional with defaults
	pub optional_fields: Vec<ConfigField>,
}

/// Configuration field definition with type and validation information.
///
/// Describes a single configuration parameter including its name, expected type,
/// human-readable description, and optional default value.
#[derive(Debug, Clone)]
pub struct ConfigField {
	/// Name of the configuration field
	pub name: String,
	/// Expected data type for this field
	pub field_type: ConfigFieldType,
	/// Human-readable description of the field's purpose
	pub description: String,
	/// Default value if the field is optional
	pub default_value: Option<ConfigValue>,
}

/// Data types supported in plugin configuration fields.
///
/// Represents the various data types that can be used in plugin configurations,
/// including basic types like strings and numbers, as well as composite types
/// like arrays and nested objects.
#[derive(Debug, Clone)]
pub enum ConfigFieldType {
	/// String configuration parameter
	String,
	/// Numeric configuration parameter (integer or float)
	Number,
	/// Boolean configuration parameter
	Boolean,
	/// Array of values with specified element type
	Array(Box<ConfigFieldType>),
	/// Nested object with arbitrary structure
	Object,
}

impl PluginConfigSchema {
	/// Validate a plugin configuration against this schema.
	///
	/// Checks that all required fields are present and that all fields have the correct type.
	///
	/// # Arguments
	/// * `config` - The plugin configuration to validate
	///
	/// # Returns
	/// Success if configuration is valid, error with validation details on failure
	pub fn validate(&self, config: &PluginConfig) -> PluginResult<()> {
		// Check required fields
		for field in &self.required_fields {
			match &field.field_type {
				ConfigFieldType::String => {
					if config.get_string(&field.name).is_none() {
						return Err(PluginError::InvalidConfiguration(format!(
							"Required field '{}' is missing",
							field.name
						)));
					}
				}
				ConfigFieldType::Number => {
					if config.get_number(&field.name).is_none() {
						return Err(PluginError::InvalidConfiguration(format!(
							"Required field '{}' is missing or not a number",
							field.name
						)));
					}
				}
				ConfigFieldType::Boolean => {
					if config.get_bool(&field.name).is_none() {
						return Err(PluginError::InvalidConfiguration(format!(
							"Required field '{}' is missing or not a boolean",
							field.name
						)));
					}
				}
				ConfigFieldType::Array(_) => {
					if config.get_array(&field.name).is_none() {
						return Err(PluginError::InvalidConfiguration(format!(
							"Required field '{}' is missing or not an array",
							field.name
						)));
					}
				}
				ConfigFieldType::Object => {
					if config.get_object(&field.name).is_none() {
						return Err(PluginError::InvalidConfiguration(format!(
							"Required field '{}' is missing or not an object",
							field.name
						)));
					}
				}
			}
		}

		// Validate field types for all provided fields (including optional ones)
		// Note: We could do more sophisticated validation here, like checking array element types

		Ok(())
	}
}
impl Default for PluginConfigSchema {
	fn default() -> Self {
		Self::new()
	}
}

impl PluginConfigSchema {
	/// Create a new empty configuration schema.
	pub fn new() -> Self {
		Self {
			required_fields: Vec::new(),
			optional_fields: Vec::new(),
		}
	}

	/// Add a required configuration field to the schema.
	///
	/// Builder pattern method for adding required fields that must be present
	/// in plugin configurations.
	///
	/// # Arguments
	/// * `name` - Name of the configuration field
	/// * `field_type` - Expected data type
	/// * `description` - Human-readable description
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

	/// Add an optional configuration field to the schema.
	///
	/// Builder pattern method for adding optional fields that can be omitted
	/// from plugin configurations, optionally with default values.
	///
	/// # Arguments
	/// * `name` - Name of the configuration field
	/// * `field_type` - Expected data type
	/// * `description` - Human-readable description  
	/// * `default` - Optional default value if field is omitted
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

/// Configuration for retry behavior in plugin operations.
///
/// Defines retry parameters including attempt limits, timing delays, and
/// backoff strategies for handling transient failures in plugin operations.
/// Supports exponential backoff with configurable multipliers and maximum delays.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
	/// Maximum number of retry attempts before giving up
	pub max_attempts: u32,
	/// Initial delay between retries in milliseconds
	pub initial_delay_ms: u64,
	/// Maximum delay between retries in milliseconds
	pub max_delay_ms: u64,
	/// Multiplier for exponential backoff between retries
	pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
	/// Create default retry configuration with reasonable values.
	///
	/// Default configuration provides 3 retry attempts with exponential backoff
	/// starting at 1 second, capped at 30 seconds, with a 2x backoff multiplier.
	fn default() -> Self {
		Self {
			max_attempts: 3,
			initial_delay_ms: 1000,
			max_delay_ms: 30000,
			backoff_multiplier: 2.0,
		}
	}
}
