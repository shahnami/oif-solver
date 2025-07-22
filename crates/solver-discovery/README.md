# Solver Discovery - Multi-Chain Order Discovery Service

The `solver-discovery` crate provides a plugin-based order discovery service that monitors multiple blockchain sources for order events. It orchestrates various discovery plugins and provides real-time monitoring capabilities.

## 🏗️ Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         DISCOVERY SERVICE                                │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Core Components                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  Plugin     │  │    Active    │  │      Discovery         │  │  │
│  │  │  Registry   │  │   Sources    │  │      Configuration     │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                    Active Sources                                  │  │
│  │  ┌────────────────────────────────────────────────────────────┐  │  │
│  │  │  Source Tracking (per plugin)                               │  │  │
│  │  │  - Status: Stopped/Starting/Running/Error/Stopping          │  │  │
│  │  │  - Plugin Name, Chain ID, Source Type                       │  │  │
│  │  │  - Direct Event Forwarding to Main Sink                     │  │  │
│  │  └────────────────────────────────────────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                     ┌──────────────┴─────────────┐
                     │                            │
            ┌────────▼─────────┐       ┌─────────▼─────────┐
            │ Discovery Plugin │       │ Discovery Plugin  │
            │   (Chain A)      │       │   (Chain B)       │
            └──────────────────┘       └───────────────────┘
                     │                            │
            ┌────────▼─────────┐       ┌─────────▼─────────┐
            │  Blockchain RPC  │       │  Event Webhooks   │
            └──────────────────┘       └───────────────────┘
```

## 📁 Module Structure

```
solver-discovery/
├── src/
│   ├── lib.rs          # Main service implementation
│   └── mod.rs          # Module exports
├── Cargo.toml          # Dependencies
└── README.md           # This file
```

## 🔑 Key Components

### 1. **DiscoveryService** (`lib.rs`)

The main service that orchestrates discovery plugins and manages event flow.

**Key Responsibilities:**

- Plugin lifecycle management (registration, start, stop)
- Source status tracking
- Multi-chain support coordination
- Event forwarding to orchestrator

**Internal Structure:**

```rust
pub struct DiscoveryService {
    // Thread-safe plugin registry
    plugins: Arc<RwLock<HashMap<String, Arc<Mutex<Box<dyn DiscoveryPlugin>>>>>>,

    // Active source tracking
    active_sources: Arc<RwLock<HashMap<String, DiscoverySource>>>,

    // Outbound event channel
    event_sink: EventSink<Event>,

    // Configuration
    config: DiscoveryConfig,
}
```

### 2. **DiscoverySource**

Tracks the state of each active discovery plugin:

```rust
pub struct DiscoverySource {
    pub plugin_name: String,
    pub chain_id: ChainId,
    pub source_type: String,
    pub status: SourceStatus,
}
```

### 3. **Source Status**

Represents the operational state of a discovery source:

```rust
pub enum SourceStatus {
    Stopped,
    Starting,
    Running(Instant),
    Error(String),
    Stopping,
}
```

## 🔄 Event Discovery Flow

```text
Blockchain/Source → Discovery Plugin → Event Creation → Main Event Sink
                                                              │
                                                              ▼
                                                       Orchestrator/Core
```

### Flow Steps:

1. **Plugin Monitoring**: Plugin monitors blockchain/source for events
2. **Event Creation**: Plugin creates DiscoveryEvent with metadata
3. **Event Forwarding**: Event sent directly to main event sink
4. **Processing**: Orchestrator handles event processing

## 🔌 Plugin System

### DiscoveryPlugin Interface:

The discovery plugin interface extends the base plugin interface with discovery-specific methods:

```rust
#[async_trait]
pub trait DiscoveryPlugin: BasePlugin {
    async fn start_monitoring(&mut self, sink: EventSink<Event>) -> PluginResult<()>;
    async fn stop_monitoring(&mut self) -> PluginResult<()>;
    async fn get_status(&self) -> PluginResult<DiscoveryStatus>;
    async fn discover_range(&self, from_block: u64, to_block: u64, sink: EventSink<Event>) -> PluginResult<u64>;
    fn supported_event_types(&self) -> Vec<EventType>;
    fn chain_id(&self) -> ChainId;
    async fn can_monitor_contract(&self, contract_address: &Address) -> PluginResult<bool>;
    async fn subscribe_to_events(&mut self, filters: Vec<EventFilter>) -> PluginResult<()>;
    async fn unsubscribe_from_events(&mut self, filters: Vec<EventFilter>) -> PluginResult<()>;
}
```

### Configuration:

```rust
pub struct DiscoveryConfig {
    pub realtime_monitoring: bool,          // Enable real-time monitoring
    pub max_events_per_second: u64,         // Rate limiting config (not enforced)
    pub max_concurrent_sources: usize,      // Max active sources
}
```

## 🚀 Usage Example

```rust
use solver_discovery::{DiscoveryService, DiscoveryServiceBuilder};
use solver_types::configs::DiscoveryConfig;

