# Off-chain Discovery Plugins

This module provides discovery plugins for off-chain data sources. These plugins allow the OIF Solver to discover orders from external APIs, webhooks, message queues, and other non-blockchain sources.

## 📚 Overview

Off-chain discovery expands the solver's capability beyond blockchain monitoring to include:

- **REST API Polling**: Periodic polling of external APIs for order updates
- **Webhook Servers**: HTTP endpoints that receive real-time order notifications
- **Message Queues**: Integration with systems like Kafka, RabbitMQ, Redis Streams
- **WebSocket Feeds**: Real-time streaming from trading platforms
- **Database Polling**: Direct database connections for order discovery

## 🏗️ Architecture

```text
Off-chain Sources → Discovery Plugins → EventSink → Orchestrator → Order Processing
                                           ↓
                                    Same Event Flow
                                           ↓
                                    Delivery & Settlement
```

All off-chain discovery plugins follow the same pattern as on-chain discovery:
1. **Monitor** external sources for order events
2. **Parse** raw data into `DiscoveryEvent` format
3. **Send** events through `EventSink` to the orchestrator
4. **Process** events through the same pipeline as on-chain orders

## 🚀 Quick Start

### 1. Enable Off-chain Discovery

Add to your `config.toml`:

```toml
# API Poller Discovery
[plugins.discovery.my_api]
enabled = true
plugin_type = "api_poller_discovery"

[plugins.discovery.my_api.config]
api_url = "https://api.example.com/orders"
chain_id = 1
source_name = "my_api"
poll_interval_ms = 5000
```

### 2. Test with curl

```bash
# Test API endpoint (your API should return this format)
curl https://api.example.com/orders
{
  "orders": [
    {
      "id": "order_123",
      "status": "created",
      "user": "0x742d35Cc6634C0532925a3b8D6Ac6c001afb7f9c",
      "order_data": "0x...",
      "metadata": {},
      "timestamp": 1640995200
    }
  ],
  "has_more": false
}
```

### 3. Monitor Events

Check solver logs for discovery events:
```
2024-01-01T12:00:00Z [INFO] API poller discovery monitoring started for https://api.example.com/orders
2024-01-01T12:00:05Z [DEBUG] Fetched 3 orders from API
2024-01-01T12:00:05Z [INFO] Order created: order_123
```

## 📝 Available Plugins

### API Poller Discovery

Polls REST APIs for order updates on a configurable interval.

**Configuration:**
```toml
[plugins.discovery.api_poller.config]
api_url = "https://api.example.com/orders"          # Required: API endpoint
chain_id = 1                                        # Required: Chain ID for orders
source_name = "api_poller"                          # Optional: Source identifier
poll_interval_ms = 5000                             # Optional: Polling interval (default: 5000)
timeout_ms = 10000                                  # Optional: HTTP timeout (default: 10000)
max_orders_per_request = 100                        # Optional: Max orders per request (default: 100)

# Optional: Custom HTTP headers
[plugins.discovery.api_poller.config.headers]
Authorization = "Bearer token123"
User-Agent = "OIF-Solver/1.0"
```

**Expected API Response:**
```json
{
  "orders": [
    {
      "id": "order_123",
      "status": "created|filled|updated|cancelled",
      "user": "0x...",
      "order_data": "0x..." or "raw_string",
      "metadata": {},
      "timestamp": 1640995200,
      "block_number": 12345678,
      "transaction_hash": "0x..."
    }
  ],
  "next_cursor": "cursor_token",
  "has_more": true
}
```

### Webhook Discovery

Starts an HTTP server to receive webhook notifications from external sources.

**Configuration:**
```toml
[plugins.discovery.webhook.config]
bind_address = "0.0.0.0"                           # Optional: Bind address (default: 127.0.0.1)
port = 3001                                         # Optional: Port (default: 3000)
webhook_path = "/webhook"                           # Optional: Endpoint path (default: /webhook)
chain_id = 1                                        # Required: Chain ID for orders
source_name = "webhook_discovery"                   # Optional: Source identifier
auth_token = "secret123"                            # Optional: Bearer token authentication
max_body_size = 1048576                             # Optional: Max request size (default: 1MB)
```

