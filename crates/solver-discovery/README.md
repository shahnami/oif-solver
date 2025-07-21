# Solver Discovery - Multi-Chain Order Discovery Service

The `solver-discovery` crate provides a plugin-based order discovery service that monitors multiple blockchain sources for order events. It orchestrates various discovery plugins, handles event deduplication, and provides real-time monitoring with comprehensive statistics.

## 🏗️ Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         DISCOVERY SERVICE                                │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Core Components                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  Plugin     │  │    Event     │  │    Discovery           │  │  │
│  │  │  Registry   │  │ Deduplicator │  │    Statistics          │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                    Active Sources                                  │  │
│  │  ┌────────────────────────────────────────────────────────────┐  │  │
│  │  │  Source Tracking (per plugin)                               │  │  │
│  │  │  - Status: Stopped/Starting/Running/Error/Stopping          │  │  │
│  │  │  - Statistics: events, blocks, errors, timing               │  │  │
│  │  │  - Filtered Event Sink with deduplication                   │  │  │
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
│   └── mod.rs          # Module re-exports (appears unused)
├── Cargo.toml          # Dependencies
└── README.md           # This file
```

## 🔑 Key Components

### 1. **DiscoveryService** (`lib.rs`)

The main service that orchestrates discovery plugins and manages event flow.

**Key Responsibilities:**

- Plugin lifecycle management (registration, start, stop)
- Event deduplication and filtering
- Source status tracking and statistics
- Multi-chain support coordination
- Health monitoring

**Internal Structure:**

```rust
pub struct DiscoveryService {
    // Thread-safe plugin registry
    plugins: Arc<RwLock<HashMap<String, Arc<Mutex<Box<dyn DiscoveryPlugin>>>>>>,

    // Active source tracking
    active_sources: Arc<RwLock<HashMap<String, DiscoverySource>>>,

    // Outbound event channel
    event_sink: EventSink<Event>,

    // Global statistics
    discovery_stats: Arc<RwLock<DiscoveryStats>>,

    // Configuration
    config: DiscoveryConfig,
}
```

### 2. **DiscoverySource**

Tracks the state and statistics of each active discovery plugin:

```rust
pub struct DiscoverySource {
    pub plugin_name: String,
    pub chain_id: ChainId,
    pub source_type: String,
    pub status: SourceStatus,
    pub stats: SourceStats,
}
```

### 3. **EventDeduplicator**

Prevents duplicate events within a configurable time window:

- Uses event key: `chain_id:event_id:tx_hash:event_type`
- Configurable deduplication window (default: 300 seconds)
- Automatic cleanup of old entries

### 4. **Filtered Event Sink**

Each plugin gets a filtered sink that:

- Applies deduplication logic
- Updates source statistics
- Forwards valid events to main sink
- Handles errors and updates error counts

## 🔄 Event Discovery Flow

```text
Blockchain/Source → Discovery Plugin → Filtered Sink → Deduplication
                                                            │
                                                            ▼
                                                    Statistics Update
                                                            │
                                                            ▼
                                                      Main Event Sink
                                                            │
                                                            ▼
                                                    Orchestrator/Core
```

### Flow Steps:

1. **Plugin Monitoring**: Plugin monitors blockchain/source for events
2. **Event Creation**: Plugin creates DiscoveryEvent with metadata
3. **Filtered Sink**: Event sent through plugin's filtered sink
4. **Deduplication**: Check if event was seen recently
5. **Statistics Update**: Update source stats (events count, blocks, etc.)
6. **Event Forwarding**: Send to main event sink for processing

## 🔌 Plugin System

### DiscoveryPlugin Interface:

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
    pub historical_sync: bool,              // Enable historical block sync
    pub realtime_monitoring: bool,          // Enable real-time monitoring
    pub dedupe_events: bool,                // Enable event deduplication
    pub max_event_age_seconds: u64,         // Max age for events (300s)
    pub max_events_per_second: u64,         // Rate limiting (1000/s)
    pub event_buffer_size: usize,           // Event buffer size (10000)
    pub deduplication_window_seconds: u64,  // Dedup window (300s)
    pub max_concurrent_sources: usize,      // Max active sources (10)
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
        dedupe_events: true,
        deduplication_window_seconds: 300,
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
    println!("{}: {:?} - {} events", name, source.status, source.stats.events_discovered);
}

// Get statistics
let stats = service.get_stats().await;
println!("Total events: {}, Rate: {}/min", stats.total_events_discovered, stats.events_per_minute);
```

