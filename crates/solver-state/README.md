# Solver State - Unified State Management Service

The `solver-state` crate provides a plugin-based state management service that abstracts storage backends behind a unified interface. It enables seamless switching between different storage systems (memory, file, Redis, etc.) while maintaining a consistent API for the solver's runtime and persistent storage needs.

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                           STATE SERVICE                                  │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Core Components                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  Plugin     │  │   Active     │  │    Background          │  │  │
│  │  │  Registry   │  │   Backend    │  │    Cleanup Task        │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                      Unified API                                   │  │
│  │  ┌────────┐  ┌────────┐  ┌─────────┐  ┌──────────────────────┐  │  │
│  │  │  Get   │  │  Set   │  │ Delete  │  │  Batch Operations    │  │  │
│  │  │  TTL   │  │ Atomic │  │  List   │  │  Stats & Cleanup     │  │  │
│  │  └────────┘  └────────┘  └─────────┘  └──────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                          ┌─────────┴─────────┐
                          │                   │
                 ┌────────▼────────┐ ┌────────▼────────┐
                 │ State Plugin A  │ │ State Plugin B  │
                 │   (e.g. Memory) │ │  (e.g. File)   │
                 └────────┬────────┘ └────────┬────────┘
                          │                   │
                 ┌────────▼────────┐ ┌────────▼────────┐
                 │   State Store   │ │   State Store   │
                 │   Instance      │ │   Instance      │
                 └─────────────────┘ └─────────────────┘
```

## Module Structure

```
solver-state/
├── src/
│   └── lib.rs          # Main service implementation
├── Cargo.toml          # Dependencies
└── README.md           # This file
```

## Key Components

### 1. **StateService** (`lib.rs`)

The main service that manages state plugins and provides a unified interface.

**Key Responsibilities:**

- Plugin registration and management
- Backend activation and switching
- Unified API for all state operations
- Health monitoring and metrics collection
- Background cleanup tasks

**Internal Structure:**

```rust
pub struct StateService {
    // Thread-safe plugin registry
    plugins: Arc<RwLock<HashMap<String, Arc<dyn StatePlugin>>>>,

    // Currently active backend
    active_backend: Arc<RwLock<Option<String>>>,
    active_store: Arc<RwLock<Option<Arc<dyn StateStore>>>>,

    // Service configuration
    config: StateConfig,
}
```

### 2. **Plugin System**

The service works with plugins that implement the `StatePlugin` trait:

- Each plugin represents a different storage backend
- Plugins create `StateStore` instances for actual operations
- Multiple plugins can be registered, but only one is active

### 3. **Unified Operations**

All storage operations go through the service's unified API:

- Basic operations: get, set, delete, exists
- TTL support: set_with_ttl
- Batch operations: batch_get, batch_set, batch_delete
- Atomic updates: atomic_update
- Management: list_keys, cleanup, get_stats

## State Management Flow

```text
Application → StateService → Active Plugin → StateStore Instance
                   │                              │
                   ├─ Plugin Registry             ├─ Memory Store
                   ├─ Backend Selection           ├─ File Store
                   └─ Operation Routing           └─ Redis Store
```

### Flow Steps:

1. **Service Initialization**: Create service with configuration
2. **Plugin Registration**: Register available state plugins
3. **Backend Activation**: Activate default or specific backend
4. **Store Creation**: Plugin creates store instance
5. **Operation Routing**: Service routes operations to active store
6. **Background Tasks**: Periodic cleanup and optimization

## Plugin Interface

State plugins must implement two main traits:

```rust
// Plugin trait for backend management
#[async_trait]
pub trait StatePlugin: BasePlugin {
    fn backend_type(&self) -> &'static str;
    async fn create_store(&self) -> PluginResult<Box<dyn StateStore>>;
    fn supports_ttl(&self) -> bool;
    fn supports_transactions(&self) -> bool;
    fn supports_atomic_operations(&self) -> bool;
    async fn get_backend_config(&self) -> PluginResult<BackendConfig>;
    async fn optimize(&self) -> PluginResult<OptimizationResult>;
    async fn backup(&self, destination: &str) -> PluginResult<BackupResult>;
    async fn restore(&self, source: &str) -> PluginResult<RestoreResult>;
}

// Store trait for actual operations
#[async_trait]
pub trait StateStore: Send + Sync + Debug {
    async fn get(&self, key: &str) -> PluginResult<Option<Bytes>>;
    async fn set(&self, key: &str, value: Bytes) -> PluginResult<()>;
    async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()>;
    async fn delete(&self, key: &str) -> PluginResult<()>;
    async fn exists(&self, key: &str) -> PluginResult<bool>;
    async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>>;
    async fn batch_get(&self, keys: &[String]) -> PluginResult<Vec<Option<Bytes>>>;
    async fn batch_set(&self, items: &[(String, Bytes)]) -> PluginResult<()>;
    async fn batch_delete(&self, keys: &[String]) -> PluginResult<()>;
    async fn atomic_update(&self, key: &str, updater: Box<dyn FnOnce(Option<Bytes>) -> PluginResult<Option<Bytes>> + Send>) -> PluginResult<()>;
    async fn get_stats(&self) -> PluginResult<StorageStats>;
    async fn cleanup(&self) -> PluginResult<CleanupStats>;
}
```

## Usage Example

```rust
use solver_state::{StateService, StateServiceBuilder};
use solver_types::configs::StateConfig;

