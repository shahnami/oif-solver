# Solver Core - Plugin-Based Orchestration Engine

The `solver-core` crate is the central orchestration engine for the OIF (Order Intent Format) solver system. It coordinates multiple services through a plugin-based architecture, managing the complete lifecycle of cross-chain order discovery, execution, and settlement.

## 🏗️ Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                            ORCHESTRATOR                                  │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Core Components                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  Lifecycle  │  │    Event     │  │   Service Manager      │  │  │
│  │  │  Manager    │  │  Processor   │  │  (Start/Stop/Health)   │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                         Services                                   │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │ Discovery   │  │   Delivery   │  │    Settlement          │  │  │
│  │  │ Service     │  │   Service    │  │    Service             │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  │                    ┌──────────────┐                               │  │
│  │                    │    State     │                               │  │
│  │                    │   Service    │                               │  │
│  │                    └──────────────┘                               │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                          Plugin Factory                                  │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────────┐     │
│  │   State     │  │  Discovery   │  │    Delivery/Settlement     │     │
│  │  Plugins    │  │   Plugins    │  │       Plugins              │     │
│  └─────────────┘  └──────────────┘  └────────────────────────────┘     │
└─────────────────────────────────────────────────────────────────────────┘
```

## 📁 Module Structure

```
solver-core/
├── src/
│   ├── lib.rs          # Public API exports
│   ├── engine.rs       # Orchestrator implementation
│   ├── error.rs        # Error types
│   └── lifecycle.rs    # Lifecycle state management
├── Cargo.toml          # Dependencies
└── README.md           # This file
```

## 🔑 Key Components

### 1. **Orchestrator** (`engine.rs`)

The main coordination component that manages all services and their lifecycle.

**Key Responsibilities:**

- Service initialization and management
- Event routing and processing
- Health monitoring
- Fill status tracking
- Graceful shutdown coordination

**Internal Structure:**

```rust
pub struct Orchestrator {
    // Configuration
    config: Arc<RwLock<SolverConfig>>,

    // Core services
    discovery_service: Arc<DiscoveryService>,
    delivery_service: Arc<DeliveryService>,
    settlement_service: Arc<SettlementService>,
    state_service: Arc<StateService>,

    // Event coordination
    event_tx: EventSender,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<Event>>>,

    // Lifecycle & monitoring
    lifecycle_manager: Arc<LifecycleManager>,
    shutdown_tx: broadcast::Sender<()>,
    tasks: Arc<Mutex<JoinSet<Result<(), CoreError>>>>,
    pending_fills: Arc<RwLock<HashMap<String, FillEvent>>>,
}
```

### 2. **Lifecycle Manager** (`lifecycle.rs`)

Manages the operational state of the orchestrator.

**States:**

- `Initializing`: Setting up services
- `Running`: Active and processing events
- `Stopping`: Graceful shutdown in progress
- `Stopped`: Fully shut down
- `Error`: Fatal error occurred

### 3. **Error Handling** (`error.rs`)

Comprehensive error types for all failure scenarios:

- `Configuration`: Invalid configuration
- `ServiceInit`: Service startup failures
- `EventProcessing`: Event handling errors
- `Lifecycle`: State transition errors
- `State/Discovery/Delivery`: Service-specific errors
- `Plugin`: Plugin operation failures
- `Channel`: Communication errors
- `Serialization`: Data format errors
- `Shutdown`: Cleanup failures

## 🔄 Event Flow

```
Discovery Plugin → OrderCreated Event → Orchestrator
                                           │
                                           ▼
                                    Process Order
                                           │
                                           ▼
                                    Delivery Service
                                           │
                                           ▼
                                    Fill Event (Pending)
                                           │
                                           ▼
                                    Monitor Fill Status
                                           │
                                           ▼
                                    Fill Confirmed
                                           │
                                           ▼
                                    Settlement Service
                                    (Monitor Conditions)
                                           │
                                           ▼
                                    SettlementReadyEvent
                                           │
                                           ▼
                                    Delivery Service
                                    (Execute Settlement)
