# OIF Solver Technical Architecture

## Executive Summary

This document outlines the refactored architecture for the OIF (Open Intent Framework) Solver, focusing on a simplified, plugin-based approach where services own their plugins directly, strategies are configuration-driven, and inter-module communication is handled through a coordinated event system.

## Core Design Principles

### 1. Service Autonomy
- Each service owns and manages its own plugins
- No centralized plugin manager
- Services are responsible for their plugin lifecycle

### 2. Configuration-Driven Strategies
- Strategies are defined in configuration files
- Each service reads its strategy configuration directly
- No centralized strategy manager

### 3. Event-Driven Communication
- Single event sink coordinated by solver-core
- Asynchronous, non-blocking communication
- Clear event contracts between services

### 4. Simplicity and Clarity
- Minimal abstraction layers
- Direct service-to-service communication where appropriate
- Clear ownership and responsibilities

## Service Architecture

### Plugin Ownership Model

Each service follows this pattern for plugin management:

```rust
pub struct ServiceName {
    // Service-specific fields
    plugins: Vec<Box<dyn PluginType>>,
    config: ServiceConfig,
}

impl ServiceName {
    pub async fn new(config: ServiceConfig) -> Result<Self> {
        // Initialize plugins based on config
        let plugins = Self::initialize_plugins(&config.plugins)?;
        
        Ok(Self {
            plugins,
            config,
        })
    }
    
    fn initialize_plugins(configs: &[PluginInstanceConfig]) -> Result<Vec<Box<dyn PluginType>>> {
        // Each service knows how to create its own plugins
    }
}
```

### Strategy Configuration Model

Strategies are now configuration values, not runtime objects:

```toml
[discovery_strategy]
type = "standard"
config = { 
    parallel_sources = true,
    max_concurrent = 10,
    retry_failed = true 
}

[delivery_strategy]
type = "fastest"
config = {
    timeout_ms = 5000,
    fallback_strategy = "cheapest"
}

[settlement_strategy]
type = "direct"
config = {
    confirmation_blocks = 2,
    gas_price_multiplier = 1.2
}
```

## Service Descriptions

### solver-core (Orchestration Engine)

**Responsibilities:**
- Service lifecycle management
- Event sink coordination
- Configuration distribution
- Health monitoring

**Key Components:**
```rust
pub struct Orchestrator {
    config: Arc<RwLock<SolverConfig>>,
    
    // Core services
    discovery_service: Arc<DiscoveryService>,
    delivery_service: Arc<DeliveryService>,
    state_service: Arc<StateService>,
    settlement_service: Arc<SettlementService>,
    
    // Event coordination
    event_sink: Arc<EventSink>,
    event_processor: Arc<EventProcessor>,
    
    // Lifecycle
    lifecycle_manager: Arc<LifecycleManager>,
}
```

### solver-discovery (Event Discovery)

**Responsibilities:**
- Monitor multiple chains for orders
- Parse and validate discovered events
- Send events to the central event sink

**Plugin Types:**
- Onchain monitoring (EIP-7683, custom contracts)
- Offchain sources (APIs, webhooks)
- Real-time streams (WebSocket, SSE)

**Strategy Application:**
```rust
impl DiscoveryService {
    async fn process_with_strategy(&self, source: &str) -> Result<()> {
        match self.config.strategy.strategy_type.as_str() {
            "aggressive" => self.aggressive_discovery(source).await,
            "conservative" => self.conservative_discovery(source).await,
            "standard" => self.standard_discovery(source).await,
            _ => Err(Error::UnknownStrategy)
        }
    }
}
```

### solver-delivery (Order Execution)

**Responsibilities:**
- Execute orders on target chains
- Manage transaction lifecycle
- Handle retries and failures

**Plugin Types:**
- EVM delivery (Ethers-based)
- Cosmos delivery
- Custom protocol delivery

**Strategy Application:**
```rust
impl DeliveryService {
    async fn select_delivery_method(&self, order: &Order) -> Result<DeliveryMethod> {
        match self.config.strategy.strategy_type.as_str() {
            "fastest" => self.select_fastest_method(order).await,
            "cheapest" => self.select_cheapest_method(order).await,
            "redundant" => self.select_redundant_methods(order).await,
            _ => self.select_default_method(order).await
        }
    }
}
```

### solver-state (Persistence Layer)

**Responsibilities:**
- Store and track orders
- Track fills and settlements
- Provide query capabilities
- Manage order lifecycle state

**Plugin Types:**
- In-memory storage
- File-based storage
- Database storage (future)