// Build service with plugins
let service = StateServiceBuilder::new()
    .with_config(StateConfig {
        default_backend: "memory".to_string(),
        cleanup_interval_seconds: 300,
        enable_metrics: true,
        max_concurrent_operations: 100,
    })
    .with_plugin("memory".to_string(), memory_plugin, memory_config)
    .with_plugin("file".to_string(), file_plugin, file_config)
    .build()
    .await;

// Initialize with default backend
service.initialize().await?;

// Start background cleanup
service.start_cleanup_task().await;

// Basic operations
service.set("order:123", order_bytes).await?;
let data = service.get("order:123").await?;

// TTL support
service.set_with_ttl("temp:456", temp_data, Duration::from_secs(3600)).await?;

// Batch operations
let keys = vec!["order:1".to_string(), "order:2".to_string()];
let values = service.batch_get(&keys).await?;

// Atomic update
service.atomic_update("counter:total", Box::new(|current| {
    match current {
        Some(bytes) => {
            let count: u64 = u64::from_be_bytes(bytes.to_vec().try_into().unwrap());
            Ok(Some(Bytes::from((count + 1).to_be_bytes().to_vec())))
        }
        None => Ok(Some(Bytes::from(1u64.to_be_bytes().to_vec())))
    }
})).await?;

// Switch backend at runtime
service.switch_backend("file").await?;

// Get statistics
let stats = service.get_stats().await?;
println!("Keys: {}, Size: {} bytes", stats.key_count, stats.total_size_bytes);
```

## Critical Observations

### Strengths:

1. **Backend Agnostic**: Clean abstraction over different storage systems
2. **Runtime Switching**: Can change backends without restart
3. **Feature Discovery**: Plugins declare their capabilities
4. **Batch Operations**: Efficient bulk operations support
5. **Atomic Updates**: Thread-safe atomic operations

### Areas of Concern:

1. **No Transaction Support**: Despite trait method, no transaction implementation
2. **Memory Overhead**: Active store wrapped in multiple Arc/RwLock layers
3. **Plugin Initialization**: Awkward initialization due to Arc<dyn StatePlugin>
4. **No Connection Pooling**: Each store instance has its own connections
5. **Limited Error Context**: Generic PluginError loses backend-specific details

### Potential Optimizations:

1. **Connection Pooling**: Share connections across store instances
2. **Read-Write Splitting**: Separate read and write paths
3. **Caching Layer**: Add LRU cache in front of stores
4. **Async Cleanup**: Make cleanup non-blocking
5. **Plugin Hot Swap**: Support changing plugins without data loss

## Dependencies

### Internal Crates:

- `solver-types`: Core type definitions and plugin traits

### External Dependencies:

- `tokio`: Async runtime
- `async-trait`: Async trait support
- `serde`/`serde_json`: Serialization
- `bytes`: Efficient byte buffer handling
- `chrono`: Time and date handling
- `uuid`: Unique identifier generation
- `thiserror`/`anyhow`: Error handling
- `tracing`: Structured logging
- `dashmap`: Concurrent hashmap (though not used in current implementation)

## Runtime Behavior

### Service Lifecycle:

1. **Creation**: Build service with plugins and config
2. **Registration**: Register all state plugins
3. **Initialization**: Activate default backend
4. **Operation**: Route operations to active store
5. **Cleanup**: Periodic background cleanup
6. **Switching**: Can switch backends at runtime

### Concurrency Model:

- Service methods are thread-safe via RwLock
- Multiple readers for get operations
- Exclusive writer for set/delete operations
- Atomic updates use closure-based approach

## Known Issues & Cruft

1. **Unused Plugin Config**: Plugin configs passed to builder but not used
2. **No Plugin Initialization**: Plugins can't be initialized with their configs
3. **Missing Transaction Support**: Interface exists but no implementation
4. **Cleanup Task Lifecycle**: No way to stop cleanup task once started
5. **Backend Switching Data Loss**: No data migration when switching backends
6. **DashMap Dependency**: Listed but not used in implementation

## Future Improvements

1. **Transaction Support**: Implement multi-key transactions
2. **Data Migration**: Transfer data when switching backends
3. **Query Language**: Add simple query capabilities
4. **Compression**: Automatic value compression
5. **Encryption**: At-rest encryption support
6. **Replication**: Multi-backend replication
7. **Event Streaming**: Publish state changes

## Performance Considerations

- **Lock Contention**: Multiple RwLocks may cause contention
- **Arc Overhead**: Multiple Arc wrappings add indirection
- **No Caching**: Every operation hits the backend
- **Serialization Cost**: All data must be serialized to Bytes
- **No Connection Pooling**: Backend connections not reused

## Security Considerations

- **No Access Control**: Any code can access any key
- **No Encryption**: Data stored in plain format
- **No Audit Trail**: No logging of who accessed what
- **Plugin Trust**: Plugins have full storage access

The `solver-state` service provides a clean abstraction for state management with good plugin support, though it lacks some advanced features like transactions and data migration that would be valuable for production use.
