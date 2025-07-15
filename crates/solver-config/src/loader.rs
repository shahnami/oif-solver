//! Configuration loading from files and environment.

use crate::types::*;
use anyhow::{Context, Result};
use solver_types::chains::ChainId;
use std::path::Path;
use tracing::{debug, info};

/// Configuration loader
pub struct ConfigLoader;

impl ConfigLoader {
	/// Load configuration from file
	pub fn from_file<P: AsRef<Path>>(path: P) -> Result<SolverConfig> {
		let path = path.as_ref();
		info!("Loading configuration from {:?}", path);

		let contents = std::fs::read_to_string(path)
			.with_context(|| format!("Failed to read config file: {:?}", path))?;

		let config = match path.extension().and_then(|s| s.to_str()) {
			Some("toml") => Self::from_toml(&contents)?,
			Some("json") => Self::from_json(&contents)?,
			Some("yaml") | Some("yml") => Self::from_yaml(&contents)?,
			_ => anyhow::bail!("Unsupported config format: {:?}", path),
		};

		Self::validate_config(&config)?;
		Ok(config)
	}

	/// Load from TOML string
	pub fn from_toml(contents: &str) -> Result<SolverConfig> {
		toml::from_str(contents).map_err(|e| anyhow::anyhow!("Failed to parse TOML: {}", e))
	}

	/// Load from JSON string
	pub fn from_json(contents: &str) -> Result<SolverConfig> {
		serde_json::from_str(contents).context("Failed to parse JSON")
	}

	/// Load from YAML string
	pub fn from_yaml(contents: &str) -> Result<SolverConfig> {
		serde_yaml::from_str(contents).context("Failed to parse YAML")
	}

	/// Load from environment variables with optional file override
	pub fn from_env_and_file(file_path: Option<&Path>) -> Result<SolverConfig> {
		// Start with default config
		let mut config = if let Some(path) = file_path {
			Self::from_file(path)?
		} else {
			SolverConfig::default()
		};

		// Override with environment variables
		Self::apply_env_overrides(&mut config)?;

		Self::validate_config(&config)?;
		Ok(config)
	}

	/// Apply environment variable overrides
	fn apply_env_overrides(config: &mut SolverConfig) -> Result<()> {
		// Override private key
		if let Ok(key) = std::env::var("SOLVER_PRIVATE_KEY") {
			debug!("Overriding private key from environment");
			config.solver.private_key = key;
		}

		// Override RPC URLs
		for (chain_id_str, url) in std::env::vars() {
			if let Some(chain_id) = chain_id_str.strip_prefix("RPC_URL_") {
				if let Ok(id) = chain_id.parse::<u64>() {
					debug!("Overriding RPC URL for chain {} from environment", id);
					if let Some(chain_config) = config.chains.get_mut(&ChainId(id)) {
						chain_config.rpc_url = url;
					}
				}
			}
		}

		// Override API keys
		if let Ok(key) = std::env::var("RPC_API_KEY") {
			debug!("Overriding API key from environment");
			if let Some(rpc) = config.delivery.services.get_mut("rpc") {
				rpc.api_key = key;
			}
		}

		Ok(())
	}

	/// Validate configuration
	fn validate_config(config: &SolverConfig) -> Result<()> {
		// Check private key format
		if !config.solver.private_key.starts_with("0x") {
			anyhow::bail!("Private key must start with 0x");
		}

		// Check required chains are configured
		for chain_id in &config.discovery.monitor_chains {
			if !config.chains.contains_key(chain_id) {
				anyhow::bail!("Chain {} in monitor_chains but not configured", chain_id);
			}
		}

		// Check settlement configuration
		if !config.settlement.strategies.is_empty()
			&& !config
				.settlement
				.strategies
				.contains_key(&config.settlement.default_type)
		{
			anyhow::bail!(
				"Default settlement type '{}' not configured",
				config.settlement.default_type
			);
		}

		// Check delivery configuration
		if !config.delivery.services.is_empty()
			&& !config
				.delivery
				.services
				.contains_key(&config.delivery.default_service)
		{
			anyhow::bail!(
				"Default delivery service '{}' not configured",
				config.delivery.default_service
			);
		}

		Ok(())
	}
}

/// Load configuration from standard locations
pub fn load_config() -> Result<SolverConfig> {
	// Check for config file in order:
	// 1. Environment variable CONFIG_FILE
	// 2. ./config.toml
	// 3. /etc/oif-solver/config.toml
	// 4. Default config with env overrides

	if let Ok(path) = std::env::var("CONFIG_FILE") {
		return ConfigLoader::from_env_and_file(Some(Path::new(&path)));
	}

	let paths = [
		"./config.toml",
		"./config/solver.toml",
		"/etc/oif-solver/config.toml",
	];

	for path in &paths {
		if Path::new(path).exists() {
			return ConfigLoader::from_env_and_file(Some(Path::new(path)));
		}
	}

	// No config file found, use defaults with env overrides
	ConfigLoader::from_env_and_file(None)
}