```

### Event Types Handled:

1. **Discovery Events**: New orders from various sources
2. **Order Events**: Order creation and updates  
3. **Fill Events**: Transaction execution status
4. **SettlementReady Events**: Settlement conditions met
5. **Settlement Events**: Settlement execution status
6. **Service Status Events**: Health and operational updates

## 🔌 Plugin System

The orchestrator delegates plugin management to specialized services:

### Plugin Creation Flow:

1. Configuration specifies plugin types
2. Services request plugins from global factory
3. Factory creates and validates plugins
4. Services manage plugin lifecycle

### Plugin Types:

- **State Plugins**: Memory, file, database storage
- **Discovery Plugins**: EIP-7683, custom order sources
- **Delivery Plugins**: Chain-specific transaction submission
- **Settlement Plugins**: Cross-chain settlement protocols
- **Order Processors**: Order format handlers

## 🚀 Usage Example

```rust
use solver_core::{Orchestrator, OrchestratorBuilder};
use solver_config::SolverConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = SolverConfig::from_file("config.toml")?;

    // Build orchestrator with services
    let orchestrator = OrchestratorBuilder::new()
        .with_config(config)
        .build()
        .await?;

    // Start processing
    orchestrator.start().await?;

    // Monitor health
    let health = orchestrator.get_health().await?;
    println!("Status: {:?}", health);

    // Graceful shutdown
    orchestrator.shutdown().await?;

    Ok(())
}
```

## 🔍 Critical Observations

### Strengths:

1. **Clean Separation**: Services manage their own plugins internally
2. **Event-Driven**: Asynchronous processing with clear event flow
3. **Fault Tolerance**: Comprehensive error handling and recovery
4. **Monitoring**: Built-in health checks and fill tracking
5. **Flexibility**: Plugin-based architecture allows easy extension

### Areas of Concern:

1. **Event Channel**: Single unbounded channel could be a bottleneck under high load
2. **Fill Monitoring**: Polling-based approach (5s interval) may miss rapid state changes
3. **Error Recovery**: Some error paths lead to immediate shutdown without retry
4. **Config Updates**: Runtime config updates don't propagate to all services

### Potential Optimizations:

1. **Bounded Channels**: Use bounded channels with backpressure
2. **Event Batching**: Process multiple events in single iteration
3. **Parallel Processing**: Events could be processed concurrently where safe
4. **Metrics Collection**: Add comprehensive metrics for monitoring
5. **Circuit Breaker**: Add circuit breaker pattern for failing services

## 🔗 Dependencies

### Internal Crates:

- `solver-types`: Core type definitions and traits
- `solver-config`: Configuration management
- `solver-state`: State storage service
- `solver-discovery`: Order discovery service
- `solver-delivery`: Transaction delivery service
- `solver-settlement`: Cross-chain settlement service
- `solver-plugin`: Plugin factory and implementations

### External Dependencies:

- `tokio`: Async runtime and utilities
- `async-trait`: Async trait support
- `anyhow`/`thiserror`: Error handling
- `serde`/`serde_json`: Serialization
- `tracing`: Structured logging
- `futures`: Async primitives
- `dashmap`: Concurrent hashmap
- `parking_lot`: Synchronization primitives
- `chrono`: Time handling

## 🏃 Runtime Behavior

### Startup Sequence:

1. Initialize lifecycle manager
2. Create service instances with plugins
3. Start state service
4. Start discovery service
5. Start delivery service
6. Start settlement service
7. Begin event processing
8. Start health monitoring
9. Start fill monitoring

### Shutdown Sequence:

1. Broadcast shutdown signal
2. Stop accepting new events
3. Process remaining events
4. Stop all services
5. Wait for background tasks
6. Clean up resources

## 🐛 Known Issues & Cruft

1. **Unused Parameter**: `create_order_processor` function creates processors but they're not used in the current flow
2. **Fill Timeout**: Hardcoded 5-minute timeout for fills may not suit all scenarios
3. **Event Cloning**: Events are cloned multiple times during processing
4. **String Allocations**: Frequent string allocations in error paths

## 🔮 Future Improvements

1. **Dynamic Plugin Loading**: Support hot-swapping plugins
2. **Event Prioritization**: Add priority queues for critical events
3. **Distributed Mode**: Support multi-instance orchestration
4. **State Snapshots**: Periodic state snapshots for recovery
5. **Admin API**: REST/gRPC API for runtime management

## 📊 Performance Considerations

- Event processing is sequential, limiting throughput
- State service calls are synchronous within event handling
- Fill monitoring creates periodic load spikes
- No connection pooling for service communications

The `solver-core` orchestrator provides a robust foundation for cross-chain order processing, with clear extension points through its plugin architecture and comprehensive error handling for production deployments.
