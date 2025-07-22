//! # Configuration Management
//!
//! Provides configuration loading and validation for the OIF solver system.
//!
//! This crate handles loading solver configuration from TOML files with
//! environment variable substitution and runtime overrides. It supports
//! flexible configuration management including file-based configuration,
//! environment variable substitution, validation, and hierarchical overrides.
//!
//! ## Features
//!
//! - **File-based Configuration**: Load configuration from TOML files
//! - **Environment Variable Substitution**: Replace `${VAR}` patterns in config files
//! - **Runtime Overrides**: Apply environment variable overrides to specific settings
//! - **Configuration Validation**: Ensure required plugins and settings are present
//! - **Flexible Loading**: Support for different configuration sources and formats

use std::env;
use std::path::Path;
use thiserror::Error;

// Import config types from solver-types
use solver_types::SolverConfig;

/// Configuration loading and validation errors.
///
/// Represents the various error conditions that can occur during configuration
/// loading, parsing, validation, and environment variable processing.
#[derive(Error, Debug)]
pub enum ConfigError {
	/// Configuration file was not found at the specified path
	#[error("File not found: {0}")]
	FileNotFound(String),

	/// Configuration file could not be parsed as valid TOML
	#[error("Parse error: {0}")]
	ParseError(String),

	/// Configuration failed validation checks
	#[error("Validation error: {0}")]
	ValidationError(String),

	/// Required environment variable was not found during substitution
	#[error("Environment variable not found: {0}")]
	EnvVarNotFound(String),

	/// File system I/O error occurred during configuration loading
	#[error("IO error: {0}")]
	IoError(#[from] std::io::Error),
}

/// Configuration loader with environment variable substitution and validation.
///
/// The configuration loader handles loading solver configuration from TOML files,
/// performing environment variable substitution, applying runtime overrides, and
/// validating the resulting configuration. It supports flexible configuration
/// management with hierarchical precedence from files to environment variables.
#[derive(Default)]
pub struct ConfigLoader {
	/// Optional path to the configuration file to load
	file_path: Option<String>,
	/// Prefix for environment variable overrides (default: "SOLVER_")
	env_prefix: String,
}

impl ConfigLoader {
	/// Create a new configuration loader with default settings.
	///
	/// Initializes the loader with no file path and the default environment
	/// variable prefix "SOLVER_". Use the builder methods to configure the
	/// file path and environment prefix as needed.
	pub fn new() -> Self {
		Self {
			file_path: None,
			env_prefix: "SOLVER_".to_string(),
		}
	}

	/// Set the configuration file path to load from.
	///
	/// Specifies the TOML configuration file that should be loaded and parsed.
	/// The file will be processed for environment variable substitution before
	/// parsing. This is a builder method for fluent configuration.
	///
	/// # Arguments
	/// * `path` - Path to the configuration file to load
	pub fn with_file<P: AsRef<Path>>(mut self, path: P) -> Self {
		self.file_path = Some(path.as_ref().to_string_lossy().to_string());
		self
	}

	/// Set the prefix for environment variable overrides.
	///
	/// Configures the prefix used when looking for environment variables that
	/// should override configuration values. For example, with prefix "SOLVER_",
	/// the environment variable "SOLVER_LOG_LEVEL" will override the log_level setting.
	/// This is a builder method for fluent configuration.
	///
	/// # Arguments
	/// * `prefix` - Environment variable prefix (e.g., "SOLVER_", "APP_")
	pub fn with_env_prefix(mut self, prefix: impl Into<String>) -> Self {
		self.env_prefix = prefix.into();
		self
	}

	/// Load and process the complete solver configuration.
	///
	/// Performs the full configuration loading pipeline including file loading,
	/// environment variable substitution, runtime overrides, and validation.
	/// The process follows this sequence:
	/// 1. Load base configuration from the specified file
	/// 2. Apply environment variable substitution in the file content
	/// 3. Parse the TOML configuration
	/// 4. Apply environment variable overrides for specific settings
	/// 5. Validate the final configuration
	///
	/// # Returns
	/// The fully processed and validated solver configuration
	///
	/// # Errors
	/// Returns error if no file is specified, file loading fails, parsing fails,
	/// environment variables are missing, or validation fails
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

	/// Load configuration from a TOML file with environment variable substitution.
	///
	/// Reads the specified file, performs environment variable substitution
	/// on the content, and parses the result as TOML configuration.
	///
	/// # Arguments
	/// * `file_path` - Path to the TOML configuration file
	///
	/// # Returns
	/// Parsed solver configuration from the file
	///
	/// # Errors
	/// Returns error if file reading, environment substitution, or TOML parsing fails
	async fn load_from_file(&self, file_path: &str) -> Result<SolverConfig, ConfigError> {
		let content = tokio::fs::read_to_string(file_path).await?;

		// Substitute environment variables
		let substituted_content = self.substitute_env_vars(&content)?;

		// Parse TOML
		let config: SolverConfig = toml::from_str(&substituted_content)
			.map_err(|e| ConfigError::ParseError(e.to_string()))?;

		Ok(config)
	}

	/// Substitute environment variables in configuration content.
	///
	/// Processes the configuration file content and replaces all `${VAR_NAME}`
	/// patterns with the corresponding environment variable values. All referenced
	/// environment variables must be present or an error is returned.
	///
	/// # Arguments
	/// * `content` - Configuration file content with potential environment variable references
	///
	/// # Returns
	/// Configuration content with all environment variables substituted
	///
	/// # Errors
	/// Returns error if any referenced environment variable is not found
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

	/// Apply environment variable overrides to specific configuration settings.
	///
	/// Checks for environment variables with the configured prefix that should
	/// override specific configuration values after the base configuration is loaded.
	/// This provides a way to override configuration at runtime without modifying
	/// the configuration file. Supported overrides include log level, HTTP port,
	/// and metrics port.
	///
	/// # Arguments
	/// * `config` - Mutable reference to the configuration to apply overrides to
	///
	/// # Returns
	/// Success if all overrides are applied successfully
	///
	/// # Errors
	/// Returns error if environment variable values are invalid (e.g., non-numeric ports)
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

	/// Validate the loaded configuration for required settings and plugins.
	///
	/// Performs validation checks on the configuration to ensure that all
	/// required plugins are enabled and settings are valid. The solver requires
	/// at least one enabled delivery plugin and one enabled state plugin to
	/// function properly.
	///
	/// # Arguments
	/// * `config` - The configuration to validate
	///
	/// # Returns
	/// Success if validation passes
	///
	/// # Errors
	/// Returns error if required plugins are missing or configuration is invalid
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
