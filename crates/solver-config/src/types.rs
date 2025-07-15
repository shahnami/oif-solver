//! Configuration types for the solver.

use crate::serde_helpers::{
	deserialize_chain_id_map, deserialize_chain_id_map_generic, serialize_chain_id_map,
	serialize_chain_id_map_generic,
};
use serde::{Deserialize, Serialize};
use solver_types::{chains::ChainId, common::Address};
use std::collections::HashMap;
use std::path::PathBuf;

/// Complete solver configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolverConfig {
	/// Solver identity and credentials
	pub solver: SolverSettings,
	/// Chain configurations
	#[serde(
		deserialize_with = "deserialize_chain_id_map_generic",
		serialize_with = "serialize_chain_id_map_generic"
	)]
	pub chains: HashMap<ChainId, ChainConfig>,
	/// Order discovery settings
	pub discovery: DiscoveryConfig,
	/// Settlement configuration
	pub settlement: SettlementConfig,
	/// State management settings
	pub state: StateConfig,
	/// Delivery service settings
	pub delivery: DeliveryConfig,
	/// Strategy settings
	pub strategy: StrategyConfig,
	/// Monitoring and metrics
	pub monitoring: MonitoringConfig,
}

/// Solver identity and credentials
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolverSettings {
	/// Solver name/identifier
	pub name: String,
	/// Private key (or path to key file)
	pub private_key: String,
	/// Solver address (derived from private key if not specified)
	pub address: Option<Address>,
}

/// Chain-specific configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChainConfig {
	/// Chain name for logging
	pub name: String,
	/// RPC endpoint URL
	pub rpc_url: String,
	/// WebSocket endpoint (optional)
	pub ws_url: Option<String>,
	/// Block confirmations required
	pub confirmations: u64,
	/// Average block time in seconds
	pub block_time: u64,
	/// Contract addresses on this chain
	pub contracts: ChainContracts,
}

/// Contract addresses for a chain
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ChainContracts {
	/// EIP-7683 settler contract
	pub settler: Option<Address>,
	/// Filler contract
	pub filler: Option<Address>,
	/// Oracle/broadcaster contract
	pub oracle: Option<Address>,
	/// Custom contracts
	pub custom: HashMap<String, Address>,
}

/// Discovery configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoveryConfig {
	/// Chains to monitor for orders
	pub monitor_chains: Vec<ChainId>,
	/// Start block for each chain (0 for latest)
	#[serde(
		deserialize_with = "deserialize_chain_id_map_generic",
		serialize_with = "serialize_chain_id_map_generic"
	)]
	pub start_blocks: HashMap<ChainId, u64>,
	/// Event polling interval in seconds
	pub poll_interval_secs: u64,
	/// Enable off-chain order sources
	pub enable_offchain: bool,
	/// Off-chain API endpoints
	pub offchain_endpoints: Vec<String>,
}

/// Settlement configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SettlementConfig {
	/// Default settlement type
	pub default_type: String, // "ArbitrumBroadcaster" or "Direct"
	/// Settlement-specific configurations
	pub strategies: HashMap<String, serde_json::Value>,
	/// Settlement polling interval
	pub poll_interval_secs: u64,
	/// Maximum settlement attempts
	pub max_attempts: u32,
}

/// State management configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StateConfig {
	/// Storage backend: "memory" or "file"
	pub storage_backend: String,
	/// Storage path for file backend
	pub storage_path: Option<PathBuf>,
	/// Maximum queue size
	pub max_queue_size: usize,
	/// Enable state recovery on startup
	pub recover_on_startup: bool,
}

/// Delivery service configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeliveryConfig {
	/// Default delivery service
	pub default_service: String, // "rpc"
	/// Service configurations
	pub services: HashMap<String, DeliveryServiceConfig>,
}

/// Individual delivery service config
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeliveryServiceConfig {
	/// API key
	pub api_key: String,
	/// Endpoints by chain
	#[serde(
		deserialize_with = "deserialize_chain_id_map",
		serialize_with = "serialize_chain_id_map"
	)]
	pub endpoints: HashMap<ChainId, String>,
	/// Gas strategy
	pub gas_strategy: GasStrategyConfig,
	/// Max retries
	pub max_retries: u32,
}

/// Gas pricing strategy
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum GasStrategyConfig {
	#[serde(rename = "standard")]
	Standard,
	#[serde(rename = "fast")]
	Fast,
	#[serde(rename = "custom")]
	Custom { multiplier: f64 },
	#[serde(rename = "eip1559")]
	Eip1559 { max_priority_fee: u64 },
}