**Expected Webhook Payload:**
```json
{
  "order_id": "order_123",
  "event_type": "order_created|order_filled|order_updated|order_cancelled",
  "user": "0x...",
  "order_data": "0x..." or "raw_string",
  "metadata": {},
  "timestamp": 1640995200
}
```

**Webhook Usage:**
```bash
# Send webhook notification
curl -X POST http://localhost:3001/webhook \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer secret123" \
  -d '{
    "order_id": "order_123",
    "event_type": "order_created",
    "user": "0x742d35Cc6634C0532925a3b8D6Ac6c001afb7f9c",
    "order_data": "0x1234...",
    "metadata": {"source": "dex_aggregator"}
  }'
```

## 🔧 Creating Custom Discovery Plugins

### 1. Define Configuration

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MyCustomConfig {
    pub endpoint: String,
    pub api_key: String,
    pub chain_id: ChainId,
    pub source_name: String,
}
```

### 2. Implement Plugin Structure

```rust
pub struct MyCustomDiscoveryPlugin {
    config: MyCustomConfig,
    metrics: PluginMetrics,
    is_initialized: bool,
    is_monitoring: bool,
    events_discovered: Arc<RwLock<u64>>,
    errors_count: Arc<RwLock<u64>>,
    shutdown_tx: Option<mpsc::UnboundedSender<()>>,
}
```

### 3. Implement BasePlugin Trait

```rust
#[async_trait]
impl BasePlugin for MyCustomDiscoveryPlugin {
    fn plugin_type(&self) -> &'static str {
        "my_custom_discovery"
    }
    
    async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()> {
        // Parse configuration from PluginConfig
        // Validate settings
        // Initialize any required clients/connections
        self.is_initialized = true;
        Ok(())
    }
    
    // ... other BasePlugin methods
}
```

### 4. Implement DiscoveryPlugin Trait

```rust
#[async_trait]
impl DiscoveryPlugin for MyCustomDiscoveryPlugin {
    async fn start_monitoring(&mut self, sink: EventSink<Event>) -> PluginResult<()> {
        // Start background task that monitors your data source
        let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();
        self.shutdown_tx = Some(shutdown_tx);
        
        tokio::spawn(async move {
            Self::monitoring_task(config, sink, shutdown_rx).await
        });
        
        self.is_monitoring = true;
        Ok(())
    }
    
