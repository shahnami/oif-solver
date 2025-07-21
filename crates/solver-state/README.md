# solver-state

## Overview

The `solver-state` module provides unified state management for runtime and persistent storage of orders, fills, and solver state. It abstracts storage backends behind a simple key-value interface, enabling seamless switching between in-memory, file-based, or distributed storage systems.

## Architecture

### Core Components

1. **StateStore Trait** - Universal key-value storage interface
2. **Type-Safe Wrappers** - OrderStore, FillStore for domain objects
3. **Built-in Backends** - Memory and file implementations
4. **Plugin Support** - Extensible to Redis, DynamoDB, PostgreSQL

### Design Principles

- **Simplicity**: Key-value interface covers all use cases
- **Type Safety**: Wrappers provide compile-time guarantees
- **Backend Agnostic**: Switch storage without code changes
- **Batch Operations**: Optional performance optimizations

## Structure

```rust
// Core storage trait - simple key-value interface
#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Bytes>>;
    async fn set(&self, key: &str, value: Bytes) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;

    // Optional batch operations
    async fn batch_get(&self, keys: &[String]) -> Result<Vec<Option<Bytes>>> {
        // Default implementation
    }
}

// Type-safe wrapper for orders
pub struct OrderStore<S: StateStore> {
    store: S,
    prefix: String,
}

// Type-safe wrapper for fills
pub struct FillStore<S: StateStore> {
    store: S,
    prefix: String,
}
```

## Abstractions

### Key-Value Design Rationale

The key-value interface was chosen for:

1. **Flexibility**: Works with any storage backend
2. **Simplicity**: Four methods cover all operations
3. **Performance**: Enables efficient indexing strategies
4. **Scalability**: Natural sharding by key prefix

### Key Naming Convention

```
order:{order_type}:{order_id}         # Order data
fill:{order_id}:{tx_hash}             # Fill data
settlement:{order_id}:{claim_hash}    # Settlement data
state:{component}:{key}               # Component state
```

### Type-Safe Wrappers

Wrappers provide domain-specific operations:

```rust
impl<S: StateStore> OrderStore<S> {
    pub async fn store_order(&self, id: &str, order: &[u8]) -> Result<()> {
        let key = format!("{}:{}", self.prefix, id);
        self.store.set(&key, order.into()).await
    }

    pub async fn get_order(&self, id: &str) -> Result<Option<Bytes>> {
        let key = format!("{}:{}", self.prefix, id);
        self.store.get(&key).await
    }

    pub async fn list_orders(&self) -> Result<Vec<String>> {
        self.store.list(&self.prefix).await
    }
}
```

## Usage

### Basic Usage

```rust
// Create in-memory store
let store = InMemoryState::new();

// Create type-safe wrapper
let order_store = OrderStore::new(store.clone(), "eip7683");

// Store order
let order_data = order.encode()?;
order_store.store_order(&order.id(), &order_data).await?;

// Retrieve order
if let Some(data) = order_store.get_order(&order.id()).await? {
    let order = Order::decode(&data)?;
}

// List all orders
let order_ids = order_store.list_orders().await?;
```

### Configuration

```toml
[state]
type = "memory"     # "memory", "file", or connection string
max_items = 10000
ttl_seconds = 3600

# File backend specific
[state.file]
base_path = "/var/lib/solver/state"
sync_interval = "30s"

# Redis backend (via connection string)
# type = "redis://localhost:6379/0"
```

## Built-in Backends

### InMemoryState

```rust
pub struct InMemoryState {
    data: Arc<RwLock<HashMap<String, Bytes>>>,
    max_items: Option<usize>,
    ttl: Option<Duration>,
}

impl InMemoryState {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            max_items: None,
            ttl: None,
        }
    }
    
    pub fn with_limits(max_items: usize, ttl: Duration) -> Self {
        Self {
            max_items: Some(max_items),
            ttl: Some(ttl),
            ..Self::new()
        }
    }
}
```

### FileState

```rust
pub struct FileState {
    base_path: PathBuf,
    cache: Option<Arc<RwLock<LruCache<String, Bytes>>>>,
}

impl FileState {
    fn key_to_path(&self, key: &str) -> PathBuf {
        // Convert key to safe file path
        let safe_key = key.replace(':', "/");
        self.base_path.join(safe_key)
    }
    
    async fn ensure_dir(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        Ok(())
    }
}
```

## Pros

1. **Flexibility**: Easy to switch storage backends
2. **Simplicity**: Minimal interface to implement
3. **Type Safety**: Compile-time guarantees with wrappers
4. **Performance**: Batch operations for efficiency
5. **Testability**: Easy to mock for testing

## Cons

1. **Serialization Overhead**: All data must be serialized
2. **No Complex Queries**: Limited to key-based lookups
3. **Manual Indexing**: Secondary indices must be maintained
4. **Transaction Support**: No built-in ACID guarantees

## Implementation Details

### Error Handling

```rust
#[derive(Error, Debug)]
pub enum StateError {
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    
    #[error("Storage backend error: {0}")]
    BackendError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}
```

### TTL Implementation

```rust
pub struct TtlEntry {
    value: Bytes,
    expires_at: Instant,
}

impl InMemoryState {
    async fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut data = self.data.write().await;
        
        data.retain(|_, entry| {
            entry.expires_at > now
        });
    }
}
```

### Cache Strategy

```rust
impl FileState {
    async fn get_with_cache(&self, key: &str) -> Result<Option<Bytes>> {
        // Check cache first
        if let Some(cache) = &self.cache {
            if let Some(value) = cache.read().await.get(key) {
                return Ok(Some(value.clone()));
            }
        }
        
        // Read from disk
        let path = self.key_to_path(key);
        match tokio::fs::read(&path).await {
            Ok(data) => {
                let bytes = Bytes::from(data);
                
                // Update cache
                if let Some(cache) = &self.cache {
                    cache.write().await.put(key.to_string(), bytes.clone());
                }
                
                Ok(Some(bytes))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
```

## Plugin Extension

### StateBackendPlugin Trait

```rust
pub trait StateBackendPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    async fn create_store(&self, config: Value) -> Result<Box<dyn StateStore>>;
    async fn health_check(&self) -> Result<()>;
}
```

### Example: Redis Plugin

```rust
pub struct RedisStatePlugin;

#[async_trait]
impl StateBackendPlugin for RedisStatePlugin {
    fn name(&self) -> &'static str {
        "redis"
    }
    
    async fn create_store(&self, config: Value) -> Result<Box<dyn StateStore>> {
        let url = config.get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Config("Missing redis URL"))?;
            
        let client = redis::Client::open(url)?;
        Ok(Box::new(RedisStateStore::new(client)))
    }
}
```

## Metrics

The module exposes metrics for monitoring:

- `state_operations_total` - Operations by type and backend
- `state_operation_duration_seconds` - Operation latency
- `state_keys_total` - Number of keys stored
- `state_size_bytes` - Storage size (where supported)

## Future Enhancements

1. **Transaction Support**: Multi-key atomic operations
2. **Query Capabilities**: Secondary indices and filtering
3. **Compression**: Automatic value compression
4. **Encryption**: At-rest encryption for sensitive data
5. **Replication**: Multi-region storage support
