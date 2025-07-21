// solver-plugins/src/factory.rs - Final implementation with separate traits

use solver_types::plugins::*;
use std::collections::HashMap;
use std::sync::Arc;

use crate::delivery::{EvmEthersConfig, EvmEthersDeliveryPlugin};
use crate::discovery::{Eip7683OnchainConfig, Eip7683OnchainDiscoveryPlugin};
use crate::order::{create_eip7683_processor, Eip7683OrderPlugin};
use crate::settlement::DirectSettlementPlugin;
use crate::state::{FileConfig, FileStatePlugin, InMemoryConfig, InMemoryStatePlugin};

/// Global plugin factory instance
use std::sync::OnceLock;

static GLOBAL_FACTORY: OnceLock<PluginFactory> = OnceLock::new();

/// Get the global plugin factory
pub fn global_plugin_factory() -> &'static PluginFactory {
	GLOBAL_FACTORY.get_or_init(create_builtin_plugin_factory)
}

/// Initialize the global plugin factory with custom plugins
pub fn init_global_plugin_factory(factory: PluginFactory) {
	let _ = GLOBAL_FACTORY.set(factory);
}

/// Unified plugin factory registry
pub struct PluginFactory {
	state_factories: HashMap<String, Box<dyn StatePluginFactory>>,
	discovery_factories: HashMap<String, Box<dyn DiscoveryPluginFactory>>,
	delivery_factories: HashMap<String, Box<dyn DeliveryPluginFactory>>,
	settlement_factories: HashMap<String, Box<dyn SettlementPluginFactory>>,
	order_processor_factories: HashMap<String, Box<dyn OrderProcessorFactory>>,
}

impl PluginFactory {
	pub fn new() -> Self {
		Self {
			state_factories: HashMap::new(),
			discovery_factories: HashMap::new(),
			delivery_factories: HashMap::new(),
			settlement_factories: HashMap::new(),
			order_processor_factories: HashMap::new(),
		}
	}

	/// Register a state plugin factory
	pub fn register_state_factory<F>(&mut self, factory: F)
	where
		F: StatePluginFactory + 'static,
	{
		self.state_factories
			.insert(factory.plugin_type().to_string(), Box::new(factory));
	}

	/// Register a discovery plugin factory
	pub fn register_discovery_factory<F>(&mut self, factory: F)
	where
		F: DiscoveryPluginFactory + 'static,
	{
		self.discovery_factories
			.insert(factory.plugin_type().to_string(), Box::new(factory));
	}

	/// Register a delivery plugin factory
	pub fn register_delivery_factory<F>(&mut self, factory: F)
	where
		F: DeliveryPluginFactory + 'static,
	{
		self.delivery_factories
			.insert(factory.plugin_type().to_string(), Box::new(factory));
	}

	/// Register a settlement plugin factory
	pub fn register_settlement_factory<F>(&mut self, factory: F)
	where
		F: SettlementPluginFactory + 'static,
	{
		self.settlement_factories
			.insert(factory.plugin_type().to_string(), Box::new(factory));
	}

	/// Register an order processor factory
	pub fn register_order_processor_factory<F>(&mut self, factory: F)
	where
		F: OrderProcessorFactory + 'static,
	{
		self.order_processor_factories
			.insert(factory.plugin_type().to_string(), Box::new(factory));
	}

	/// Create a state plugin
	pub fn create_state_plugin(
		&self,
		plugin_type: &str,
		config: PluginConfig,
	) -> PluginResult<Arc<dyn StatePlugin>> {
		let factory = self.state_factories.get(plugin_type).ok_or_else(|| {
			PluginError::NotFound(format!("State plugin '{}' not found", plugin_type))
		})?;
		factory.create_plugin(config).map(Arc::from)
	}

	/// Create a discovery plugin
	pub fn create_discovery_plugin(
		&self,
		plugin_type: &str,
		config: PluginConfig,
	) -> PluginResult<Box<dyn DiscoveryPlugin>> {
		let factory = self.discovery_factories.get(plugin_type).ok_or_else(|| {
			PluginError::NotFound(format!("Discovery plugin '{}' not found", plugin_type))
		})?;
		factory.create_plugin(config)
	}

