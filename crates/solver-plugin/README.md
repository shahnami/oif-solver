# solver-plugin

## Overview

The `solver-plugin` crate consolidates all plugin implementations in a single, well-organized package. It provides implementations for order types, discovery sources, delivery methods, settlement strategies, and state backends, making it easy to extend the solver's functionality through Cargo features.

## Architecture

### Organization

```
solver-plugin/
├── src/
│   ├── lib.rs           # Plugin exports and registration
│   ├── orders/          # Order type plugins
│   │   ├── mod.rs
│   │   ├── eip7683.rs
│   │   ├── uniswapx.rs
│   │   └── cowswap.rs
│   ├── discovery/       # Discovery source plugins
│   │   ├── mod.rs
│   │   ├── chain.rs
│   │   ├── webhook.rs
│   │   └── mempool.rs
│   ├── delivery/        # Delivery method plugins
│   │   ├── mod.rs
│   │   ├── rpc.rs
│   │   ├── relayer.rs
│   │   └── bundler.rs
│   ├── settlement/      # Settlement strategy plugins
│   │   ├── mod.rs
│   │   ├── direct.rs
│   │   ├── arbitrum.rs
│   │   └── optimistic.rs
│   └── state/           # State backend plugins
│       ├── mod.rs
│       ├── redis.rs
│       ├── dynamodb.rs
│       └── postgres.rs
```

### Design Principles

- **Single Dependency**: Applications only need `solver-plugin`
- **Feature Flags**: Enable only needed plugins
- **Shared Utilities**: Common code reused across plugins
- **Consistent Versioning**: All plugins version together
- **Type Safety**: Compile-time plugin verification

## Plugin Categories

### 1. Order Type Plugins

Implement different order standards:

```rust
pub struct Eip7683OrderPlugin {
    config: Eip7683Config,
}

impl OrderTypePlugin for Eip7683OrderPlugin {
    fn name(&self) -> &'static str {
        "eip7683"
    }

    fn parse_order(&self, data: &[u8]) -> Result<Box<dyn Order>> {
        let order = Eip7683Order::decode(data)?;
        Ok(Box::new(order))
    }

    fn validate_order(&self, data: &[u8]) -> Result<()> {
        let order = Eip7683Order::decode(data)?;
        
        // Standard-specific validation
        if order.expires_at() < Timestamp::now() {
            return Err(Error::OrderExpired);
        }
        
        order.validate_signature()?;
        Ok(())
    }
}
```

### 2. Discovery Source Plugins

Monitor different event sources:

```rust
pub struct ChainDiscoveryPlugin {
    chain_id: ChainId,
    provider: Arc<Provider<Http>>,
    contracts: Vec<Address>,
    poll_interval: Duration,
}

#[async_trait]
impl DiscoverySourcePlugin for ChainDiscoveryPlugin {
    async fn start_monitoring(&self, sink: EventSink) -> Result<()> {
        // Monitor blockchain for events
    }
}
```

### 3. Delivery Method Plugins

Submit transactions through various mechanisms:

```rust
pub struct RpcDeliveryPlugin {
    chain_id: ChainId,
    provider: Arc<Provider<Http>>,
    signer: LocalWallet,
}

#[async_trait]
impl DeliveryMethodPlugin for RpcDeliveryPlugin {
    fn chain_id(&self) -> ChainId {
        self.chain_id
    }

    async fn deliver(&self, request: DeliveryRequest) -> Result<DeliveryReceipt> {
        // Submit via RPC
    }
}
```

### 4. Settlement Strategy Plugins

Handle order settlement and claiming:

```rust
pub struct DirectSettlementPlugin {
    oracle_address: Address,
    settler_addresses: HashMap<ChainId, Address>,
}

#[async_trait]
impl SettlementStrategyPlugin for DirectSettlementPlugin {
    async fn prepare_settlement(&self, fill: &FillData) -> Result<SettlementTx> {
        // Prepare settlement transaction
    }
}
```

### 5. State Backend Plugins

Provide different storage backends:

```rust
pub struct RedisStatePlugin {
    connection_pool: ConnectionPool,
}

#[async_trait]
impl StateBackendPlugin for RedisStatePlugin {
    async fn create_store(&self, config: Value) -> Result<Box<dyn StateStore>> {
        Ok(Box::new(RedisStateStore::new(self.connection_pool.clone())))
    }
}
```

## Usage

### Cargo Features

```toml
[dependencies]
solver-plugin = { version = "0.1", features = ["eip7683", "rpc-delivery", "redis-state"] }

# Available features:
# Order types: "eip7683", "uniswapx", "cowswap"
# Discovery: "chain-discovery", "webhook-discovery", "mempool-discovery"
# Delivery: "rpc-delivery", "relayer-delivery", "bundler-delivery"
# Settlement: "direct-settlement", "arbitrum-settlement", "optimistic-settlement"
# State: "redis-state", "dynamodb-state", "postgres-state"
```

### Plugin Registration

```rust
use solver_plugin::{PluginRegistry, register_defaults};

// Create registry with all enabled plugins
let mut registry = PluginRegistry::new();
register_defaults(&mut registry)?;

// Create specific plugin
let order_plugin = registry.create_order_type("eip7683", config)?;
```

### Configuration

```toml
# Order type configuration
[plugins.orders.eip7683]
enabled = true
validation_level = "strict"

# Discovery configuration
[plugins.discovery.chain]
enabled = true
chain_id = 1
contracts = ["0x..."]

# Delivery configuration
[plugins.delivery.rpc]
enabled = true
endpoints = { 1 = "https://eth.rpc" }
private_key = "${RPC_PRIVATE_KEY}"

# Settlement configuration
[plugins.settlement.direct]
enabled = true
oracle_address = "0x..."

# State configuration
[plugins.state.redis]
enabled = true
url = "redis://localhost:6379"
```