/// Strategy configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StrategyConfig {
	/// Profitability settings
	pub profitability: ProfitabilityConfig,
	/// Risk management
	pub risk: RiskConfig,
	/// Fallback strategies
	pub fallback: FallbackConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProfitabilityConfig {
	/// Minimum profit in basis points
	pub min_profit_bps: u16,
	/// Include gas costs in profit calculation
	pub include_gas_costs: bool,
	/// Price slippage tolerance
	pub price_slippage_tolerance: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RiskConfig {
	/// Maximum order value in USD
	pub max_order_value_usd: Option<f64>,
	/// Maximum exposure per chain
	pub max_exposure_per_chain: Option<f64>,
	/// Blocked token addresses
	pub blocked_tokens: Vec<Address>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FallbackConfig {
	/// Enable fallback strategies
	pub enabled: bool,
	/// Delay before fallback in seconds
	pub delay_before_fallback_secs: u64,
	/// Fallback strategy types
	pub strategies: Vec<String>,
}

/// Monitoring configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MonitoringConfig {
	/// Enable monitoring
	pub enabled: bool,
	/// Metrics port
	pub metrics_port: u16,
	/// Health check port
	pub health_port: u16,
	/// Log level
	pub log_level: String,
}

/// Default configuration
impl Default for SolverConfig {
	fn default() -> Self {
		Self {
			solver: SolverSettings {
				name: "oif-solver".to_string(),
				private_key: "0x...".to_string(),
				address: None,
			},
			chains: HashMap::new(),
			discovery: DiscoveryConfig {
				monitor_chains: vec![],
				start_blocks: HashMap::new(),
				poll_interval_secs: 12,
				enable_offchain: false,
				offchain_endpoints: vec![],
			},
			settlement: SettlementConfig {
				default_type: "Direct".to_string(),
				strategies: HashMap::new(),
				poll_interval_secs: 30,
				max_attempts: 5,
			},
			state: StateConfig {
				storage_backend: "memory".to_string(),
				storage_path: None,
				max_queue_size: 10_000,
				recover_on_startup: true,
			},
			delivery: DeliveryConfig {
				default_service: "rpc".to_string(),
				services: HashMap::new(),
			},
			strategy: StrategyConfig {
				profitability: ProfitabilityConfig {
					min_profit_bps: 50,
					include_gas_costs: true,
					price_slippage_tolerance: 0.01,
				},
				risk: RiskConfig {
					max_order_value_usd: Some(100_000.0),
					max_exposure_per_chain: Some(500_000.0),
					blocked_tokens: vec![],
				},
				fallback: FallbackConfig {
					enabled: false,
					delay_before_fallback_secs: 300,
					strategies: vec![],
				},
			},
			monitoring: MonitoringConfig {
				enabled: true,
				metrics_port: 9090,
				health_port: 8080,
				log_level: "info".to_string(),
			},
		}
	}
}

/// Configuration defaults for common chains
impl ChainConfig {
	/// Ethereum mainnet configuration
	pub fn ethereum() -> Self {
		Self {
			name: "Ethereum".to_string(),
			rpc_url: "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY".to_string(),
			ws_url: None,
			confirmations: 12,
			block_time: 12,
			contracts: ChainContracts::default(),
		}
	}

	/// Arbitrum One configuration
	pub fn arbitrum() -> Self {
		Self {
			name: "Arbitrum One".to_string(),
			rpc_url: "https://arb-mainnet.g.alchemy.com/v2/YOUR_KEY".to_string(),
			ws_url: None,
			confirmations: 1,
			block_time: 1,
			contracts: ChainContracts::default(),
		}
	}

	/// Base configuration
	pub fn base() -> Self {
		Self {
			name: "Base".to_string(),
			rpc_url: "https://base-mainnet.g.alchemy.com/v2/YOUR_KEY".to_string(),
			ws_url: None,
			confirmations: 1,
			block_time: 2,
			contracts: ChainContracts::default(),
		}
	}
}

impl Default for GasStrategyConfig {
	fn default() -> Self {
		Self::Standard
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_default_config() {
		let config = SolverConfig::default();
		assert_eq!(config.solver.name, "oif-solver");
		assert_eq!(config.settlement.default_type, "Direct");
		assert_eq!(config.state.storage_backend, "memory");
		assert_eq!(config.delivery.default_service, "rpc");
	}

	#[test]
	fn test_chain_config_defaults() {
		let eth = ChainConfig::ethereum();
		assert_eq!(eth.name, "Ethereum");
		assert_eq!(eth.confirmations, 12);
		assert_eq!(eth.block_time, 12);

		let arb = ChainConfig::arbitrum();
		assert_eq!(arb.name, "Arbitrum One");
		assert_eq!(arb.confirmations, 1);
		assert_eq!(arb.block_time, 1);
	}

	#[test]
	fn test_gas_strategy_serialization() {
		let strategy = GasStrategyConfig::Custom { multiplier: 1.5 };
		let json = serde_json::to_string(&strategy).unwrap();
		assert!(json.contains("\"type\":\"custom\""));
		assert!(json.contains("\"multiplier\":1.5"));
	}
}
