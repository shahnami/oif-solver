# solver-discovery

## Overview

The `solver-discovery` module is responsible for discovering and monitoring orders from various sources. It provides a unified interface for different discovery mechanisms (on-chain monitoring, webhooks, mempool scanning) while maintaining source isolation and backpressure handling.

## Architecture

### Core Components

1. **DiscoveryManager** - Orchestrates multiple discovery sources
2. **DiscoverySource Enum** - Type-safe wrapper around discovery plugins
3. **OrderSink** - Channel-based order collection with backpressure
4. **DiscoveryEvent** - Standardized event format from all sources

### Design Principles

- **Source Isolation**: Each source runs independently
- **Backpressure Handling**: Bounded channels prevent memory exhaustion
- **Decoupled Processing**: Discovery separate from validation/storage
- **Event-Driven**: Asynchronous event streaming

## Structure

```rust
// Discovery source enumeration
#[derive(Clone)]
pub enum DiscoverySource {
    Chain(ChainDiscoveryPlugin),
    Webhook(WebhookDiscoveryPlugin),
    Custom(String, Arc<dyn DiscoverySourcePlugin>),
}

// Order sink with bounded channel for backpressure
pub struct OrderSink {
    sender: mpsc::Sender<DiscoveryEvent>,
}

// Standardized discovery event
pub struct DiscoveryEvent {
    pub id: String,
    pub data: Bytes,
    pub source: String,
    pub timestamp: Timestamp,
}

// Manager orchestrating discovery sources
pub struct DiscoveryManager {
    sources: Vec<DiscoverySource>,
    sink: OrderSink,
}
```

## Abstractions

### DiscoverySourcePlugin Trait

```rust
#[async_trait]
pub trait DiscoverySourcePlugin: Send + Sync {
    /// Start monitoring for events
    async fn start_monitoring(&self, sink: EventSink) -> Result<()>;

    /// Stop monitoring gracefully
    async fn stop_monitoring(&self) -> Result<()>;
}
```

### Why OrderSink?

The `OrderSink` pattern decouples discovery from state management:

1. **Deduplication**: Core handles duplicate detection, not sources
2. **Validation**: Order validation happens centrally
3. **Batching**: Core can batch process orders efficiently
4. **Simplicity**: Discovery sources focus only on event detection

## Usage

### Basic Usage

```rust
// Create discovery manager
let (tx, rx) = mpsc::channel(1000); // Bounded channel
let sink = OrderSink::new(tx);

let sources = vec![
    DiscoverySource::Chain(chain_plugin),
    DiscoverySource::Webhook(webhook_plugin),
];

let manager = DiscoveryManager::new(sources, sink);

// Start monitoring
manager.start().await?;

// Process discovered orders
while let Some(event) = rx.recv().await {
    match process_order(event).await {
        Ok(_) => info!("Order processed: {}", event.id),
        Err(e) => error!("Failed to process order: {}", e),
    }
}
```

### Configuration

```toml
[discovery]
sources = ["onchain", "webhook"]

[discovery.onchain]
chain_id = 1
contracts = ["0x1234...", "0x5678..."]
start_block = 18000000
poll_interval = "12s"

[discovery.webhook]
port = 8081
auth_token = "${WEBHOOK_AUTH_TOKEN}"
path = "/orders"
```

## Pros

1. **Modularity**: Easy to add new discovery sources
2. **Resilience**: Source failures don't affect others
3. **Scalability**: Parallel monitoring across sources
4. **Backpressure**: Natural flow control via channels
5. **Type Safety**: Compile-time guarantees

## Cons

1. **Channel Overhead**: Additional hop for order processing
2. **Memory Usage**: Buffered orders in channels
3. **Complexity**: Multiple moving parts to coordinate
4. **Latency**: Buffering can add small delays

## Implementation Details

### Chain Discovery Plugin

```rust
pub struct ChainDiscoveryPlugin {
    chain_id: ChainId,
    provider: Arc<Provider<Http>>, // Read-only, no signer
    contracts: Vec<Address>,
    poll_interval: Duration,
}

impl ChainDiscoveryPlugin {
    async fn monitor_blocks(&self, sink: EventSink) -> Result<()> {
        let mut block_stream = self.provider
            .watch_blocks()
            .await?
            .interval(self.poll_interval);

        while let Some(block_hash) = block_stream.next().await {
            let block = self.provider.get_block(block_hash).await?;
            
            // Check for relevant events
            let logs = self.provider
                .get_logs(&Filter::new()
                    .address(self.contracts.clone())
                    .from_block(block.number.unwrap())
                    .to_block(block.number.unwrap()))
                .await?;

            for log in logs {
                let event = self.parse_log(log)?;
                sink.send_event(event).await?;
            }
        }
        
        Ok(())
    }
}
```

### Webhook Discovery Plugin

```rust
pub struct WebhookDiscoveryPlugin {
    port: u16,
    auth_token: String,
    path: String,
}

impl WebhookDiscoveryPlugin {
    async fn start_server(&self, sink: EventSink) -> Result<()> {
        let app = Router::new()
            .route(&self.path, post(handle_webhook))
            .layer(middleware::auth(self.auth_token.clone()))
            .with_state(sink);

        Server::bind(&format!("0.0.0.0:{}", self.port).parse()?)
            .serve(app.into_make_service())
            .await?;

        Ok(())
    }
}
```

### Backpressure Handling

```rust
impl OrderSink {
    pub async fn send(&self, event: DiscoveryEvent) -> Result<()> {
        // Channel automatically provides backpressure
        match self.sender.try_send(event) {
            Ok(_) => Ok(()),
            Err(TrySendError::Full(event)) => {
                // Channel full, wait for space
                warn!("Discovery channel full, applying backpressure");
                self.sender.send(event).await
                    .map_err(|_| Error::ChannelClosed)?;
                Ok(())
            }
            Err(TrySendError::Closed(_)) => {
                Err(Error::ChannelClosed)
            }
        }
    }
}
```

### Error Recovery

```rust
impl DiscoveryManager {
    async fn start_with_recovery(&self) -> Result<()> {
        for source in &self.sources {
            let sink = self.sink.clone();
            
            tokio::spawn(async move {
                loop {
                    match source.start_monitoring(sink.clone()).await {
                        Ok(_) => info!("Discovery source stopped normally"),
                        Err(e) => {
                            error!("Discovery source error: {}", e);
                            // Exponential backoff
                            tokio::time::sleep(Duration::from_secs(60)).await;
                            continue;
                        }
                    }
                    break;
                }
            });
        }
        Ok(())
    }
}
```

## Metrics

The module exposes metrics for monitoring:

- `discovery_events_total` - Total events by source
- `discovery_errors_total` - Errors by source and type
- `discovery_lag_seconds` - Time behind chain tip
- `discovery_channel_utilization` - Channel capacity usage

## Future Enhancements

1. **Mempool Integration**: Monitor pending transactions
2. **GraphQL Support**: Alternative to RPC for historical data
3. **Event Filtering**: In-source filtering to reduce noise
4. **Replay Support**: Reprocess historical blocks
5. **Cross-Chain Coordination**: Unified discovery across chains