## 🔍 Critical Observations

### Strengths:

1. **Plugin Isolation**: Each plugin runs independently with its own mutex
2. **Comprehensive Statistics**: Detailed tracking per source and globally
3. **Event Deduplication**: Prevents processing duplicate events
4. **Flexible Configuration**: Extensive configuration options
5. **Multi-Chain Support**: Can monitor multiple chains simultaneously

### Areas of Concern:

1. **Double Mutex**: Plugins wrapped in `Arc<Mutex<Box<dyn DiscoveryPlugin>>>` - redundant Arc
2. **mod.rs Confusion**: The mod.rs file appears to be unused boilerplate
3. **Missing Rate Limiting**: Config has `max_events_per_second` but no implementation
4. **No Event Filtering**: All events forwarded, no source-level filtering
5. **Memory Growth**: Deduplication cache grows until cleanup, could be optimized

### Potential Optimizations:

1. **Simplify Plugin Storage**: Remove redundant Arc wrapper
2. **Implement Rate Limiting**: Add rate limiter per source
3. **Event Filtering**: Add configurable event filters at source level
4. **Bloom Filter**: Use bloom filter for deduplication (memory efficient)
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
2. **Source Activation**: Start monitoring creates filtered sink
3. **Event Discovery**: Plugins send events through filtered sink
4. **Deduplication**: Events checked against recent history
5. **Statistics Update**: Per-source stats updated in real-time
6. **Event Forwarding**: Valid events sent to orchestrator

### Concurrency Model:

- Each plugin runs in its own async task
- Filtered sinks spawn dedicated forwarding tasks
- Statistics updates use RwLock for concurrent access
- Event deduplication uses async RwLock

## 🐛 Known Issues & Cruft

1. **Unused mod.rs**: The mod.rs file contains incorrect re-exports and appears unused
2. **Rate Limiting Missing**: Configuration exists but no implementation
3. **Historical Sync**: Config flag exists but no implementation in service
4. **Event Age Filtering**: `max_event_age_seconds` configured but not enforced
5. **Plugin Double-Lock**: Unnecessary complexity in plugin storage type
6. **No Graceful Shutdown**: Plugins stopped immediately without draining

## 🔮 Future Improvements

1. **Rate Limiting**: Implement per-source rate limiting
2. **Event Filtering**: Add source-level event filtering
3. **Historical Sync**: Implement historical block range discovery
4. **Graceful Shutdown**: Allow plugins to finish processing before stop
5. **Circuit Breaker**: Add circuit breaker for failing sources
6. **Plugin Hot Reload**: Support adding/removing plugins at runtime
7. **Event Replay**: Support replaying events from specific block

## 📊 Performance Considerations

- **Lock Contention**: Multiple RwLocks could cause contention
- **Channel Overhead**: Each plugin has dedicated channel and task
- **Deduplication Cost**: Hash map lookups for every event
- **Statistics Updates**: Frequent writes to shared state
- **No Batching**: Events processed individually, not batched

## ⚠️ Security Considerations

- **Plugin Trust**: Plugins have full event sink access
- **No Authentication**: No built-in auth for webhook sources
- **Event Validation**: No validation of event data integrity
- **Resource Limits**: No protection against malicious plugins

The `solver-discovery` service provides a robust foundation for multi-chain event discovery with good statistics and monitoring, though some configuration options lack implementation.