#[cfg(test)]
mod tests {
	use solver_types::Address;

	use super::*;

	#[test]
	fn test_default_config() {
		let config = SolverConfig::default();
		assert_eq!(config.solver.name, "oif-solver");
		assert_eq!(config.settlement.default_type, "Direct");
	}

	#[test]
	fn test_toml_parsing() {
		let toml = r#"
[solver]
name = "test-solver"
private_key = "0x123"

[chains]

[discovery]
monitor_chains = []
start_blocks = {}
poll_interval_secs = 12
enable_offchain = false
offchain_endpoints = []

[settlement]
default_type = "Direct"
strategies = {}
poll_interval_secs = 30
max_attempts = 5

[state]
storage_backend = "memory"
max_queue_size = 10000
recover_on_startup = true

[delivery]
default_service = "rpc"
services = {}

[strategy.profitability]
min_profit_bps = 50
include_gas_costs = true
price_slippage_tolerance = 0.01

[strategy.risk]
blocked_tokens = []

[strategy.fallback]
enabled = false
delay_before_fallback_secs = 300
strategies = []

[monitoring]
enabled = true
metrics_port = 9090
health_port = 8080
log_level = "info"
"#;

		let config = ConfigLoader::from_toml(toml).unwrap();
		assert_eq!(config.solver.name, "test-solver");
		assert_eq!(config.monitoring.metrics_port, 9090);
	}

	#[test]
	fn test_toml_parsing_with_chain_id_maps() {
		let toml = r#"
[solver]
name = "test-solver"
private_key = "0x123"

[chains]
1 = { name = "Ethereum", rpc_url = "https://eth.example.com", confirmations = 12, block_time = 12, contracts = { custom = {} } }
42161 = { name = "Arbitrum", rpc_url = "https://arb.example.com", confirmations = 1, block_time = 1, contracts = { custom = {} } }

[discovery]
monitor_chains = [1, 42161]
poll_interval_secs = 12
enable_offchain = false
offchain_endpoints = []

[discovery.start_blocks]
1 = 18000000
42161 = 150000000

[settlement]
default_type = "Direct"
strategies = {}
poll_interval_secs = 30
max_attempts = 5

[state]
storage_backend = "memory"
max_queue_size = 10000
recover_on_startup = true

[delivery]
default_service = "rpc"

[delivery.services.rpc]
api_key = "test_key"
max_retries = 3

[delivery.services.rpc.endpoints]
1 = "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
42161 = "https://arb-mainnet.g.alchemy.com/v2/YOUR_KEY"

[delivery.services.rpc.gas_strategy]
type = "fast"

[strategy.profitability]
min_profit_bps = 50
include_gas_costs = true
price_slippage_tolerance = 0.01

[strategy.risk]
max_order_value_usd = 100000.0
blocked_tokens = []

[strategy.fallback]
enabled = false
delay_before_fallback_secs = 300
strategies = []

[monitoring]
enabled = true
metrics_port = 9090
health_port = 8080
log_level = "info"
"#;

		let config = ConfigLoader::from_toml(toml).unwrap();
		assert_eq!(config.solver.name, "test-solver");
		assert_eq!(config.chains.len(), 2);
		assert!(config.chains.contains_key(&ChainId(1)));
		assert!(config.chains.contains_key(&ChainId(42161)));

		// Check discovery start blocks
		assert_eq!(
			config.discovery.start_blocks.get(&ChainId(1)),
			Some(&18000000)
		);
		assert_eq!(
			config.discovery.start_blocks.get(&ChainId(42161)),
			Some(&150000000)
		);

		// Check delivery endpoints
		let rpc = config.delivery.services.get("rpc").unwrap();
		assert_eq!(rpc.endpoints.len(), 2);
		assert!(rpc.endpoints.contains_key(&ChainId(1)));
		assert!(rpc.endpoints.contains_key(&ChainId(42161)));
	}