	/// Create a delivery plugin
	pub fn create_delivery_plugin(
		&self,
		plugin_type: &str,
		config: PluginConfig,
	) -> PluginResult<Box<dyn DeliveryPlugin>> {
		let factory = self.delivery_factories.get(plugin_type).ok_or_else(|| {
			PluginError::NotFound(format!("Delivery plugin '{}' not found", plugin_type))
		})?;
		factory.create_plugin(config)
	}

	/// Create a settlement plugin
	pub fn create_settlement_plugin(
		&self,
		plugin_type: &str,
		config: PluginConfig,
	) -> PluginResult<Box<dyn SettlementPlugin>> {
		let factory = self.settlement_factories.get(plugin_type).ok_or_else(|| {
			PluginError::NotFound(format!("Settlement plugin '{}' not found", plugin_type))
		})?;
		factory.create_plugin(config)
	}

	/// Create an order processor
	pub fn create_order_processor(
		&self,
		plugin_type: &str,
		config: PluginConfig,
	) -> PluginResult<Arc<dyn OrderProcessor>> {
		let factory = self
			.order_processor_factories
			.get(plugin_type)
			.ok_or_else(|| {
				PluginError::NotFound(format!("Order processor '{}' not found", plugin_type))
			})?;
		factory.create_processor(config)
	}

	/// List all available plugins
	pub fn list_available_plugins(&self) -> AvailablePlugins {
		AvailablePlugins {
			state_plugins: self.state_factories.keys().cloned().collect(),
			discovery_plugins: self.discovery_factories.keys().cloned().collect(),
			delivery_plugins: self.delivery_factories.keys().cloned().collect(),
			settlement_plugins: self.settlement_factories.keys().cloned().collect(),
			order_processors: self.order_processor_factories.keys().cloned().collect(),
		}
	}

	/// Get features supported by a state plugin
	pub fn get_state_plugin_features(&self, plugin_type: &str) -> Option<Vec<StateFeature>> {
		self.state_factories
			.get(plugin_type)
			.map(|factory| factory.supports_features())
	}

	/// Get chains supported by a discovery plugin
	pub fn get_discovery_plugin_chains(&self, plugin_type: &str) -> Option<Vec<ChainId>> {
		self.discovery_factories
			.get(plugin_type)
			.map(|factory| factory.supported_chains())
	}

	/// Get chains supported by a delivery plugin
	pub fn get_delivery_plugin_chains(&self, plugin_type: &str) -> Option<Vec<ChainId>> {
		self.delivery_factories
			.get(plugin_type)
			.map(|factory| factory.supported_chains())
	}

	/// Get chains supported by a settlement plugin
	pub fn get_settlement_plugin_chains(&self, plugin_type: &str) -> Option<Vec<ChainId>> {
		self.settlement_factories
			.get(plugin_type)
			.map(|factory| factory.supported_chains())
	}

	/// Check if a plugin type is available
	pub fn has_state_plugin(&self, plugin_type: &str) -> bool {
		self.state_factories.contains_key(plugin_type)
	}

	pub fn has_discovery_plugin(&self, plugin_type: &str) -> bool {
		self.discovery_factories.contains_key(plugin_type)
	}

	pub fn has_delivery_plugin(&self, plugin_type: &str) -> bool {
		self.delivery_factories.contains_key(plugin_type)
	}

	pub fn has_settlement_plugin(&self, plugin_type: &str) -> bool {
		self.settlement_factories.contains_key(plugin_type)
	}

	pub fn has_order_processor(&self, plugin_type: &str) -> bool {
		self.order_processor_factories.contains_key(plugin_type)
	}
}

impl Default for PluginFactory {
	fn default() -> Self {
		Self::new()
	}
}

#[derive(Debug, Clone)]
pub struct AvailablePlugins {
	pub state_plugins: Vec<String>,
	pub discovery_plugins: Vec<String>,
	pub delivery_plugins: Vec<String>,
	pub settlement_plugins: Vec<String>,
	pub order_processors: Vec<String>,
}

