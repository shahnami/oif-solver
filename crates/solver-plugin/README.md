# Solver Plugin - Extensible Plugin System

The `solver-plugin` crate provides concrete implementations of all plugin interfaces defined in `solver-types`. It includes a centralized factory system for plugin creation and management, along with built-in implementations for state storage, order discovery, transaction delivery, and settlement strategies.

## 🏗️ Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         PLUGIN FACTORY                                   │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                    Factory Registry                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │   State     │  │  Discovery   │  │     Delivery           │  │  │
│  │  │ Factories   │  │  Factories   │  │    Factories           │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  │  ┌─────────────┐  ┌──────────────┐                               │  │
│  │  │ Settlement  │  │    Order     │                               │  │
│  │  │ Factories   │  │  Processors  │                               │  │
│  │  └─────────────┘  └──────────────┘                               │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
┌───────▼────────┐       ┌──────────▼────────┐       ┌─────────▼────────┐
│ State Plugins  │       │ Discovery Plugins │       │ Delivery Plugins │
├────────────────┤       ├───────────────────┤       ├──────────────────┤
│ • Memory       │       │ • EIP-7683        │       │ • EVM/Ethers     │
│ • File         │       │   Onchain         │       │                  │
└────────────────┘       └───────────────────┘       └──────────────────┘
                                    │
                ┌───────────────────┴───────────────────┐
                │                                       │
     ┌──────────▼────────┐                  ┌──────────▼────────┐
     │ Settlement Plugins│                  │ Order Processors  │
     ├───────────────────┤                  ├───────────────────┤
     │ • Direct          │                  │ • EIP-7683        │
     │ • Arbitrum        │                  │                   │
     └───────────────────┘                  └───────────────────┘
```

## 📁 Module Structure

```
solver-plugin/
├── src/
│   ├── lib.rs              # Module exports
│   ├── factory.rs          # Plugin factory system
│   ├── state/              # State storage plugins
│   │   ├── mod.rs
│   │   ├── memory.rs       # In-memory state storage
│   │   └── file.rs         # File-based state storage
│   ├── discovery/          # Order discovery plugins
│   │   ├── mod.rs
│   │   ├── onchain/        # On-chain discovery
│   │   │   └── eip7683.rs  # EIP-7683 event monitoring
│   │   └── offchain/       # Off-chain discovery (placeholder)
│   ├── delivery/           # Transaction delivery plugins
│   │   ├── mod.rs
│   │   └── evm/            # EVM-based delivery
│   │       └── ethers.rs   # Ethers.rs implementation
│   ├── settlement/         # Settlement strategy plugins
│   │   ├── mod.rs
│   │   ├── direct.rs       # Direct settlement
│   │   └── arbitrum.rs     # Arbitrum-specific settlement
│   └── order/              # Order processing plugins
│       ├── mod.rs
│       ├── processor.rs    # Generic order processor
│       └── eip7683.rs      # EIP-7683 order plugin
├── Cargo.toml
└── README.md
```

## 🔑 Key Components

### 1. **Plugin Factory** (`factory.rs`)

The central registry for all plugin types with a global singleton pattern.

**Key Features:**

- Type-safe plugin creation
- Configuration validation
- Feature discovery
- Chain support queries
- Global factory singleton

**Factory Structure:**

```rust
pub struct PluginFactory {
    state_factories: HashMap<String, Box<dyn StatePluginFactory>>,
    discovery_factories: HashMap<String, Box<dyn DiscoveryPluginFactory>>,
    delivery_factories: HashMap<String, Box<dyn DeliveryPluginFactory>>,
    settlement_factories: HashMap<String, Box<dyn SettlementPluginFactory>>,
    order_processor_factories: HashMap<String, Box<dyn OrderProcessorFactory>>,
}
```

### 2. **State Plugins** (`state/`)

#### Memory Plugin

- In-memory key-value storage with TTL support
- Atomic operations
- Configurable max entries and default TTL
- Thread-safe with DashMap

#### File Plugin

- Persistent file-based storage
- MD5-based file naming for key distribution
- Directory size tracking
- Atomic write operations with sync-on-write option

### 3. **Discovery Plugins** (`discovery/`)

#### EIP-7683 Onchain Discovery

- Monitors blockchain for EIP-7683 events (Open, Finalised, OrderPurchased)
- Configurable polling intervals
- Multi-contract monitoring

### 4. **Delivery Plugins** (`delivery/`)

#### EVM Ethers Delivery

- Full EVM transaction management
- EIP-1559 support
- Nonce management
- Gas price optimization
- Transaction status monitoring
- Mempool tracking (optional)

### 5. **Settlement Plugins** (`settlement/`)

#### Direct Settlement

- Simple settlement strategy
- Profitability validation
- Fill data verification
- Transaction preparation

#### Arbitrum Broadcaster

- Arbitrum-specific settlement broadcasting
- Cross-chain message handling
- Custom gas optimization

### 6. **Order Processors** (`order/`)

#### EIP-7683 Order Processor

- Parses EIP-7683 order events
- Creates transaction requests for fills
- Handles settlement transaction creation
- Validates order data

## 🔌 Plugin Interfaces

Each plugin implements the base plugin trait plus its specific interface:

```rust
// Base plugin trait (all plugins)
pub trait BasePlugin: Send + Sync {
    async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()>;
    async fn shutdown(&mut self) -> PluginResult<()>;
    async fn health_check(&self) -> PluginResult<PluginHealth>;
    fn plugin_type(&self) -> &'static str;
    fn version(&self) -> &'static str;
}