	#[test]
	fn test_settlement_strategies_with_chain_id_maps() {
		let toml = r#"
[solver]
name = "test-solver"
private_key = "0x123"

[chains]

[discovery]
monitor_chains = []
start_blocks = {}
poll_interval_secs = 12
enable_offchain = false
offchain_endpoints = []

[settlement]
default_type = "ArbitrumBroadcaster"
poll_interval_secs = 30
max_attempts = 5

[settlement.strategies.ArbitrumBroadcaster]
poll_interval_secs = 60
max_wait_secs = 600

[settlement.strategies.ArbitrumBroadcaster.broadcaster_addresses]
1 = "0x1111111111111111111111111111111111111111"
42161 = "0x2222222222222222222222222222222222222222"

[state]
storage_backend = "memory"
max_queue_size = 10000
recover_on_startup = true

[delivery]
default_service = "rpc"
services = {}

[strategy.profitability]
min_profit_bps = 50
include_gas_costs = true
price_slippage_tolerance = 0.01

[strategy.risk]
blocked_tokens = []

[strategy.fallback]
enabled = false
delay_before_fallback_secs = 300
strategies = []

[monitoring]
enabled = true
metrics_port = 9090
health_port = 8080
log_level = "info"
"#;

		let config = ConfigLoader::from_toml(toml).unwrap();
		assert_eq!(config.settlement.default_type, "ArbitrumBroadcaster");

		// Check that the strategies field contains ArbitrumBroadcaster config
		let arb_config = config
			.settlement
			.strategies
			.get("ArbitrumBroadcaster")
			.unwrap();

		// Verify the JSON value contains the expected structure
		assert!(arb_config.get("poll_interval_secs").is_some());
		assert!(arb_config.get("max_wait_secs").is_some());
		assert!(arb_config.get("broadcaster_addresses").is_some());

		// Check that broadcaster_addresses has the expected chain entries
		let broadcaster_addrs = arb_config.get("broadcaster_addresses").unwrap();
		assert!(broadcaster_addrs.get("1").is_some());
		assert!(broadcaster_addrs.get("42161").is_some());
	}

	#[test]
	fn test_json_parsing() {
		let json = r#"{
            "solver": {
                "name": "test-solver",
                "private_key": "0x123"
            },
            "chains": {},
            "discovery": {
                "monitor_chains": [],
                "start_blocks": {},
                "poll_interval_secs": 12,
                "enable_offchain": false,
                "offchain_endpoints": []
            },
            "settlement": {
                "default_type": "Direct",
                "strategies": {},
                "poll_interval_secs": 30,
                "max_attempts": 5
            },
            "state": {
                "storage_backend": "memory",
                "max_queue_size": 10000,
                "recover_on_startup": true
            },
            "delivery": {
                "default_service": "rpc",
                "services": {}
            },
            "strategy": {
                "profitability": {
                    "min_profit_bps": 50,
                    "include_gas_costs": true,
                    "price_slippage_tolerance": 0.01
                },
                "risk": {
                    "blocked_tokens": []
                },
                "fallback": {
                    "enabled": false,
                    "delay_before_fallback_secs": 300,
                    "strategies": []
                }
            },
            "monitoring": {
                "enabled": true,
                "metrics_port": 9090,
                "health_port": 8080,
                "log_level": "info"
            }
        }"#;

