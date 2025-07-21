# solver-delivery

## Overview

The `solver-delivery` module is responsible for submitting transactions through various delivery mechanisms. It provides a pluggable architecture that supports multiple delivery methods (RPC, relayers, bundlers) with different strategies (fastest, cheapest, redundant).

## Architecture

### Core Components

1. **DeliveryManager** - Orchestrates transaction submission across multiple delivery methods
2. **DeliveryMethod Enum** - Type-safe wrapper around delivery plugins
3. **DeliveryRequest** - Standardized request format for all delivery methods
4. **DeliveryStrategy** - Configurable strategies for method selection

### Design Principles

- **Plugin-Owned Providers**: Each delivery plugin manages its own chain connections
- **Type Safety**: Enum-based approach reduces runtime overhead
- **Strategy Pattern**: Flexible delivery strategies without code changes
- **Isolation**: Plugin failures don't affect other plugins

## Structure

```rust
// Delivery method enumeration for type safety and performance
#[derive(Clone)]
pub enum DeliveryMethod {
    Rpc(RpcDeliveryPlugin),
    Relayer(RelayerDeliveryPlugin),
    Bundler(BundlerDeliveryPlugin),
    Custom(String, Arc<dyn DeliveryMethodPlugin>),
}

// Standardized delivery request
pub struct DeliveryRequest {
    pub transaction: Transaction,
    pub chain_id: ChainId,
    pub priority: DeliveryPriority,
    pub metadata: HashMap<String, Value>,
}

// Priority levels for transaction delivery
pub enum DeliveryPriority {
    Normal,
    Fast,
    Custom { max_fee: U256, priority_fee: U256 },
}

// Manager orchestrating delivery methods
pub struct DeliveryManager {
    plugins: HashMap<ChainId, Vec<DeliveryMethod>>,
    strategy: DeliveryStrategy,
}
```

## Abstractions

### DeliveryMethodPlugin Trait

```rust
#[async_trait]
pub trait DeliveryMethodPlugin: Send + Sync {
    /// Get the chain ID this plugin operates on
    fn chain_id(&self) -> ChainId;

    /// Check if this plugin can handle the delivery request
    fn can_deliver(&self, request: &DeliveryRequest) -> bool;

    /// Estimate delivery cost and time
    async fn estimate(&self, request: &DeliveryRequest) -> Result<DeliveryEstimate>;

    /// Execute delivery
    async fn deliver(&self, request: DeliveryRequest) -> Result<DeliveryReceipt>;
}
```

### Delivery Strategies

1. **FirstSuccess** - Try methods sequentially until one succeeds
2. **Redundant** - Submit through all available methods
3. **Cheapest** - Estimate costs and pick the most economical
4. **Fastest** - Select method with lowest latency

## Usage

### Basic Usage

```rust
// Create delivery manager with configured plugins
let mut plugins = HashMap::new();
plugins.insert(
    ChainId::from(1),
    vec![
        DeliveryMethod::Rpc(rpc_plugin),
        DeliveryMethod::Relayer(relayer_plugin),
    ]
);

let manager = DeliveryManager::new(plugins, DeliveryStrategy::FirstSuccess);

// Submit transaction
let request = DeliveryRequest {
    transaction: tx,
    chain_id: ChainId::from(1),
    priority: DeliveryPriority::Normal,
    metadata: HashMap::new(),
};

let response = manager.submit(request).await?;
```

### Configuration

```toml
[delivery]
methods = ["rpc", "flashbots"]
strategy = "fastest"

[delivery.rpc]
chain_id = 1
rpc_url = "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY"
private_key = "0x..."
max_retries = 3
timeout_ms = 30000

[delivery.flashbots]
chain_id = 1
relay_url = "https://relay.flashbots.net"
signer_key = "${FLASHBOTS_SIGNER_KEY}"
```

## Pros

1. **Flexibility**: Easy to add new delivery methods via plugins
2. **Performance**: Enum-based dispatch is faster than trait objects
3. **Resilience**: Multiple delivery methods provide fallback options
4. **Type Safety**: Compile-time guarantees for delivery methods
5. **Isolation**: Each plugin manages its own connections and state

## Cons

1. **Recompilation**: Adding new enum variants requires rebuilding
2. **Resource Usage**: Each plugin maintains separate connections
3. **Complexity**: Strategy selection adds cognitive overhead
4. **Configuration**: Each plugin needs chain-specific settings

## Implementation Details

### Provider Management

Each delivery plugin creates and manages its own provider:

```rust
impl RpcDeliveryPlugin {
    pub async fn new(config: ChainConfig) -> Result<Self> {
        // Plugin owns its provider with retry and timeout middleware
        let provider = Provider::<Http>::try_from(&config.rpc_url)?
            .with_retry(config.max_retries)
            .with_timeout(Duration::from_millis(config.timeout_ms));

        let wallet = config.private_key
            .ok_or_else(|| Error::Config("Delivery requires private_key".into()))?
            .parse::<LocalWallet>()?
            .with_chain_id(config.chain_id.as_u64());

        Ok(Self {
            chain_id: config.chain_id,
            provider: Arc::new(provider),
            signer: wallet,
        })
    }
}
```

### Error Handling

The module uses a hierarchical error system:

```rust
#[derive(Error, Debug)]
pub enum DeliveryError {
    #[error("No delivery method available for chain {0}")]
    NoMethodForChain(ChainId),
    
    #[error("All delivery methods failed")]
    AllMethodsFailed(Vec<String>),
    
    #[error("Transaction rejected: {0}")]
    TransactionRejected(String),
    
    #[error("Estimation failed: {0}")]
    EstimationFailed(String),
}
```

### Metrics

The module exposes metrics for monitoring:

- `delivery_requests_total` - Total delivery requests by method
- `delivery_success_rate` - Success rate per method
- `delivery_latency_seconds` - Time to submit transactions
- `delivery_cost_wei` - Gas costs by method

## Future Enhancements

1. **Dynamic Plugin Loading**: Load delivery plugins at runtime
2. **Advanced Strategies**: ML-based method selection
3. **Circuit Breakers**: Automatic failover for unreliable methods
4. **Gas Oracle Integration**: Better cost estimation
5. **MEV Protection**: Enhanced privacy features
