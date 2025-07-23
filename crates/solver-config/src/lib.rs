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

		// Process templates first
		let templated_content = self.process_templates(&content)?;

		// Substitute environment variables
		let substituted_content = self.substitute_env_vars(&templated_content)?;

		// Parse TOML
		let config: SolverConfig = toml::from_str(&substituted_content)
			.map_err(|e| ConfigError::ParseError(e.to_string()))?;

		Ok(config)
	}

	/// Process configuration templates for reusable plugin configurations.
	///
	/// Extracts template definitions from the [templates] section and replaces
	/// template references throughout the configuration. Templates allow sharing
	/// common configuration between multiple plugin instances.
	///
	/// Template references use the syntax: `template = "template_name"`
	///
	/// # Arguments
	/// * `content` - Raw configuration file content with potential template definitions
	///
	/// # Returns
	/// Configuration content with all template references expanded
	///
	/// # Errors
	/// Returns error if TOML parsing fails or template references are invalid
	fn process_templates(&self, content: &str) -> Result<String, ConfigError> {
		// First, parse the TOML to extract templates
		let mut value: toml::Value = toml::from_str(content).map_err(|e| {
			ConfigError::ParseError(format!(
				"Failed to parse TOML for template processing: {}",
				e
			))
		})?;

		// Extract templates section if it exists
		let templates = value
			.get("templates")
			.and_then(|t| t.as_table())
			.cloned()
			.unwrap_or_default();

		if templates.is_empty() {
			// No templates defined, return content as-is
			return Ok(content.to_string());
		}

		// Process templates recursively using a closure
		fn expand_value(
			value: &mut toml::Value,
			templates: &toml::map::Map<String, toml::Value>,
		) -> Result<(), ConfigError> {
			match value {
				toml::Value::Table(table) => {
					// Check if this table has a template reference
					if let Some(toml::Value::String(template_name)) = table.get("template") {
						if let Some(template_value) = templates.get(template_name) {
							if let Some(template_table) = template_value.as_table() {
								// Remove the template key
								table.remove("template");

								// Merge template values (template values first, so they can be overridden)
								for (key, val) in template_table.iter() {
									if !table.contains_key(key) {
										table.insert(key.clone(), val.clone());
									}
								}
							}
						}
					}

					// Recursively process all values in the table
					for (_, val) in table.iter_mut() {
						expand_value(val, templates)?;
					}
				}
				toml::Value::Array(array) => {
					// Recursively process array elements
					for val in array.iter_mut() {
						expand_value(val, templates)?;
					}
				}
				_ => {} // Other value types don't need processing
			}
			Ok(())
		}

		expand_value(&mut value, &templates)?;

		// Remove templates section
		if let Some(table) = value.as_table_mut() {
			table.remove("templates");
		}

		// Convert back to TOML string
		toml::to_string(&value).map_err(|e| {
			ConfigError::ParseError(format!("Failed to serialize processed TOML: {}", e))
		})
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
		let mut missing_plugins = Vec::new();
		if !config.plugins.delivery.values().any(|p| p.enabled) {
			missing_plugins.push("delivery");
		}
		if !config.plugins.state.values().any(|p| p.enabled) {
			missing_plugins.push("state");
		}
		if !config.plugins.settlement.values().any(|p| p.enabled) {
			missing_plugins.push("settlement");
		}
		if !config.plugins.discovery.values().any(|p| p.enabled) {
			missing_plugins.push("discovery");
		}

		if !missing_plugins.is_empty() {
			return Err(ConfigError::ValidationError(format!(
				"At least one plugin must be enabled for each of: {}",
				missing_plugins.join(", ")
			)));
		}

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::io::Write;
	use tempfile::NamedTempFile;

	#[tokio::test]
	async fn test_template_processing() {
		let config_content = r#"
[templates.common_discovery]
plugin_type = "eip7683_onchain"

[templates.common_discovery_config]
poll_interval_ms = 3000
enable_historical_sync = false

[templates.common_delivery]
plugin_type = "evm_alloy"

[templates.common_delivery_config]
max_retries = 3
timeout_ms = 30000
enable_eip1559 = true

[solver]
name = "test-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.discovery.origin]
enabled = true
template = "common_discovery"

[plugins.discovery.origin.config]
template = "common_discovery_config"
chain_id = 1
rpc_url = "http://localhost:8545"

[plugins.delivery.origin]
enabled = true
template = "common_delivery"

[plugins.delivery.origin.config]
template = "common_delivery_config"
chain_id = 1
rpc_url = "http://localhost:8545"
private_key = "0x123"

[plugins.state.memory]
enabled = true
plugin_type = "memory"

[plugins.state.memory.config]
max_entries = 1000

[plugins.order.test_order]
enabled = true
plugin_type = "test_order"

[plugins.order.test_order.config]

[plugins.settlement.test_settlement]
enabled = true
plugin_type = "test_settlement"

[plugins.settlement.test_settlement.config]

# Service configurations
[discovery]
realtime_monitoring = true

[state]
default_backend = "memory"
enable_metrics = false
cleanup_interval_seconds = 300
max_concurrent_operations = 100

[delivery]
strategy = "RoundRobin"
fallback_enabled = false
max_parallel_attempts = 2

[settlement]
default_strategy = "test_settlement"
fallback_strategies = []
profit_threshold_wei = "0"
"#;

		// Create a temporary file
		let mut temp_file = NamedTempFile::new().unwrap();
		temp_file.write_all(config_content.as_bytes()).unwrap();

		// Load configuration
		let loader = ConfigLoader::new().with_file(temp_file.path());
		let config = loader.load().await.unwrap();

		// Verify template values were applied
		let origin_discovery = &config.plugins.discovery["origin"];
		assert_eq!(origin_discovery.plugin_type, "eip7683_onchain");
		assert_eq!(
			origin_discovery.get_number("poll_interval_ms").unwrap(),
			3000
		);
		assert!(!origin_discovery.get_bool("enable_historical_sync").unwrap());
		assert_eq!(origin_discovery.get_number("chain_id").unwrap(), 1);
		assert_eq!(
			origin_discovery.get_string("rpc_url").unwrap(),
			"http://localhost:8545"
		);

		let origin_delivery = &config.plugins.delivery["origin"];
		assert_eq!(origin_delivery.plugin_type, "evm_alloy");
		assert_eq!(origin_delivery.get_number("max_retries").unwrap(), 3);
		assert_eq!(origin_delivery.get_number("timeout_ms").unwrap(), 30000);
		assert!(origin_delivery.get_bool("enable_eip1559").unwrap());
		assert_eq!(origin_delivery.get_string("private_key").unwrap(), "0x123");
	}

	#[test]
	fn test_template_processing_no_templates() {
		let content = r#"
[solver]
name = "test-solver"
log_level = "info"
"#;

		let loader = ConfigLoader::new();
		let result = loader.process_templates(content).unwrap();

		// Content should remain unchanged when no templates are defined
		assert!(result.contains("[solver]"));
		assert!(result.contains("name = \"test-solver\""));
	}

	#[test]
	fn test_template_processing_removes_template_section() {
		let content = r#"
[templates.test]
value = "test"

[solver]
name = "test-solver"
"#;

		let loader = ConfigLoader::new();
		let result = loader.process_templates(content).unwrap();

		// Templates section should be removed
		assert!(!result.contains("[templates"));
		assert!(!result.contains("value = \"test\""));
		assert!(result.contains("[solver]"));
	}

	#[tokio::test]
	async fn test_env_vars_in_templates() {
		// Set up environment variables
		env::set_var("TEST_PRIVATE_KEY", "0xabcdef123456");
		env::set_var("TEST_RPC_URL", "http://test.rpc.url");
		env::set_var("TEST_TOKEN_ADDRESS", "0x1234567890");

		let config_content = r#"
[templates.delivery_config]
private_key = "${TEST_PRIVATE_KEY}"
rpc_url = "${TEST_RPC_URL}"
max_retries = 5

[solver]
name = "test-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.discovery.test]
enabled = true
plugin_type = "test_discovery"

[plugins.discovery.test.config]

[plugins.delivery.test]
enabled = true
plugin_type = "evm_alloy"

[plugins.delivery.test.config]
template = "delivery_config"
chain_id = 1
token_address = "${TEST_TOKEN_ADDRESS}"

[plugins.state.memory]
enabled = true
plugin_type = "memory"

[plugins.state.memory.config]
max_entries = 1000

[plugins.order.test_order]
enabled = true
plugin_type = "test_order"

[plugins.order.test_order.config]

[plugins.settlement.test_settlement]
enabled = true
plugin_type = "test_settlement"

[plugins.settlement.test_settlement.config]

# Service configurations
[discovery]
realtime_monitoring = true

[state]
default_backend = "memory"
enable_metrics = false
cleanup_interval_seconds = 300
max_concurrent_operations = 100

[delivery]
strategy = "RoundRobin"
fallback_enabled = false
max_parallel_attempts = 1

[settlement]
default_strategy = "test_settlement"
fallback_strategies = []
profit_threshold_wei = "0"
monitor_interval_seconds = 30
"#;

		// Write configuration to a temporary file
		let mut temp_file = NamedTempFile::new().unwrap();
		temp_file.write_all(config_content.as_bytes()).unwrap();

		// Load configuration
		let loader = ConfigLoader::new().with_file(temp_file.path());
		let config = loader.load().await.unwrap();

		// Verify environment variables were substituted in template values
		let test_delivery = &config.plugins.delivery["test"];
		assert_eq!(
			test_delivery.get_string("private_key").unwrap(),
			"0xabcdef123456"
		);
		assert_eq!(
			test_delivery.get_string("rpc_url").unwrap(),
			"http://test.rpc.url"
		);
		assert_eq!(test_delivery.get_number("max_retries").unwrap(), 5);
		assert_eq!(
			test_delivery.get_string("token_address").unwrap(),
			"0x1234567890"
		);

		// Clean up
		env::remove_var("TEST_PRIVATE_KEY");
		env::remove_var("TEST_RPC_URL");
		env::remove_var("TEST_TOKEN_ADDRESS");
	}

	#[tokio::test]
	async fn test_env_var_override_template_value() {
		// Set up environment variable
		env::set_var("TEST_OVERRIDE_VALUE", "overridden");

		let config_content = r#"
[templates.test_template]
value1 = "${TEST_OVERRIDE_VALUE}"
value2 = "template_default"

[solver]
name = "test-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.discovery.test]
enabled = true
plugin_type = "test_plugin"

[plugins.discovery.test.config]
template = "test_template"
value1 = "explicit_override"
value3 = "additional_value"

[plugins.state.memory]
enabled = true
plugin_type = "memory"

[plugins.state.memory.config]
max_entries = 1000

[plugins.delivery.test]
enabled = true
plugin_type = "test_delivery"

[plugins.delivery.test.config]

[plugins.order.test_order]
enabled = true
plugin_type = "test_order"

[plugins.order.test_order.config]

[plugins.settlement.test_settlement]
enabled = true
plugin_type = "test_settlement"

[plugins.settlement.test_settlement.config]

# Service configurations
[discovery]
realtime_monitoring = true

[state]
default_backend = "memory"
enable_metrics = false
cleanup_interval_seconds = 300
max_concurrent_operations = 100

[delivery]
strategy = "RoundRobin"
fallback_enabled = false
max_parallel_attempts = 1

[settlement]
default_strategy = "test_settlement"
fallback_strategies = []
profit_threshold_wei = "0"
monitor_interval_seconds = 30
"#;

		// Write configuration to a temporary file
		let mut temp_file = NamedTempFile::new().unwrap();
		temp_file.write_all(config_content.as_bytes()).unwrap();

		// Load configuration
		let loader = ConfigLoader::new().with_file(temp_file.path());
		let config = loader.load().await.unwrap();

		// Verify explicit values override template values
		let test_discovery = &config.plugins.discovery["test"];
		assert_eq!(
			test_discovery.get_string("value1").unwrap(),
			"explicit_override"
		);
		assert_eq!(
			test_discovery.get_string("value2").unwrap(),
			"template_default"
		);
		assert_eq!(
			test_discovery.get_string("value3").unwrap(),
			"additional_value"
		);

		// Clean up
		env::remove_var("TEST_OVERRIDE_VALUE");
	}

	#[tokio::test]
	async fn test_env_vars_runtime_overrides() {
		// Set up runtime override environment variables
		env::set_var("SOLVER_LOG_LEVEL", "debug");
		env::set_var("SOLVER_HTTP_PORT", "9999");
		env::set_var("SOLVER_METRICS_PORT", "8888");

		let config_content = r#"
[solver]
name = "test-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.discovery.test]
enabled = true
plugin_type = "test_discovery"

[plugins.discovery.test.config]

[plugins.state.memory]
enabled = true
plugin_type = "memory"

[plugins.state.memory.config]
max_entries = 1000

[plugins.delivery.test]
enabled = true
plugin_type = "test_delivery"

[plugins.delivery.test.config]

[plugins.order.test_order]
enabled = true
plugin_type = "test_order"

[plugins.order.test_order.config]

[plugins.settlement.test_settlement]
enabled = true
plugin_type = "test_settlement"

[plugins.settlement.test_settlement.config]

# Service configurations
[discovery]
realtime_monitoring = true

[state]
default_backend = "memory"
enable_metrics = false
cleanup_interval_seconds = 300
max_concurrent_operations = 100

[delivery]
strategy = "RoundRobin"
fallback_enabled = false
max_parallel_attempts = 1

[settlement]
default_strategy = "test_settlement"
fallback_strategies = []
profit_threshold_wei = "0"
monitor_interval_seconds = 30
"#;

		// Write configuration to a temporary file
		let mut temp_file = NamedTempFile::new().unwrap();
		temp_file.write_all(config_content.as_bytes()).unwrap();

		// Load configuration
		let loader = ConfigLoader::new().with_file(temp_file.path());
		let config = loader.load().await.unwrap();

		// Verify runtime overrides were applied
		assert_eq!(config.solver.log_level, "debug");
		assert_eq!(config.solver.http_port, 9999);
		assert_eq!(config.solver.metrics_port, 8888);

		// Clean up
		env::remove_var("SOLVER_LOG_LEVEL");
		env::remove_var("SOLVER_HTTP_PORT");
		env::remove_var("SOLVER_METRICS_PORT");
	}

	#[tokio::test]
	async fn test_missing_env_var_error() {
		let config_content = r#"
[templates.error_template]
missing_var = "${MISSING_ENV_VAR}"

[solver]
name = "test-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.discovery.test]
enabled = true
plugin_type = "test_plugin"

[plugins.discovery.test.config]
template = "error_template"

[plugins.state.memory]
enabled = true
plugin_type = "memory"

[plugins.state.memory.config]
max_entries = 1000
"#;

		// Write configuration to a temporary file
		let mut temp_file = NamedTempFile::new().unwrap();
		temp_file.write_all(config_content.as_bytes()).unwrap();

		// Load configuration should fail due to missing env var
		let loader = ConfigLoader::new().with_file(temp_file.path());
		let result = loader.load().await;

		assert!(result.is_err());
		match result {
			Err(ConfigError::EnvVarNotFound(var)) => {
				assert_eq!(var, "MISSING_ENV_VAR");
			}
			_ => panic!("Expected EnvVarNotFound error"),
		}
	}

	#[tokio::test]
	async fn test_env_prefix_customization() {
		// Set up environment variable with custom prefix
		env::set_var("CUSTOM_LOG_LEVEL", "warn");

		let config_content = r#"
[solver]
name = "test-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.discovery.test]
enabled = true
plugin_type = "test_discovery"

[plugins.discovery.test.config]

[plugins.state.memory]
enabled = true
plugin_type = "memory"

[plugins.state.memory.config]
max_entries = 1000

[plugins.delivery.test]
enabled = true
plugin_type = "test_delivery"

[plugins.delivery.test.config]

[plugins.order.test_order]
enabled = true
plugin_type = "test_order"

[plugins.order.test_order.config]

[plugins.settlement.test_settlement]
enabled = true
plugin_type = "test_settlement"

[plugins.settlement.test_settlement.config]

# Service configurations
[discovery]
realtime_monitoring = true

[state]
default_backend = "memory"
enable_metrics = false
cleanup_interval_seconds = 300
max_concurrent_operations = 100

[delivery]
strategy = "RoundRobin"
fallback_enabled = false
max_parallel_attempts = 1

[settlement]
default_strategy = "test_settlement"
fallback_strategies = []
profit_threshold_wei = "0"
monitor_interval_seconds = 30
"#;

		// Write configuration to a temporary file
		let mut temp_file = NamedTempFile::new().unwrap();
		temp_file.write_all(config_content.as_bytes()).unwrap();

		// Load configuration with custom prefix
		let loader = ConfigLoader::new()
			.with_file(temp_file.path())
			.with_env_prefix("CUSTOM_");
		let config = loader.load().await.unwrap();

		// Verify custom prefix was used
		assert_eq!(config.solver.log_level, "warn");

		// Clean up
		env::remove_var("CUSTOM_LOG_LEVEL");
	}
}