// Specific plugin traits
pub trait StatePlugin: BasePlugin { ... }
pub trait DiscoveryPlugin: BasePlugin { ... }
pub trait DeliveryPlugin: BasePlugin { ... }
pub trait SettlementPlugin: BasePlugin { ... }
pub trait OrderPlugin: BasePlugin { ... }
```

## 🚀 Usage Example

```rust
use solver_plugin::factory::{global_plugin_factory, PluginFactory};
use solver_types::PluginConfig;

// Get the global factory (includes all built-in plugins)
let factory = global_plugin_factory();

// Create a state plugin
let mut config = PluginConfig::default();
config.set("max_entries", 10000);
let state_plugin = factory.create_state_plugin("memory", config)?;

// Create a discovery plugin
let mut config = PluginConfig::default();
config.set("chain_id", 1);
config.set("rpc_url", "https://eth-mainnet.g.alchemy.com/v2/KEY");
config.set("input_settler_addresses", vec!["0x..."]);
let discovery_plugin = factory.create_discovery_plugin("eip7683_onchain", config)?;

// Create a delivery plugin
let mut config = PluginConfig::default();
config.set("chain_id", 1);
config.set("rpc_url", "https://eth-mainnet.g.alchemy.com/v2/KEY");
config.set("private_key", "0x...");
let delivery_plugin = factory.create_delivery_plugin("evm_ethers", config)?;

// List available plugins
let available = factory.list_available_plugins();
println!("State plugins: {:?}", available.state_plugins);
println!("Discovery plugins: {:?}", available.discovery_plugins);

// Check plugin features
let features = factory.get_state_plugin_features("file").unwrap();
println!("File plugin features: {:?}", features);
```

## 🔍 Critical Observations

### Strengths:

1. **Centralized Factory**: Single point for all plugin creation
2. **Type Safety**: Compile-time verification of plugin types
3. **Configuration Validation**: Plugins validate their configs
4. **Feature Discovery**: Runtime querying of plugin capabilities
5. **Modular Design**: Easy to add new plugin types

### Areas of Concern:

1. **Global Singleton**: The global factory pattern may complicate testing
2. **Box Allocations**: Heavy use of trait objects impacts performance
3. **String-based Registry**: Plugin names are strings, not enums
4. **Limited Error Context**: Factory errors lose plugin-specific context
5. **No Plugin Versioning**: Despite version() method, no version checking

### Potential Optimizations:

1. **Plugin Caching**: Reuse plugin instances where possible
2. **Lazy Loading**: Load plugins only when needed
3. **Configuration Schema**: Add JSON schema validation
4. **Plugin Dependencies**: Support inter-plugin dependencies
5. **Hot Reload**: Support updating plugins at runtime

## 🔗 Dependencies

### Internal Crates:

- `solver-types`: Core type definitions and plugin traits

### External Dependencies:

- `tokio`: Async runtime
- `async-trait`: Async trait support
- `ethers`: Ethereum interaction
- `alloy`: Ethereum types
- `serde`/`serde_json`: Serialization
- `tracing`: Logging
- `reqwest`: HTTP client
- `thiserror`/`anyhow`: Error handling
- `hex`: Hex encoding
- `backoff`: Retry logic
- `priority-queue`: Priority queue implementation
- `dashmap`: Concurrent hashmap
- `bytes`: Byte buffers
- `rand`: Random number generation
- `libc`: System calls
- `md5`: Hash generation for file names

## 🏃 Runtime Behavior

### Plugin Lifecycle:

1. **Factory Creation**: Global factory initialized on first use
2. **Plugin Registration**: Built-in plugins auto-registered
3. **Configuration**: User provides plugin config
4. **Instantiation**: Factory creates plugin instance
5. **Initialization**: Plugin initializes with config
6. **Operation**: Plugin performs its function
7. **Shutdown**: Clean shutdown when done

### Memory Management:

- Plugins are heap-allocated (Box/Arc)
- State plugins use Arc for shared access
- Order processors use Arc for multi-service use
- Discovery/Delivery/Settlement use Box for single ownership

## 🐛 Known Issues & Cruft

1. **Unused Offchain Module**: `discovery/offchain/mod.rs` exists but empty
2. **Duplicate Factory Structs**: Settlement plugins define unused factory structs
3. **Inconsistent Defaults**: Some configs have defaults, others don't
4. **Test Coverage**: Limited test coverage for most plugins
5. **Error Propagation**: Many `unwrap()` calls in plugin implementations
6. **Config Duplication**: Similar config fields across plugins

## 🔮 Future Improvements

1. **Dynamic Plugin Loading**: Load plugins from external files
2. **Plugin Marketplace**: Registry of community plugins
3. **Configuration Schema**: Formal schema for each plugin type
4. **Plugin Composition**: Combine multiple plugins
5. **Metrics Integration**: Built-in Prometheus metrics
6. **WASM Support**: Run plugins in WASM sandbox
7. **Plugin Templates**: Generator for new plugin types

## 📊 Performance Considerations

- **Factory Lookups**: HashMap lookups for each plugin creation
- **Box Allocations**: Every plugin creation allocates
- **Config Parsing**: Runtime config validation overhead
- **No Plugin Pooling**: Plugins created fresh each time

## ⚠️ Security Considerations

- **Private Key Handling**: Delivery plugins handle private keys
- **RPC Trust**: Plugins trust RPC endpoints
- **No Sandboxing**: Plugins run in same process
- **Config Validation**: Limited validation of config values

The `solver-plugin` crate provides a comprehensive plugin system with built-in implementations for all major solver functionality, though the global singleton pattern and extensive use of trait objects may impact testing and performance.
