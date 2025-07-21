# Solver Delivery - Transaction Submission Service

The `solver-delivery` crate is responsible for orchestrating transaction submission across multiple blockchain networks through a plugin-based architecture. It manages both order filling and settlement transactions with configurable delivery strategies.

## 🏗️ Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         DELIVERY SERVICE                                 │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Core Components                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  Plugin     │  │   Delivery   │  │      Delivery          │  │  │
│  │  │  Registry   │  │   Tracker    │  │      Strategy          │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                    Plugin Collections                              │  │
│  │  ┌─────────────────────┐      ┌─────────────────────────────┐   │  │
│  │  │  Delivery Plugins    │      │    Order Processors         │   │  │
│  │  │  ┌───────────────┐  │      │  ┌───────────────────────┐  │   │  │
│  │  │  │ RPC Plugin    │  │      │  │ EIP-7683 Processor    │  │   │  │
│  │  │  ├───────────────┤  │      │  ├───────────────────────┤  │   │  │
│  │  │  │ Relayer Plugin│  │      │  │ Custom Processor      │  │   │  │
│  │  │  ├───────────────┤  │      │  └───────────────────────┘  │   │  │
│  │  │  │ Bundler Plugin│  │      └─────────────────────────────┘   │  │
│  │  │  └───────────────┘  │                                         │  │
│  │  └─────────────────────┘                                         │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                            ┌───────┴────────┐
                            │                │
                    ┌───────▼────┐   ┌───────▼────────┐
                    │   Order     │   │     Fill       │
                    │   Events    │   │    Events      │
                    └────────────┘   └────────────────┘
```

## 📁 Module Structure

```
solver-delivery/
├── src/
│   └── lib.rs          # Service implementation and plugin orchestration
├── Cargo.toml          # Dependencies
└── README.md           # This file
```

## 🔑 Key Components

### 1. **DeliveryService** (`lib.rs`)
The main service that orchestrates transaction delivery through plugins.

**Key Responsibilities:**
- Plugin registration and lifecycle management
- Transaction routing to appropriate plugins
- Order event processing into transaction requests
- Delivery tracking and status monitoring
- Strategy-based plugin selection

**Internal Structure:**
```rust
pub struct DeliveryService {
    // Plugin registries (thread-safe)
    delivery_plugins: Arc<RwLock<HashMap<String, Arc<dyn DeliveryPlugin>>>>,
    order_processors: Arc<RwLock<HashMap<String, Arc<dyn OrderProcessor>>>>,
    
    // Active delivery tracking
    active_deliveries: Arc<RwLock<HashMap<String, DeliveryTracker>>>,
    
    // Configuration
    config: DeliveryConfig,
}
```

### 2. **DeliveryTracker**
Tracks the lifecycle of each delivery attempt:
```rust
pub struct DeliveryTracker {
    pub request: DeliveryRequest,
    pub attempts: Vec<DeliveryAttempt>,
    pub started_at: u64,
    pub status: DeliveryTrackingStatus,
}
```

### 3. **Plugin Management**
- **Delivery Plugins**: Handle actual transaction submission (RPC, relayers, bundlers)
- **Order Processors**: Convert order/fill events into transaction requests

## 🔄 Transaction Flow

```text
OrderEvent → OrderProcessor → TransactionRequest → DeliveryService
                                                         │
                                                         ▼
                                                  Plugin Selection
                                                         │
                                                         ▼
                                                  DeliveryPlugin
                                                         │
                                                         ▼
                                                  DeliveryResponse
```

### Flow Steps:
1. **Order Processing**: OrderEvent received from discovery
2. **Transaction Creation**: OrderProcessor creates TransactionRequest
3. **Plugin Selection**: Service selects suitable plugins based on chain
4. **Delivery Execution**: Plugin submits transaction to blockchain
5. **Status Tracking**: Service monitors transaction status
6. **Settlement**: FillEvent triggers settlement transaction

## 🔌 Plugin System

### DeliveryPlugin Interface:
```rust
#[async_trait]
pub trait DeliveryPlugin: BasePlugin {
    fn chain_id(&self) -> ChainId;
    async fn can_deliver(&self, request: &DeliveryRequest) -> PluginResult<bool>;
    async fn estimate(&self, request: &DeliveryRequest) -> PluginResult<DeliveryEstimate>;
    async fn deliver(&self, request: DeliveryRequest) -> PluginResult<DeliveryResponse>;
    async fn get_transaction_status(&self, tx_hash: &TxHash) -> PluginResult<Option<DeliveryResponse>>;
    async fn cancel_transaction(&self, tx_hash: &TxHash) -> PluginResult<bool>;
    async fn replace_transaction(&self, original_tx_hash: &TxHash, new_request: DeliveryRequest) -> PluginResult<DeliveryResponse>;
    fn supported_features(&self) -> Vec<DeliveryFeature>;
    async fn get_network_status(&self) -> PluginResult<NetworkStatus>;
}
```

### OrderProcessor Interface:
```rust
#[async_trait]
pub trait OrderProcessor: Send + Sync {
    async fn process_order_event(&self, event: &OrderEvent) -> PluginResult<Option<TransactionRequest>>;
    async fn process_fill_event(&self, event: &FillEvent) -> PluginResult<Option<TransactionRequest>>;
    fn can_handle_source(&self, source: &str) -> bool;
}
```

## 🚀 Usage Example

```rust
use solver_delivery::{DeliveryService, DeliveryServiceBuilder};
use solver_types::configs::DeliveryConfig;
use solver_types::plugins::DeliveryStrategy;

