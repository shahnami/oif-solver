//! # Plugin System
//!
//! Defines the plugin architecture and common types used throughout the solver system.
//!
//! This module provides the foundation for the extensible plugin system that allows
//! the solver to support multiple blockchain protocols, order formats, delivery
//! mechanisms, and settlement strategies through a unified interface.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use thiserror::Error;

pub mod base;
pub mod delivery;
pub mod discovery;
pub mod order;
pub mod settlement;
pub mod state;

pub use base::*;
pub use delivery::*;
pub use discovery::*;
pub use order::*;
pub use settlement::*;
pub use state::*;

/// Unique identifier for a plugin instance.
pub type PluginId = String;

/// Blockchain network identifier.
pub type ChainId = u64;

/// Blockchain address representation.
pub type Address = String;

/// Transaction hash identifier.
pub type TxHash = String;

/// Unix timestamp in seconds.
pub type Timestamp = u64;

/// Error types for plugin operations.
///
/// Encompasses all possible error conditions that can occur during plugin
/// operations, from initialization through execution and configuration.
#[derive(Error, Debug)]
pub enum PluginError {
	/// Requested plugin was not found or registered
	#[error("Plugin not found: {0}")]
	NotFound(String),

	/// Plugin failed to initialize properly
	#[error("Plugin initialization failed: {0}")]
	InitializationFailed(String),

	/// Plugin execution encountered an error
	#[error("Plugin execution failed: {0}")]
	ExecutionFailed(String),

	/// Plugin configuration is invalid or missing required fields
	#[error("Invalid configuration: {0}")]
	InvalidConfiguration(String),

	/// Plugin type is not supported by the current system
	#[error("Plugin not supported: {0}")]
	NotSupported(String),

	/// Error in plugin state management operations
	#[error("State error: {0}")]
	StateError(String),

	/// Data serialization or deserialization error
	#[error("Serialization error: {0}")]
	SerializationError(String),

	/// Network communication error
	#[error("Network error: {0}")]
	NetworkError(String),
}

/// Result type for plugin operations.
pub type PluginResult<T> = Result<T, PluginError>;

/// Configuration value type for plugin settings.
///
/// Supports various data types commonly used in configuration files,
/// allowing plugins to receive strongly-typed configuration parameters
/// while maintaining flexibility for different plugin requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
	/// String configuration value
	String(String),
	/// Integer configuration value
	Number(i64),
	/// Floating-point configuration value
	Float(f64),
	/// Boolean configuration value
	Boolean(bool),
	/// Array of configuration values
	Array(Vec<ConfigValue>),
	/// Nested object configuration
	Object(HashMap<String, ConfigValue>),
	/// Null or unset value
	Null,
}

impl From<String> for ConfigValue {
	fn from(s: String) -> Self {
		ConfigValue::String(s)
	}
}

impl From<&str> for ConfigValue {
	fn from(s: &str) -> Self {
		ConfigValue::String(s.to_string())
	}
}

impl From<i64> for ConfigValue {
	fn from(n: i64) -> Self {
		ConfigValue::Number(n)
	}
}

impl From<bool> for ConfigValue {
	fn from(b: bool) -> Self {
		ConfigValue::Boolean(b)
	}
}

/// Configuration structure for plugin instances.
///
/// Contains the plugin type identifier, enable/disable flag, and
/// configuration parameters specific to the plugin implementation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginConfig {
	/// Type identifier for the plugin (e.g., "eip7683_onchain", "alloy_delivery")
	pub plugin_type: String,
	/// Whether this plugin instance is enabled
	pub enabled: bool,
	/// Plugin-specific configuration parameters
	pub config: HashMap<String, ConfigValue>,
}

impl PluginConfig {
	/// Create a new plugin configuration with the specified type.
	///
	/// Creates an enabled plugin configuration with the given type identifier
	/// and an empty configuration map.
	pub fn new(plugin_type: impl Into<String>) -> Self {
		Self {
			plugin_type: plugin_type.into(),
			enabled: true,
			config: HashMap::new(),
		}
	}

	/// Add a configuration parameter to this plugin config.
	///
	/// Provides a builder-pattern method for adding configuration values.
	pub fn with_config(mut self, key: impl Into<String>, value: impl Into<ConfigValue>) -> Self {
		self.config.insert(key.into(), value.into());
		self
	}

	/// Get a string configuration value.
	pub fn get_string(&self, key: &str) -> Option<String> {
		match self.config.get(key)? {
			ConfigValue::String(s) => Some(s.clone()),
			_ => None,
		}
	}

	/// Get an integer configuration value.
	pub fn get_number(&self, key: &str) -> Option<i64> {
		match self.config.get(key)? {
			ConfigValue::Number(n) => Some(*n),
			_ => None,
		}
	}

	/// Get a boolean configuration value.
	pub fn get_bool(&self, key: &str) -> Option<bool> {
		match self.config.get(key)? {
			ConfigValue::Boolean(b) => Some(*b),
			_ => None,
		}
	}

	/// Get an array of strings from configuration.
	///
	/// Extracts string values from a configuration array, filtering out
	/// non-string elements.
	pub fn get_array(&self, key: &str) -> Option<Vec<String>> {
		match self.config.get(key)? {
			ConfigValue::Array(arr) => Some(
				arr.iter()
					.filter_map(|v| {
						if let ConfigValue::String(s) = v {
							Some(s.clone())
						} else {
							None
						}
					})
					.collect(),
			),
			_ => None,
		}
	}

	/// Get an array of numbers from configuration.
	///
	/// Extracts numeric values from a configuration array, filtering out
	/// non-numeric elements.
	pub fn get_number_array(&self, key: &str) -> Option<Vec<i64>> {
		match self.config.get(key)? {
			ConfigValue::Array(arr) => Some(
				arr.iter()
					.filter_map(|v| {
						if let ConfigValue::Number(n) = v {
							Some(*n)
						} else {
							None
						}
					})
					.collect(),
			),
			_ => None,
		}
	}

	/// Get an object value from configuration.
	pub fn get_object(&self, key: &str) -> Option<HashMap<String, ConfigValue>> {
		self.config.get(key).and_then(|v| match v {
			ConfigValue::Object(obj) => Some(obj.clone()),
			_ => None,
		})
	}
}
