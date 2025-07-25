use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
	#[error("IO error: {0}")]
	Io(#[from] std::io::Error),
	#[error("Parse error: {0}")]
	Parse(#[from] toml::de::Error),
	#[error("Validation error: {0}")]
	Validation(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
	pub solver: SolverConfig,
	pub storage: StorageConfig,
	pub delivery: DeliveryConfig,
	pub account: AccountConfig,
	pub discovery: DiscoveryConfig,
	pub order: OrderConfig,
	pub settlement: SettlementConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolverConfig {
	pub id: String,
	#[serde(default = "default_monitoring_timeout_minutes")]
	pub monitoring_timeout_minutes: u64,
}

fn default_monitoring_timeout_minutes() -> u64 {
	480 // Default to 8 hours
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
	pub backend: String,
	pub config: toml::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeliveryConfig {
	pub providers: HashMap<String, toml::Value>,
	#[serde(default = "default_confirmations")]
	pub confirmations: u64,
}

fn default_confirmations() -> u64 {
	12 // Default to 12 confirmations
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountConfig {
	pub provider: String,
	pub config: toml::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoveryConfig {
	pub sources: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderConfig {
	pub implementations: HashMap<String, toml::Value>,
	pub execution_strategy: StrategyConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StrategyConfig {
	pub strategy_type: String,
	pub config: toml::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SettlementConfig {
	pub implementations: HashMap<String, toml::Value>,
}

impl Config {
	pub fn from_file(path: &str) -> Result<Self, ConfigError> {
		let content = std::fs::read_to_string(path)?;
		content.parse()
	}

	fn validate(&self) -> Result<(), ConfigError> {
		// Validate solver config
		if self.solver.id.is_empty() {
			return Err(ConfigError::Validation("Solver ID cannot be empty".into()));
		}

		// Validate storage config
		if self.storage.backend.is_empty() {
			return Err(ConfigError::Validation(
				"Storage backend cannot be empty".into(),
			));
		}

		// Validate delivery config
		if self.delivery.providers.is_empty() {
			return Err(ConfigError::Validation(
				"At least one delivery provider required".into(),
			));
		}

		// Validate account config
		if self.account.provider.is_empty() {
			return Err(ConfigError::Validation(
				"Account provider cannot be empty".into(),
			));
		}

		// Validate discovery config
		if self.discovery.sources.is_empty() {
			return Err(ConfigError::Validation(
				"At least one discovery source required".into(),
			));
		}

		// Validate order config
		if self.order.implementations.is_empty() {
			return Err(ConfigError::Validation(
				"At least one order implementation required".into(),
			));
		}
		if self.order.execution_strategy.strategy_type.is_empty() {
			return Err(ConfigError::Validation(
				"Execution strategy type cannot be empty".into(),
			));
		}

		// Validate settlement config
		if self.settlement.implementations.is_empty() {
			return Err(ConfigError::Validation(
				"At least one settlement implementation required".into(),
			));
		}

		Ok(())
	}
}

impl FromStr for Config {
	type Err = ConfigError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let config: Config = toml::from_str(s)?;
		config.validate()?;
		Ok(config)
	}
}