// State Factory Structs
#[derive(Default)]
pub struct FileStatePluginFactory;

#[derive(Default)]
pub struct InMemoryStatePluginFactory;

// Delivery Factory Structs
#[derive(Default)]
pub struct EvmEthersDeliveryPluginFactory;

// Discovery Factory Structs
#[derive(Default)]
pub struct Eip7683OnchainDiscoveryPluginFactory;

// Settlement Factory Structs
#[derive(Default)]
pub struct DirectSettlementPluginFactory;

// Order Processor Factory Structs
#[derive(Default)]
pub struct Eip7683OrderProcessorFactory;

// Settlement Factory Structs

/// Create a plugin factory with all built-in plugins registered
pub fn create_builtin_plugin_factory() -> PluginFactory {
	let mut factory = PluginFactory::new();

	// Register state plugins
	factory.register_state_factory(InMemoryStatePluginFactory);
	factory.register_state_factory(FileStatePluginFactory);

	// Register delivery plugins
	factory.register_delivery_factory(EvmEthersDeliveryPluginFactory);

	// Register discovery plugins
	factory.register_discovery_factory(Eip7683OnchainDiscoveryPluginFactory);

	// Register settlement plugins
	factory.register_settlement_factory(DirectSettlementPluginFactory);

	// Register order processors
	factory.register_order_processor_factory(Eip7683OrderProcessorFactory);

	factory
}

/// Factory for creating in-memory state plugins
impl StatePluginFactory for InMemoryStatePluginFactory {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn StatePlugin>> {
		let memory_config = InMemoryConfig {
			max_entries: config.get_number("max_entries").map(|n| n as usize),
			default_ttl: config
				.get_number("default_ttl_seconds")
				.map(|n| std::time::Duration::from_secs(n as u64)),
		};

		let plugin = InMemoryStatePlugin::with_config(memory_config);
		Ok(Box::new(plugin))
	}

	fn plugin_type(&self) -> &'static str {
		"memory"
	}

	fn supports_features(&self) -> Vec<StateFeature> {
		vec![StateFeature::TTL, StateFeature::AtomicOperations]
	}
}

impl StatePluginFactory for FileStatePluginFactory {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn StatePlugin>> {
		let file_config = FileConfig {
			storage_path: std::path::PathBuf::from(
				config
					.get_string("storage_path")
					.unwrap_or("./state".to_string()),
			),
			create_dirs: config.get_bool("create_dirs").unwrap_or(true),
			sync_on_write: config.get_bool("sync_on_write").unwrap_or(true),
		};

		let plugin = FileStatePlugin::with_config(file_config);
		Ok(Box::new(plugin))
	}

	fn plugin_type(&self) -> &'static str {
		"file"
	}

	fn supports_features(&self) -> Vec<StateFeature> {
		vec![
			StateFeature::TTL,
			StateFeature::Backup,
			StateFeature::Restore,
			StateFeature::AtomicOperations,
		]
	}
}

impl DeliveryPluginFactory for EvmEthersDeliveryPluginFactory {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn DeliveryPlugin>> {
		// Extract chain_id (required)
		let chain_id = config.get_number("chain_id").ok_or_else(|| {
			PluginError::InvalidConfiguration(
				"chain_id is required and must be a number".to_string(),
			)
		})? as ChainId;

		// Extract rpc_url (required)
		let rpc_url = config
			.get_string("rpc_url")
			.ok_or_else(|| PluginError::InvalidConfiguration("rpc_url is required".to_string()))?
			.to_string();

		// Extract private_key (required)
		let private_key = config
			.get_string("private_key")
			.ok_or_else(|| {
				PluginError::InvalidConfiguration("private_key is required".to_string())
			})?
			.to_string();

		// Extract optional parameters with defaults
		let max_retries = config.get_number("max_retries").unwrap_or(3) as u32;

		let timeout_ms = config.get_number("timeout_ms").unwrap_or(30000) as u64;

		let gas_price_multiplier = config
			.get_number("gas_price_multiplier")
			.map(|n| n as f64)
			.unwrap_or(1.1);