**Key Interfaces:**
```rust
pub trait StateStore: Send + Sync {
    async fn save_order(&self, order: OrderState) -> Result<()>;
    async fn update_order_status(&self, id: &str, status: OrderStatus) -> Result<()>;
    async fn get_order(&self, id: &str) -> Result<Option<OrderState>>;
    async fn query_orders(&self, filter: OrderFilter) -> Result<Vec<OrderState>>;
    async fn save_fill(&self, fill: FillState) -> Result<()>;
    async fn save_settlement(&self, settlement: SettlementState) -> Result<()>;
}
```

### solver-settlement (Blockchain Settlement)

**Responsibilities:**
- Encode settlement transactions
- Broadcast to destination chains
- Monitor settlement status

**Plugin Types:**
- Direct settlement
- Arbitrum broadcaster
- Optimistic rollup settlers

## Inter-Service Communication

### Event Sink Architecture

A single, coordinated event sink managed by solver-core:

```rust
// In solver-core
pub struct EventProcessor {
    // Single event channel for all services
    event_tx: mpsc::UnboundedSender<Event>,
    event_rx: mpsc::UnboundedReceiver<Event>,
    
    // Event routing
    handlers: HashMap<EventType, Vec<Box<dyn EventHandler>>>,
}

pub enum Event {
    Discovery(DiscoveryEvent),
    OrderCreated(OrderEvent),
    OrderFilled(FillEvent),
    SettlementComplete(SettlementEvent),
    ServiceStatus(StatusEvent),
}
```

### Event Flow

1. **Discovery Phase:**
   ```
   Chain Monitor -> Discovery Plugin -> Discovery Service -> Event Sink
   ```

2. **Processing Phase:**
   ```
   Event Sink -> Event Processor -> Order Classification -> State Service
   ```

3. **Execution Phase:**
   ```
   State Service -> Delivery Service -> Delivery Plugin -> Target Chain
   ```

4. **Settlement Phase:**
   ```
   Delivery Result -> Settlement Service -> Settlement Plugin -> Event Sink
   ```

### Service Communication Patterns

**Direct Communication:**
- Used for synchronous operations
- Service references passed during initialization
- Example: Delivery service querying state service

**Event-Based Communication:**
- Used for asynchronous notifications
- All events flow through central event sink
- Example: Discovery notifying about new orders

## Configuration Flow

### 1. Initial Load
```rust
// In main.rs
let config = ConfigLoader::load_from_file(&config_path)?;
let solver_config = Arc::new(RwLock::new(config));
```

### 2. Service Configuration Distribution
```rust
// Each service gets its relevant config section
let discovery_config = DiscoveryConfig {
    plugins: config.plugins.discovery.clone(),
    strategy: config.discovery_strategy.clone(),
};

let delivery_config = DeliveryConfig {
    plugins: config.plugins.delivery.clone(),
    strategy: config.delivery_strategy.clone(),
};
```

### 3. Plugin Configuration
```rust
// Each service initializes its plugins with config
for plugin_config in &service_config.plugins {
    let plugin = factory.create_plugin(plugin_config)?;
    plugins.push(plugin);
}
```

## Implementation Roadmap

### Phase 1: Core Refactoring
1. Remove StrategyManager from solver-core
2. Remove centralized plugin management
3. Implement service-owned plugin initialization

### Phase 2: Event System
1. Design unified event types
2. Implement central event sink in solver-core
3. Refactor services to use event sink

### Phase 3: Configuration Updates
1. Update configuration structures
2. Add strategy configuration to each service section
3. Implement configuration validation

### Phase 4: Service Updates
1. Update each service to own its plugins
2. Implement strategy-based execution
3. Add event sink integration

### Phase 5: Testing and Documentation
1. Unit tests for each service
2. Integration tests for event flow
3. Update API documentation
4. Create migration guide

## Benefits of This Architecture

### 1. Simplified Mental Model
- Clear ownership boundaries
- Direct service responsibilities
- Reduced abstraction layers

### 2. Better Performance
- Less indirection
- Direct plugin calls
- Efficient event routing

### 3. Easier Debugging
- Clear event flow
- Service-isolated issues
- Better error propagation

### 4. Flexible Evolution
- Services can evolve independently
- Easy to add new plugin types
- Strategy changes through configuration

## Future Considerations

### 1. Scalability
- Event sink can be replaced with message queue
- Services can be distributed
- Horizontal scaling per service type

### 2. Monitoring
- Centralized metrics collection
- Event flow visualization
- Performance profiling hooks

### 3. External Integrations
- Webhook support for events
- REST API for order submission
- gRPC for high-performance communication