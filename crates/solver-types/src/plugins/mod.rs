// solver-types/src/plugins/mod.rs

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

// Common types used across all plugins
pub type PluginId = String;
pub type ChainId = u64;
pub type Address = String; // Simplified for now
pub type TxHash = String;
pub type Timestamp = u64;

#[derive(Error, Debug)]
pub enum PluginError {
	#[error("Plugin not found: {0}")]
	NotFound(String),

	#[error("Plugin initialization failed: {0}")]
	InitializationFailed(String),

	#[error("Plugin execution failed: {0}")]
	ExecutionFailed(String),

	#[error("Invalid configuration: {0}")]
	InvalidConfiguration(String),

	#[error("Plugin not supported: {0}")]
	NotSupported(String),

	#[error("State error: {0}")]
	StateError(String),

	#[error("Network error: {0}")]
	NetworkError(String),
}

pub type PluginResult<T> = Result<T, PluginError>;

// Plugin configuration value type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ConfigValue {
	String(String),
	Number(i64),
	Float(f64),
	Boolean(bool),
	Array(Vec<ConfigValue>),
	Object(HashMap<String, ConfigValue>),
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

// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginConfig {
	pub plugin_type: String,
	pub enabled: bool,
	pub config: HashMap<String, ConfigValue>,
}

impl PluginConfig {
	pub fn new(plugin_type: impl Into<String>) -> Self {
		Self {
			plugin_type: plugin_type.into(),
			enabled: true,
			config: HashMap::new(),
		}
	}

	pub fn with_config(mut self, key: impl Into<String>, value: impl Into<ConfigValue>) -> Self {
		self.config.insert(key.into(), value.into());
		self
	}

	pub fn get_string(&self, key: &str) -> Option<String> {
		match self.config.get(key)? {
			ConfigValue::String(s) => Some(s.clone()),
			_ => None,
		}
	}

	pub fn get_number(&self, key: &str) -> Option<i64> {
		match self.config.get(key)? {
			ConfigValue::Number(n) => Some(*n),
			_ => None,
		}
	}

	pub fn get_bool(&self, key: &str) -> Option<bool> {
		match self.config.get(key)? {
			ConfigValue::Boolean(b) => Some(*b),
			_ => None,
		}
	}

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
}