// Build service with plugins
let service = DeliveryServiceBuilder::new()
    .with_config(DeliveryConfig {
        strategy: DeliveryStrategy::RoundRobin,
        fallback_enabled: true,
        max_parallel_attempts: 3,
    })
    .with_plugin("rpc".to_string(), Box::new(rpc_plugin), rpc_config)
    .with_plugin("flashbots".to_string(), Box::new(flashbots_plugin), flashbots_config)
    .with_order_processor("eip7683".to_string(), Arc::new(eip7683_processor))
    .build()
    .await;

// Process an order event
let order_event = OrderEvent { /* ... */ };
if let Some(tx_request) = service.process_order_to_transaction(&order_event).await? {
    // Execute the transaction
    let response = service.execute_transaction(tx_request).await?;
    println!("Transaction submitted: {:?}", response.tx_hash);
}

// Check transaction status
let status = service.get_transaction_status(&tx_hash, chain_id).await?;

// Health check all plugins
let health_status = service.health_check().await?;
```

## 🔍 Critical Observations

### Strengths:
1. **Plugin Isolation**: Each plugin manages its own connections and state
2. **Type Safety**: Strong typing for requests, responses, and priorities
3. **Tracking**: Comprehensive delivery tracking with attempt history
4. **Flexibility**: Easy to add new delivery methods via plugins
5. **Separation of Concerns**: Order processing separated from delivery

### Areas of Concern:
1. **Single Strategy**: Only RoundRobin strategy is implemented (others mentioned in docs but missing)
2. **Error Recovery**: Limited retry logic within the service itself
3. **Plugin Discovery**: No automatic plugin discovery based on chain
4. **Metrics**: No built-in metrics collection despite README claims
5. **Parallel Attempts**: Config supports parallel attempts but not implemented

### Potential Optimizations:
1. **Strategy Implementation**: Add Fastest, Cheapest, and Redundant strategies
2. **Connection Pooling**: Share connections between plugins for same chain
3. **Circuit Breaker**: Add circuit breaker for failing plugins
4. **Batch Processing**: Support batching multiple transactions
5. **Priority Queue**: Implement priority-based transaction queuing

## 🔗 Dependencies

### Internal Crates:
- `solver-types`: Core type definitions and plugin traits

### External Dependencies:
- `tokio`: Async runtime
- `async-trait`: Async trait support
- `futures`: Async utilities
- `tracing`: Structured logging
- `uuid`: Unique identifier generation
- `bytes`: Byte manipulation
- `thiserror`/`anyhow`: Error handling

## 🏃 Runtime Behavior

### Service Lifecycle:
1. **Initialization**: Plugins are initialized during builder.build()
2. **Registration**: Successfully initialized plugins are registered
3. **Order Processing**: Orders converted to transactions via processors
4. **Delivery**: Transactions routed to appropriate plugins
5. **Monitoring**: Active deliveries tracked in memory

### Transaction Processing:
1. **Request Conversion**: TransactionRequest → DeliveryRequest
2. **Plugin Selection**: Find plugins that can handle the chain
3. **Strategy Execution**: Apply configured strategy (currently only RoundRobin)
4. **Attempt Recording**: Track each delivery attempt
5. **Status Updates**: Update tracking status on completion/failure

## 🐛 Known Issues & Cruft

1. **Incomplete Strategies**: Only RoundRobin is implemented despite multiple strategies in types
2. **Unused Config**: `max_parallel_attempts` and `fallback_enabled` are stored but never used
3. **Memory Leak Risk**: `active_deliveries` grows unbounded - no cleanup mechanism
4. **Missing Features**: No implementation for replace_transaction despite trait requirement
5. **Timestamp Generation**: Repeated timestamp code could be extracted
6. **Error Context**: Many errors lose context during propagation

## 🔮 Future Improvements

1. **Complete Strategy Implementation**: Implement all delivery strategies
2. **Delivery Cleanup**: Add TTL-based cleanup for old deliveries
3. **Plugin Hot-Reload**: Support adding/removing plugins at runtime
4. **Transaction Batching**: Batch multiple transactions for efficiency
5. **Enhanced Monitoring**: Add Prometheus metrics
6. **WebSocket Support**: Real-time transaction status updates
7. **Gas Oracle Integration**: Better gas price estimation

## 📊 Performance Considerations

- **Lock Contention**: Multiple RwLocks could cause contention under load
- **Plugin Iteration**: Linear search through plugins for each request
- **Memory Usage**: Unbounded delivery tracking map
- **No Caching**: Network status and estimates not cached

## ⚠️ Security Considerations

- **Plugin Trust**: All plugins have full access to transaction data
- **Key Management**: No built-in key management - relies on plugins
- **Transaction Privacy**: No MEV protection built into service layer

The `solver-delivery` service provides a flexible foundation for multi-chain transaction submission with room for enhancement in strategy implementation and operational features.