		let config = ConfigLoader::from_json(json).unwrap();
		assert_eq!(config.solver.name, "test-solver");
		assert_eq!(config.monitoring.metrics_port, 9090);
	}

	#[test]
	fn test_validation_private_key() {
		let mut config = SolverConfig::default();
		config.solver.private_key = "invalid_key".to_string();

		let result = ConfigLoader::validate_config(&config);
		assert!(result.is_err());
		assert!(result
			.unwrap_err()
			.to_string()
			.contains("Private key must start with 0x"));
	}

	#[test]
	fn test_full_config_with_settlement_strategies() {
		let toml = r#"
[solver]
name = "production-solver"
private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

# Chain configurations using numeric keys
[chains]
1 = { name = "Ethereum", rpc_url = "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY", confirmations = 12, block_time = 12, contracts = { settler = "0x1234567890123456789012345678901234567890", custom = {} } }
42161 = { name = "Arbitrum", rpc_url = "https://arb-mainnet.g.alchemy.com/v2/YOUR_KEY", confirmations = 1, block_time = 1, contracts = { settler = "0x0987654321098765432109876543210987654321", filler = "0xabcdefabcdefabcdefabcdefabcdefabcdefabcd", custom = {} } }
8453 = { name = "Base", rpc_url = "https://base-mainnet.g.alchemy.com/v2/YOUR_KEY", confirmations = 1, block_time = 2, contracts = { custom = {} } }

[discovery]
monitor_chains = [1, 42161, 8453]
poll_interval_secs = 12
enable_offchain = true
offchain_endpoints = ["https://api.example.com/orders"]

[discovery.start_blocks]
1 = 18000000
42161 = 150000000
8453 = 5000000

[settlement]
default_type = "ArbitrumBroadcaster"
poll_interval_secs = 30
max_attempts = 5

[settlement.strategies.ArbitrumBroadcaster]
poll_interval_secs = 60
max_wait_secs = 600

[settlement.strategies.ArbitrumBroadcaster.broadcaster_addresses]
1 = "0x1111111111111111111111111111111111111111"
42161 = "0x2222222222222222222222222222222222222222"
8453 = "0x3333333333333333333333333333333333333333"

[settlement.strategies.ArbitrumBroadcaster.settler_addresses]
1 = "0x4444444444444444444444444444444444444444"
42161 = "0x5555555555555555555555555555555555555555"
8453 = "0x6666666666666666666666666666666666666666"

[state]
storage_backend = "file"
storage_path = "./data/solver-state"
max_queue_size = 10000
recover_on_startup = true

[delivery]
default_service = "rpc"

[delivery.services.rpc]
api_key = "YOUR_ALCHEMY_KEY"
max_retries = 3

[delivery.services.rpc.endpoints]
1 = "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
42161 = "https://arb-mainnet.g.alchemy.com/v2/YOUR_KEY"
8453 = "https://base-mainnet.g.alchemy.com/v2/YOUR_KEY"

[delivery.services.rpc.gas_strategy]
type = "eip1559"
max_priority_fee = 2000000000

[delivery.services.openzeppelin]
api_key = "YOUR_OZ_KEY"
max_retries = 5

[delivery.services.openzeppelin.endpoints]
1 = "https://api.defender.openzeppelin.com/autotasks/YOUR_TASK/runs/webhook/YOUR_KEY"
42161 = "https://api.defender.openzeppelin.com/autotasks/YOUR_TASK_ARB/runs/webhook/YOUR_KEY"

[delivery.services.openzeppelin.gas_strategy]
type = "custom"
multiplier = 1.5

[strategy.profitability]
min_profit_bps = 50
include_gas_costs = true
price_slippage_tolerance = 0.01

[strategy.risk]
max_order_value_usd = 100000.0
max_exposure_per_chain = 500000.0
blocked_tokens = ["0xdead000000000000000000000000000000000000"]

[strategy.fallback]
enabled = true
delay_before_fallback_secs = 300
strategies = ["retry_with_higher_gas", "use_alternative_route"]

[monitoring]
enabled = true
metrics_port = 9090
health_port = 8080
log_level = "info"
"#;

		// Parse the configuration
		let config = ConfigLoader::from_toml(toml).expect("Failed to parse configuration");

		// Verify basic settings
		assert_eq!(config.solver.name, "production-solver");

		// Verify chains are parsed correctly with numeric keys
		assert_eq!(config.chains.len(), 3);
		assert!(config.chains.contains_key(&ChainId(1)));
		assert!(config.chains.contains_key(&ChainId(42161)));
		assert!(config.chains.contains_key(&ChainId(8453)));

		let eth_chain = config.chains.get(&ChainId(1)).unwrap();
		assert_eq!(eth_chain.name, "Ethereum");
		assert_eq!(
			format!("{:?}", eth_chain.contracts.settler.unwrap()),
			format!(
				"{:?}",
				"0x1234567890123456789012345678901234567890"
					.parse::<Address>()
					.unwrap()
			)
		);

		// Verify discovery configuration
		assert_eq!(config.discovery.monitor_chains.len(), 3);
		assert_eq!(config.discovery.start_blocks.len(), 3);
		assert_eq!(
			config.discovery.start_blocks.get(&ChainId(1)),
			Some(&18000000)
		);

		// Verify delivery services
		assert_eq!(config.delivery.services.len(), 2);

		let rpc = config.delivery.services.get("rpc").unwrap();
		assert_eq!(rpc.endpoints.len(), 3);
		assert!(rpc.endpoints.contains_key(&ChainId(1)));
		assert!(rpc.endpoints.contains_key(&ChainId(42161)));
		assert!(rpc.endpoints.contains_key(&ChainId(8453)));

		let oz = config.delivery.services.get("openzeppelin").unwrap();
		assert_eq!(oz.endpoints.len(), 2);

		// Verify settlement strategies
		assert_eq!(config.settlement.default_type, "ArbitrumBroadcaster");
		assert!(config
			.settlement
			.strategies
			.contains_key("ArbitrumBroadcaster"));

		let arb_strategy = config
			.settlement
			.strategies
			.get("ArbitrumBroadcaster")
			.unwrap();
		assert!(arb_strategy.get("broadcaster_addresses").is_some());
		assert!(arb_strategy.get("settler_addresses").is_some());

		// Test round-trip serialization
		let serialized = toml::to_string(&config).expect("Failed to serialize config");
		let reparsed = ConfigLoader::from_toml(&serialized).expect("Failed to reparse config");

		// Verify key fields survive round-trip
		assert_eq!(reparsed.chains.len(), config.chains.len());
		assert_eq!(
			reparsed
				.delivery
				.services
				.get("rpc")
				.unwrap()
				.endpoints
				.len(),
			3
		);
		assert_eq!(reparsed.discovery.start_blocks.len(), 3);
	}
}