		let max_gas_price = config.get_number("max_gas_price").map(|n| n as u64);

		let enable_eip1559 = config.get_bool("enable_eip1559").unwrap_or(true);

		let confirmation_blocks = config.get_number("confirmation_blocks").unwrap_or(12) as u32;

		let nonce_management = config.get_bool("nonce_management").unwrap_or(true);

		let mempool_monitoring = config.get_bool("mempool_monitoring").unwrap_or(false);

		let max_pending_transactions = config
			.get_number("max_pending_transactions")
			.unwrap_or(1000) as usize;

		// Create the configuration
		let evm_config = EvmEthersConfig {
			chain_id,
			rpc_url,
			private_key,
			max_retries,
			timeout_ms,
			gas_price_multiplier,
			max_gas_price,
			enable_eip1559,
			confirmation_blocks,
			nonce_management,
			mempool_monitoring,
			max_pending_transactions,
		};

		// Create the plugin
		let plugin = EvmEthersDeliveryPlugin::with_config(evm_config);
		Ok(Box::new(plugin))
	}

	fn plugin_type(&self) -> &'static str {
		"evm_ethers"
	}

	fn supported_chains(&self) -> Vec<ChainId> {
		vec![
			1,     // Ethereum Mainnet
			10,    // Optimism
			56,    // BSC
			137,   // Polygon
			250,   // Fantom
			8453,  // Base
			31337, // Hardhat
			31338, // Hardhat 2
			42161, // Arbitrum One
			43114, // Avalanche
		]
	}
}

impl DiscoveryPluginFactory for Eip7683OnchainDiscoveryPluginFactory {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn DiscoveryPlugin>> {
		// Extract configuration
		let chain_id = config
			.get_number("chain_id")
			.ok_or_else(|| PluginError::InvalidConfiguration("chain_id is required".to_string()))?
			as ChainId;

		let rpc_url = config
			.get_string("rpc_url")
			.ok_or_else(|| PluginError::InvalidConfiguration("rpc_url is required".to_string()))?;

		let poll_interval_ms = config.get_number("poll_interval_ms").unwrap_or(2000) as u64;
		let start_block = config.get_number("start_block").map(|n| n as u64);

		// Create configuration struct
		let discovery_config = Eip7683OnchainConfig {
			chain_id,
			rpc_url,

			// Connection settings with sensible defaults
			timeout_ms: config.get_number("timeout_ms").unwrap_or(30000) as u64,
			max_retries: config.get_number("max_retries").unwrap_or(3) as u32,

			// Contract addresses - use provided contracts or extract from specific fields
			input_settler_addresses: config
				.get_array("input_settler_addresses")
				.unwrap_or_default()
				.iter()
				.map(|s| s.to_string())
				.collect(),

			output_settler_addresses: config
				.get_array("output_settler_addresses")
				.unwrap_or_default()
				.iter()
				.map(|s| s.to_string())
				.collect(),

			// Event monitoring flags - default to monitoring all events
			monitor_open: config.get_bool("monitor_open").unwrap_or(true),
			monitor_finalised: config.get_bool("monitor_finalised").unwrap_or(true),
			monitor_order_purchased: config.get_bool("monitor_order_purchased").unwrap_or(true),

			// Performance settings
			batch_size: config.get_number("batch_size").unwrap_or(100) as u32,
			poll_interval_ms,
			max_blocks_per_request: config.get_number("max_blocks_per_request").unwrap_or(1000)
				as u64,

			// Historical sync settings
			enable_historical_sync: config.get_bool("enable_historical_sync").unwrap_or(false),
			historical_start_block: start_block.or_else(|| {
				config
					.get_number("historical_start_block")
					.map(|n| n as u64)
			}),
		};

		let plugin = Eip7683OnchainDiscoveryPlugin::with_config(discovery_config);
		Ok(Box::new(plugin))
	}