// Create event channel
let (tx, mut rx) = mpsc::unbounded_channel();
let event_sink = EventSink::new(tx);

// Build service with plugins
let service = DiscoveryServiceBuilder::new()
    .with_config(DiscoveryConfig {
        realtime_monitoring: true,
        max_concurrent_sources: 5,
        ..Default::default()
    })
    .with_plugin("eth_mainnet".to_string(), Box::new(eth_plugin), eth_config)
    .with_plugin("arbitrum".to_string(), Box::new(arb_plugin), arb_config)
    .build(event_sink)
    .await;

// Start specific source
service.start_source("eth_mainnet").await?;

// Or start all sources
service.start_all().await?;

// Process discovered events
tokio::spawn(async move {
    while let Some(event) = rx.recv().await {
        match event {
            Event::Discovery(discovery_event) => {
                println!("New order discovered: {}", discovery_event.id);
            }
            _ => {}
        }
    }
});

// Monitor status
let status = service.get_status().await;
for (name, source) in status {
    println!("{}: {:?}", name, source.status);
}
```

## 🔍 Critical Observations

### Strengths:

1. **Plugin Isolation**: Each plugin runs independently with its own mutex
2. **Multi-Chain Support**: Can monitor multiple chains simultaneously
3. **Flexible Configuration**: Configurable monitoring options
4. **Clean Architecture**: Well-separated concerns between service and plugins

### Areas of Concern:

1. **Double Mutex**: Plugins wrapped in `Arc<Mutex<Box<dyn DiscoveryPlugin>>>` - redundant Arc
2. **Missing Rate Limiting**: Config has `max_events_per_second` but no enforcement
3. **No Statistics**: No built-in metrics or statistics tracking
4. **Basic Status Tracking**: Limited to operational status only

### Potential Optimizations:

1. **Simplify Plugin Storage**: Remove redundant Arc wrapper
2. **Implement Rate Limiting**: Add rate limiter per source
3. **Add Statistics**: Track events discovered, errors, performance metrics
4. **Event Filtering**: Add configurable event filters at source level
5. **Metrics Export**: Add Prometheus metrics export

## 🔗 Dependencies

### Internal Crates:

- `solver-types`: Core type definitions and plugin traits

### External Dependencies:

- `tokio`: Async runtime and channels
- `async-trait`: Async trait support
- `futures`: Async utilities
- `tracing`: Structured logging
- `uuid`: Unique identifier generation
- `bytes`: Byte buffer handling
- `thiserror`/`anyhow`: Error handling
- `serde`/`serde_json`: Serialization support

## 🏃 Runtime Behavior

### Service Lifecycle:

1. **Plugin Registration**: Plugins initialized and registered
2. **Source Activation**: Start monitoring with provided event sink
3. **Event Discovery**: Plugins send events through sink
4. **Status Updates**: Source status tracked in real-time
5. **Event Forwarding**: Events sent to orchestrator

### Concurrency Model:

- Each plugin runs in its own async task
- Status updates use RwLock for concurrent access
- Direct event forwarding without buffering

## 🐛 Known Issues & Limitations

1. **No Rate Limiting**: Configuration exists but not implemented
2. **No Statistics**: No metrics or performance tracking
3. **No Event Filtering**: All events forwarded without filtering
4. **Plugin Double-Lock**: Unnecessary complexity in plugin storage type
5. **No Graceful Shutdown**: Plugins stopped immediately without draining

## 🔮 Future Improvements

1. **Rate Limiting**: Implement per-source rate limiting
2. **Event Filtering**: Add source-level event filtering
3. **Statistics**: Add comprehensive metrics tracking
4. **Event Deduplication**: Prevent duplicate event processing
5. **Historical Sync**: Implement historical block range discovery
6. **Graceful Shutdown**: Allow plugins to finish processing before stop
7. **Circuit Breaker**: Add circuit breaker for failing sources
8. **Plugin Hot Reload**: Support adding/removing plugins at runtime

## 📊 Performance Considerations

- **Lock Contention**: Multiple RwLocks could cause contention under load
- **Direct Forwarding**: No buffering means backpressure affects plugins
- **No Batching**: Events processed individually, not batched

## ⚠️ Security Considerations

- **Plugin Trust**: Plugins have full event sink access
- **No Authentication**: No built-in auth for webhook sources
- **Event Validation**: No validation of event data integrity
- **Resource Limits**: No protection against malicious plugins

The `solver-discovery` service provides a foundation for multi-chain event discovery with a clean plugin architecture, though several features mentioned in configuration are not yet implemented.