    async fn stop_monitoring(&mut self) -> PluginResult<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        self.is_monitoring = false;
        Ok(())
    }
    
    // ... other DiscoveryPlugin methods
}
```

### 5. Implement Monitoring Logic

```rust
impl MyCustomDiscoveryPlugin {
    async fn monitoring_task(
        config: MyCustomConfig,
        sink: EventSink<Event>,
        mut shutdown_rx: mpsc::UnboundedReceiver<()>,
    ) -> PluginResult<()> {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        
        loop {
            tokio::select! {
                _ = interval.tick() => {
                    // Check your data source for new orders
                    match Self::fetch_orders(&config).await {
                        Ok(orders) => {
                            for order in orders {
                                let event = Self::parse_order(&config, order).await?;
                                sink.send_discovery(event)?;
                            }
                        }
                        Err(e) => error!("Failed to fetch orders: {}", e),
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Monitoring task shutting down");
                    break;
                }
            }
        }
        Ok(())
    }
    
    async fn parse_order(
        config: &MyCustomConfig,
        raw_order: RawOrder,
    ) -> PluginResult<DiscoveryEvent> {
        // Convert your raw order format to DiscoveryEvent
        Ok(DiscoveryEvent {
            id: raw_order.id,
            event_type: EventType::OrderCreated,
            source: config.source_name.clone(),
            chain_id: config.chain_id,
            // ... fill in other fields
        })
    }
}
```

### 6. Register with Plugin Factory

Add to your plugin factory implementation:

```rust
impl PluginFactory {
    pub fn create_discovery_plugin(
        &self,
        plugin_type: &str,
        config: PluginConfig,
    ) -> PluginResult<Box<dyn DiscoveryPlugin>> {
        match plugin_type {
            "my_custom_discovery" => {
                let mut plugin = MyCustomDiscoveryPlugin::new();
                plugin.initialize(config).await?;
                Ok(Box::new(plugin))
            }
            // ... other plugin types
        }
    }
}
```

## 📊 Monitoring & Debugging

### Health Checks

```bash
# Check discovery service status
curl http://localhost:8080/api/discovery/status

# Check specific plugin health
curl http://localhost:8080/api/health
```

### Metrics

Monitor key metrics:
- `events_discovered`: Total events found
- `errors_count`: Error count
- `is_monitoring`: Whether plugin is active
- `poll_interval_ms`: Polling frequency

### Logging

Enable debug logging to see discovery events:

```toml
[solver]
log_level = "debug"
```

Look for these log patterns:
```
[INFO] Starting API polling for https://api.example.com
[DEBUG] Fetched 5 orders from API
[INFO] Order created: order_123
[WARN] Failed to parse API order: Invalid status 'unknown'
[ERROR] Failed to fetch orders from API: Connection timeout
```

## 🔒 Security Considerations

### Authentication

- Use **API keys** for authenticated endpoints
- Store secrets in **environment variables**
- Implement **token rotation** for long-running processes

### Webhook Security

- Use **HTTPS** in production
- Validate **webhook signatures** if supported
- Implement **rate limiting** to prevent abuse
- Use **firewall rules** to restrict webhook access

### Data Validation

- **Validate** all incoming data before processing
- **Sanitize** user inputs and addresses
- **Check** order data integrity
- **Implement** circuit breakers for failing sources

## 🚀 Performance Optimization

### Polling Strategies

- Use **exponential backoff** for failed requests
- Implement **cursor-based pagination** for large datasets
- **Cache** API responses to reduce redundant calls
- **Batch** multiple orders in single requests

### Resource Management

- **Limit concurrent connections** to prevent resource exhaustion
- **Implement timeouts** for all network calls
- **Monitor memory usage** for long-running processes
- **Use connection pooling** for database sources

### Error Handling

- **Retry** transient failures with backoff
- **Circuit break** persistently failing sources
- **Dead letter queue** for problematic orders
- **Alert** on high error rates

## 🧪 Testing

### Unit Tests

```rust
#[tokio::test]
async fn test_api_polling() {
    let mut plugin = ApiPollerDiscoveryPlugin::new();
    let config = create_test_config();
    
    plugin.initialize(config).await.unwrap();
    assert!(plugin.is_initialized);
}
```

### Integration Tests

```rust
#[tokio::test]
async fn test_webhook_endpoint() {
    let payload = WebhookPayload {
        order_id: "test_order".to_string(),
        event_type: "order_created".to_string(),
        // ... other fields
    };
    
    let event = WebhookDiscoveryPlugin::parse_webhook_payload(&config, payload)
        .await
        .unwrap();
    
    assert_eq!(event.id, "test_order");
    assert_eq!(event.event_type, EventType::OrderCreated);
}
```

### Manual Testing

```bash
# Test API endpoint
curl https://your-api.com/orders

# Test webhook endpoint
curl -X POST http://localhost:3001/webhook \
  -H "Content-Type: application/json" \
  -d '{"order_id": "test", "event_type": "order_created"}'
```

## 📚 Examples

See `config/offchain-example.toml` for a complete configuration example with multiple off-chain discovery sources.

## 🤝 Contributing

When adding new off-chain discovery plugins:

1. **Follow** the existing plugin pattern
2. **Add** comprehensive configuration validation
3. **Implement** proper error handling and retries
4. **Include** metrics and health checks
5. **Write** unit and integration tests
6. **Document** configuration options and API formats
7. **Add** example configurations

## 🔗 Related

- [Discovery Service Documentation](../../discovery/README.md)
- [Plugin System Overview](../README.md)
- [Event System Types](../../../solver-types/src/events.rs)
- [Configuration Examples](../../../../config/) 