	fn plugin_type(&self) -> &'static str {
		"eip7683_onchain"
	}

	fn supported_chains(&self) -> Vec<ChainId> {
		vec![
			1,     // Ethereum Mainnet
			10,    // Optimism
			56,    // BSC
			137,   // Polygon
			250,   // Fantom
			8453,  // Base
			42161, // Arbitrum One
			43114, // Avalanche
		]
	}

	fn source_type(&self) -> DiscoverySourceType {
		DiscoverySourceType::OnchainPolling
	}

	fn supports_historical(&self) -> bool {
		true
	}

	fn supports_realtime(&self) -> bool {
		true
	}
}

/// Factory for creating direct settlement plugins
impl SettlementPluginFactory for DirectSettlementPluginFactory {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn SettlementPlugin>> {
		let plugin = DirectSettlementPlugin::new();
		Ok(Box::new(plugin))
	}

	fn plugin_type(&self) -> &'static str {
		"direct_settlement"
	}

	fn supported_chains(&self) -> Vec<ChainId> {
		vec![
			1,     // Ethereum Mainnet
			10,    // Optimism
			56,    // BSC
			137,   // Polygon
			250,   // Fantom
			8453,  // Base
			31337, // Hardhat
			31338, // Hardhat 2
			42161, // Arbitrum One
			43114, // Avalanche
		]
	}

	fn supported_settlement_types(&self) -> Vec<SettlementType> {
		vec![SettlementType::Direct]
	}
}

impl OrderProcessorFactory for Eip7683OrderProcessorFactory {
	fn create_processor(&self, config: PluginConfig) -> PluginResult<Arc<dyn OrderProcessor>> {
		// Create plugin with config
		let plugin = Eip7683OrderPlugin::with_config(config)?;
		let processor = create_eip7683_processor(Arc::new(plugin));
		Ok(processor)
	}

	fn plugin_type(&self) -> &'static str {
		"eip7683_order"
	}

	fn source_types(&self) -> Vec<String> {
		vec!["eip7683_onchain".to_string(), "eip7683".to_string()]
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_plugin_factory_registration() {
		let factory = create_builtin_plugin_factory();
		let available = factory.list_available_plugins();

		// Check that built-in plugins are registered
		assert!(available.state_plugins.contains(&"memory".to_string()));
		assert!(available.state_plugins.contains(&"file".to_string()));

		// Check plugin availability
		assert!(factory.has_state_plugin("memory"));
		assert!(factory.has_state_plugin("file"));
		assert!(!factory.has_state_plugin("nonexistent"));
	}

	#[test]
	fn test_plugin_features() {
		let factory = create_builtin_plugin_factory();

		// Test memory plugin features
		let memory_features = factory.get_state_plugin_features("memory");
		assert!(memory_features.is_some());
		let features = memory_features.unwrap();
		assert!(features.contains(&StateFeature::TTL));
		assert!(features.contains(&StateFeature::AtomicOperations));

		// Test file plugin features
		let file_features = factory.get_state_plugin_features("file");
		assert!(file_features.is_some());
		let features = file_features.unwrap();
		assert!(features.contains(&StateFeature::TTL));
		assert!(features.contains(&StateFeature::Backup));
		assert!(features.contains(&StateFeature::AtomicOperations));
	}

	#[test]
	fn test_plugin_creation() {
		let factory = create_builtin_plugin_factory();

		// Test memory plugin creation
		let config = PluginConfig::default();
		let memory_plugin = factory.create_state_plugin("memory", config);
		assert!(memory_plugin.is_ok());

		// Test file plugin creation
		let config = PluginConfig::default();
		let file_plugin = factory.create_state_plugin("file", config);
		assert!(file_plugin.is_ok());

		// Test nonexistent plugin
		let config = PluginConfig::default();
		let nonexistent_plugin = factory.create_state_plugin("nonexistent", config);
		assert!(nonexistent_plugin.is_err());
	}

	#[test]
	fn test_global_factory() {
		let factory = global_plugin_factory();

		// Should have built-in plugins
		assert!(factory.has_state_plugin("memory"));
		assert!(factory.has_state_plugin("file"));

		// Should be able to create plugins
		let config = PluginConfig::default();
		let plugin = factory.create_state_plugin("memory", config);
		assert!(plugin.is_ok());
	}
}
