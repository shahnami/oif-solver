// solver-config/src/lib.rs

use std::env;
use std::path::Path;
use thiserror::Error;

// Import config types from solver-types
use solver_types::SolverConfig;

#[derive(Error, Debug)]
pub enum ConfigError {
	#[error("File not found: {0}")]
	FileNotFound(String),

	#[error("Parse error: {0}")]
	ParseError(String),

	#[error("Validation error: {0}")]
	ValidationError(String),

	#[error("Environment variable not found: {0}")]
	EnvVarNotFound(String),

	#[error("IO error: {0}")]
	IoError(#[from] std::io::Error),
}

/// Configuration loader with environment variable substitution
#[derive(Default)]
pub struct ConfigLoader {
	file_path: Option<String>,
	env_prefix: String,
}

impl ConfigLoader {
	pub fn new() -> Self {
		Self {
			file_path: None,
			env_prefix: "SOLVER_".to_string(),
		}
	}

	pub fn with_file<P: AsRef<Path>>(mut self, path: P) -> Self {
		self.file_path = Some(path.as_ref().to_string_lossy().to_string());
		self
	}

	pub fn with_env_prefix(mut self, prefix: impl Into<String>) -> Self {
		self.env_prefix = prefix.into();
		self
	}

	pub async fn load(&self) -> Result<SolverConfig, ConfigError> {
		// Load base configuration from file
		let mut config = if let Some(file_path) = &self.file_path {
			self.load_from_file(file_path).await?
		} else {
			return Err(ConfigError::FileNotFound(
				"No configuration file specified".to_string(),
			));
		};

		// Apply environment variable overrides
		self.apply_env_overrides(&mut config)?;

		// Validate configuration
		self.validate_config(&config)?;

		Ok(config)
	}

	async fn load_from_file(&self, file_path: &str) -> Result<SolverConfig, ConfigError> {
		let content = tokio::fs::read_to_string(file_path).await?;

		// Substitute environment variables
		let substituted_content = self.substitute_env_vars(&content)?;

		// Parse TOML
		let config: SolverConfig = toml::from_str(&substituted_content)
			.map_err(|e| ConfigError::ParseError(e.to_string()))?;

		Ok(config)
	}

	fn substitute_env_vars(&self, content: &str) -> Result<String, ConfigError> {
		let mut result = content.to_string();

		// Find and replace ${VAR_NAME} patterns
		let re = regex::Regex::new(r"\$\{([^}]+)\}").unwrap();

		for cap in re.captures_iter(content) {
			let full_match = &cap[0];
			let var_name = &cap[1];

			let env_value = env::var(var_name)
				.map_err(|_| ConfigError::EnvVarNotFound(var_name.to_string()))?;

			result = result.replace(full_match, &env_value);
		}

		Ok(result)
	}

	fn apply_env_overrides(&self, config: &mut SolverConfig) -> Result<(), ConfigError> {
		// Apply environment variable overrides for common settings
		if let Ok(log_level) = env::var(format!("{}LOG_LEVEL", self.env_prefix)) {
			config.solver.log_level = log_level;
		}

		if let Ok(http_port) = env::var(format!("{}HTTP_PORT", self.env_prefix)) {
			config.solver.http_port = http_port
				.parse()
				.map_err(|e| ConfigError::ValidationError(format!("Invalid HTTP port: {}", e)))?;
		}

		if let Ok(metrics_port) = env::var(format!("{}METRICS_PORT", self.env_prefix)) {
			config.solver.metrics_port = metrics_port.parse().map_err(|e| {
				ConfigError::ValidationError(format!("Invalid metrics port: {}", e))
			})?;
		}

		Ok(())
	}

	fn validate_config(&self, config: &SolverConfig) -> Result<(), ConfigError> {
		let has_enabled_delivery = config.plugins.delivery.values().any(|p| p.enabled);

		let has_enabled_state = config.plugins.state.values().any(|p| p.enabled);

		if !has_enabled_delivery {
			return Err(ConfigError::ValidationError(
				"At least one delivery plugin must be enabled".to_string(),
			));
		}

		if !has_enabled_state {
			return Err(ConfigError::ValidationError(
				"At least one state plugin must be enabled".to_string(),
			));
		}

		Ok(())
	}
}