## Pros

1. **Centralized Management**: All plugins in one place
2. **Feature Control**: Fine-grained plugin selection
3. **Code Reuse**: Shared utilities across plugins
4. **Easy Discovery**: Clear organization structure
5. **Version Consistency**: Unified versioning

## Cons

1. **Compilation Time**: Large crate with many features
2. **Binary Size**: Unused code included without LTO
3. **Coupling**: Changes affect all plugins
4. **Testing**: Need to test all combinations

## Implementation Details

### Plugin Factory Pattern

```rust
pub type PluginFactory<T> = fn(config: Value) -> Result<Box<T>>;

pub struct PluginRegistry {
    order_types: HashMap<&'static str, PluginFactory<dyn OrderTypePlugin>>,
    discovery_sources: HashMap<&'static str, PluginFactory<dyn DiscoverySourcePlugin>>,
    // ... other categories
}

impl PluginRegistry {
    pub fn register_order_type(&mut self, name: &'static str, factory: PluginFactory<dyn OrderTypePlugin>) {
        self.order_types.insert(name, factory);
    }

    pub fn create_order_type(&self, name: &str, config: Value) -> Result<Box<dyn OrderTypePlugin>> {
        let factory = self.order_types.get(name)
            .ok_or_else(|| Error::PluginNotFound(name.to_string()))?;
        factory(config)
    }
}
```

### Feature Flag Organization

```toml
[features]
default = ["eip7683", "chain-discovery", "rpc-delivery", "direct-settlement"]

# Order types
eip7683 = []
uniswapx = ["dep:uniswapx-rs"]
cowswap = ["dep:cowswap-rs"]

# Discovery sources
chain-discovery = ["dep:ethers"]
webhook-discovery = ["dep:axum"]
mempool-discovery = ["dep:ethers-flashbots"]

# Delivery methods
rpc-delivery = ["dep:ethers"]
relayer-delivery = ["dep:reqwest"]
bundler-delivery = ["dep:ethers-flashbots"]

# Settlement strategies
direct-settlement = []
arbitrum-settlement = ["dep:arbitrum-sdk"]
optimistic-settlement = []

# State backends
redis-state = ["dep:redis", "dep:bb8-redis"]
dynamodb-state = ["dep:aws-sdk-dynamodb"]
postgres-state = ["dep:sqlx"]
```

### Shared Utilities

```rust
// Common chain configuration
#[derive(Clone, Debug, Deserialize)]
pub struct ChainConfig {
    pub chain_id: ChainId,
    pub rpc_url: String,
    pub private_key: Option<String>,
    pub max_retries: u32,
    pub timeout_ms: u64,
}

// Shared provider creation
pub async fn create_provider(config: &ChainConfig) -> Result<Provider<Http>> {
    Provider::<Http>::try_from(&config.rpc_url)?
        .with_retry(config.max_retries)
        .with_timeout(Duration::from_millis(config.timeout_ms))
}

// Common error types
#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Provider error: {0}")]
    Provider(String),
    
    #[error("Plugin not found: {0}")]
    NotFound(String),
}
```

### Testing Strategy

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_eip7683_plugin() {
        let config = json!({
            "validation_level": "strict"
        });
        
        let plugin = Eip7683OrderPlugin::new(config).unwrap();
        let order_data = include_bytes!("../test_data/eip7683_order.bin");
        
        let order = plugin.parse_order(order_data).unwrap();
        assert_eq!(plugin.name(), "eip7683");
    }

    #[cfg(all(feature = "eip7683", feature = "chain-discovery"))]
    #[tokio::test]
    async fn test_plugin_integration() {
        // Test plugin combinations
    }
}
```

## Extending Plugins

### Adding a New Order Type

1. Create `src/orders/myorder.rs`:

```rust
pub struct MyOrderPlugin {
    config: MyOrderConfig,
}

impl OrderTypePlugin for MyOrderPlugin {
    fn name(&self) -> &'static str {
        "myorder"
    }
    // ... implement trait
}
```

2. Add to `src/orders/mod.rs`:

```rust
#[cfg(feature = "myorder")]
pub mod myorder;
```

3. Register in `src/lib.rs`:

```rust
#[cfg(feature = "myorder")]
registry.register_order_type("myorder", |config| {
    Ok(Box::new(myorder::MyOrderPlugin::new(config)?))
});
```

4. Add feature to `Cargo.toml`:

```toml
[features]
myorder = ["dep:myorder-sdk"]
```

## Metrics

Each plugin category exposes specific metrics:

- Order types: `plugin_orders_parsed_total`, `plugin_orders_validated_total`
- Discovery: `plugin_discovery_events_total`, `plugin_discovery_errors_total`
- Delivery: `plugin_delivery_attempts_total`, `plugin_delivery_success_rate`
- Settlement: `plugin_settlements_prepared_total`, `plugin_settlements_completed_total`
- State: `plugin_state_operations_total`, `plugin_state_latency_seconds`

## Future Enhancements

1. **Dynamic Loading**: Runtime plugin loading via WASM/dylib
2. **Plugin Marketplace**: Registry of community plugins
3. **Hot Reload**: Update plugins without restart
4. **Plugin Composition**: Combine multiple plugins
5. **Cross-Plugin Communication**: Shared event bus
