//! # API Poller Discovery Plugin
//!
//! Provides discovery via periodic polling of REST APIs.
//!
//! This plugin periodically polls external REST APIs to discover new orders
//! and order status updates from off-chain sources like DEX aggregators,
//! order book systems, or market data providers.

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use solver_types::plugins::*;
use solver_types::Event;

/// Configuration for API poller discovery plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiPollerConfig {
    /// Base URL of the API to poll
    pub api_url: String,
    /// HTTP headers to include in requests
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Polling interval in milliseconds
    pub poll_interval_ms: u64,
    /// HTTP timeout in milliseconds
    pub timeout_ms: u64,
    /// Chain ID for discovered orders
    pub chain_id: ChainId,
    /// Source identifier for events
    pub source_name: String,
    /// Maximum number of orders to fetch per request
    pub max_orders_per_request: usize,
}

impl Default for ApiPollerConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:3000/api/orders".to_string(),
            headers: HashMap::new(),
            poll_interval_ms: 5000, // 5 seconds
            timeout_ms: 10000,      // 10 seconds
            chain_id: 1,
            source_name: "api_poller".to_string(),
            max_orders_per_request: 100,
        }
    }
}

/// API response structure for order data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiOrderResponse {
    /// List of orders
    pub orders: Vec<ApiOrder>,
    /// Pagination cursor for next request
    pub next_cursor: Option<String>,
    /// Whether there are more orders available
    pub has_more: bool,
}

/// Individual order from API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiOrder {
    /// Order ID
    pub id: String,
    /// Order status/event type
    pub status: String,
    /// User address
    pub user: Option<String>,
    /// Raw order data (hex string or JSON)
    pub order_data: Option<String>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Timestamp of the order/update
    pub timestamp: Option<u64>,
    /// Block number (if applicable)
    pub block_number: Option<u64>,
    /// Transaction hash (if applicable)
    pub transaction_hash: Option<String>,
}

/// API poller discovery plugin implementation.
pub struct ApiPollerDiscoveryPlugin {
    /// Plugin configuration
    config: ApiPollerConfig,
    /// Plugin metrics
    metrics: PluginMetrics,
    /// Whether plugin is initialized
    is_initialized: bool,
    /// Whether monitoring is active
    is_monitoring: bool,
    /// Event statistics
    events_discovered: Arc<RwLock<u64>>,
    errors_count: Arc<RwLock<u64>>,
    /// Last polling cursor
    last_cursor: Arc<RwLock<Option<String>>>,
    /// Shutdown signal sender
    shutdown_tx: Option<mpsc::UnboundedSender<()>>,
}

impl std::fmt::Debug for ApiPollerDiscoveryPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiPollerDiscoveryPlugin")
            .field("config", &self.config)
            .field("is_initialized", &self.is_initialized)
            .field("is_monitoring", &self.is_monitoring)
            .finish()
    }
}

impl Default for ApiPollerDiscoveryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiPollerDiscoveryPlugin {
    /// Create a new API poller discovery plugin.
    pub fn new() -> Self {
        Self {
            config: ApiPollerConfig::default(),
            metrics: PluginMetrics::new(),
            is_initialized: false,
            is_monitoring: false,
            events_discovered: Arc::new(RwLock::new(0)),
            errors_count: Arc::new(RwLock::new(0)),
            last_cursor: Arc::new(RwLock::new(None)),
            shutdown_tx: None,
        }
    }

    /// Create a new plugin with configuration.
    pub fn with_config(config: ApiPollerConfig) -> Self {
        Self {
            config,
            metrics: PluginMetrics::new(),
            is_initialized: false,
            is_monitoring: false,
            events_discovered: Arc::new(RwLock::new(0)),
            errors_count: Arc::new(RwLock::new(0)),
            last_cursor: Arc::new(RwLock::new(None)),
            shutdown_tx: None,
        }
    }

    /// Parse API order into discovery event.
    async fn parse_api_order(
        config: &ApiPollerConfig,
        order: ApiOrder,
    ) -> PluginResult<DiscoveryEvent> {
        // Map string status to EventType enum
        let event_type = match order.status.as_str() {
            "created" | "open" | "new" => EventType::OrderCreated,
            "filled" | "completed" | "executed" => EventType::OrderFilled,
            "updated" | "modified" => EventType::OrderUpdated,
            "cancelled" | "canceled" => EventType::OrderCancelled,
            _ => {
                return Err(PluginError::InvalidConfiguration(format!(
                    "Unknown order status: {}",
                    order.status
                )));
            }
        };

        // Parse order data
        let raw_data = if let Some(data) = order.order_data {
            if data.starts_with("0x") || data.chars().all(|c| c.is_ascii_hexdigit()) {
                // Treat as hex data
                match hex::decode(data.strip_prefix("0x").unwrap_or(&data)) {
                    Ok(bytes) => Bytes::from(bytes),
                    Err(_) => Bytes::from(data.as_bytes().to_vec()),
                }
            } else {
                // Treat as JSON or raw string
                Bytes::from(data.as_bytes().to_vec())
            }
        } else {
            Bytes::new()
        };

        // Create parsed data
        let parsed_data = ParsedEventData {
            order_id: Some(order.id.clone()),
            user: order.user.clone(),
            contract_address: order.metadata.get("contract_address").cloned(),
            method_signature: Some(order.status.clone()),
            decoded_params: HashMap::new(),
        };

        let timestamp = order.timestamp.unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        });

        Ok(DiscoveryEvent {
            id: order.id,
            event_type,
            source: config.source_name.clone(),
            chain_id: config.chain_id,
            block_number: order.block_number,
            transaction_hash: order.transaction_hash,
            timestamp,
            raw_data,
            parsed_data: Some(parsed_data),
            metadata: EventMetadata {
                source_specific: order.metadata,
                confidence_score: 0.85, // Good confidence for API data
                processing_delay: None,  // Real-time processing
                retry_count: 0,
            },
        })
    }

    /// Fetch orders from the API.
    async fn fetch_orders(
        config: &ApiPollerConfig,
        cursor: Option<String>,
    ) -> PluginResult<ApiOrderResponse> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to create HTTP client: {}", e)))?;

        let mut request = client.get(&config.api_url);

        // Add headers
        for (key, value) in &config.headers {
            request = request.header(key, value);
        }

        // Add query parameters
        if let Some(cursor) = cursor {
            request = request.query(&[("cursor", cursor)]);
        }
        request = request.query(&[("limit", config.max_orders_per_request)]);

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| PluginError::ExecutionFailed(format!("HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(PluginError::ExecutionFailed(format!(
                "HTTP request failed with status: {}",
                response.status()
            )));
        }

        let order_response: ApiOrderResponse = response
            .json()
            .await
            .map_err(|e| PluginError::ExecutionFailed(format!("Failed to parse JSON response: {}", e)))?;

        Ok(order_response)
    }

    /// Background task for polling the API.
    async fn polling_task(
        config: ApiPollerConfig,
        sink: EventSink<Event>,
        mut shutdown_rx: mpsc::UnboundedReceiver<()>,
        events_discovered: Arc<RwLock<u64>>,
        errors_count: Arc<RwLock<u64>>,
        last_cursor: Arc<RwLock<Option<String>>>,
    ) -> PluginResult<()> {
        let mut poll_interval = interval(Duration::from_millis(config.poll_interval_ms));

        info!("Starting API polling for {}", config.api_url);

        loop {
            tokio::select! {
                _ = poll_interval.tick() => {
                    let cursor = last_cursor.read().await.clone();
                    
                    match Self::fetch_orders(&config, cursor).await {
                        Ok(response) => {
                            debug!("Fetched {} orders from API", response.orders.len());
                            
                            for order in response.orders {
                                match Self::parse_api_order(&config, order).await {
                                    Ok(event) => {
                                        if let Err(e) = sink.send_discovery(event) {
                                            error!("Failed to send discovery event: {}", e);
                                            let mut errors = errors_count.write().await;
                                            *errors += 1;
                                        } else {
                                            let mut count = events_discovered.write().await;
                                            *count += 1;
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to parse API order: {}", e);
                                        let mut errors = errors_count.write().await;
                                        *errors += 1;
                                    }
                                }
                            }
                            
                            // Update cursor for next request
                            if let Some(next_cursor) = response.next_cursor {
                                let mut cursor = last_cursor.write().await;
                                *cursor = Some(next_cursor);
                            }
                        }
                        Err(e) => {
                            error!("Failed to fetch orders from API: {}", e);
                            let mut errors = errors_count.write().await;
                            *errors += 1;
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal, stopping API polling");
                    break;
                }
            }
        }

        info!("API polling stopped");
        Ok(())
    }
}

#[async_trait]
impl BasePlugin for ApiPollerDiscoveryPlugin {
    fn plugin_type(&self) -> &'static str {
        "api_poller_discovery"
    }

    fn name(&self) -> String {
        format!("API Poller Discovery Plugin ({})", self.config.api_url)
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn description(&self) -> &'static str {
        "Discovers orders by polling REST APIs periodically"
    }

    async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()> {
        debug!("Initializing API poller discovery plugin");

        // Parse configuration
        if let Some(api_url) = config.get_string("api_url") {
            self.config.api_url = api_url;
        }

        if let Some(poll_interval) = config.get_number("poll_interval_ms") {
            self.config.poll_interval_ms = poll_interval as u64;
        }

        if let Some(timeout) = config.get_number("timeout_ms") {
            self.config.timeout_ms = timeout as u64;
        }

        if let Some(chain_id) = config.get_number("chain_id") {
            self.config.chain_id = chain_id as ChainId;
        }

        if let Some(source_name) = config.get_string("source_name") {
            self.config.source_name = source_name;
        }

        if let Some(max_orders) = config.get_number("max_orders_per_request") {
            self.config.max_orders_per_request = max_orders as usize;
        }

        // Parse headers if provided
        if let Some(headers_value) = config.config.get("headers") {
            if let Some(headers_map) = headers_value.as_object() {
                for (key, value) in headers_map {
                    if let Some(value_str) = value.as_string() {
                        self.config.headers.insert(key.clone(), value_str);
                    }
                }
            }
        }

        self.is_initialized = true;
        debug!("API poller discovery plugin initialized successfully");
        Ok(())
    }

    fn validate_config(&self, config: &PluginConfig) -> PluginResult<()> {
        let schema = self.config_schema();
        schema.validate(config)?;

        // Additional validation
        if let Some(poll_interval) = config.get_number("poll_interval_ms") {
            if poll_interval < 1000 {
                return Err(PluginError::InvalidConfiguration(
                    "poll_interval_ms must be at least 1000ms".to_string(),
                ));
            }
        }

        if let Some(timeout) = config.get_number("timeout_ms") {
            if timeout <= 0 {
                return Err(PluginError::InvalidConfiguration(
                    "timeout_ms must be positive".to_string(),
                ));
            }
        }

        if let Some(api_url) = config.get_string("api_url") {
            if !api_url.starts_with("http://") && !api_url.starts_with("https://") {
                return Err(PluginError::InvalidConfiguration(
                    "api_url must be a valid HTTP/HTTPS URL".to_string(),
                ));
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> PluginResult<PluginHealth> {
        if !self.is_initialized {
            return Ok(PluginHealth::unhealthy("Plugin not initialized"));
        }

        let events_discovered = *self.events_discovered.read().await;
        let errors_count = *self.errors_count.read().await;

        // Test API connectivity
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(self.config.timeout_ms))
            .build()
            .map_err(|_| PluginError::ExecutionFailed("Failed to create HTTP client".to_string()))?;

        match client.head(&self.config.api_url).send().await {
            Ok(response) if response.status().is_success() => {
                Ok(PluginHealth::healthy("API poller discovery plugin is operational")
                    .with_detail("api_url", self.config.api_url.clone())
                    .with_detail("events_discovered", events_discovered.to_string())
                    .with_detail("errors_count", errors_count.to_string())
                    .with_detail("is_monitoring", self.is_monitoring.to_string()))
            }
            Ok(response) => Ok(PluginHealth::degraded(format!(
                "API returned status: {}",
                response.status()
            ))),
            Err(e) => Ok(PluginHealth::unhealthy(format!("API connectivity failed: {}", e))),
        }
    }

    async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
        let mut metrics = self.metrics.clone();

        metrics.set_gauge("events_discovered", *self.events_discovered.read().await as f64);
        metrics.set_gauge("errors_count", *self.errors_count.read().await as f64);
        metrics.set_gauge("is_monitoring", if self.is_monitoring { 1.0 } else { 0.0 });
        metrics.set_gauge("poll_interval_ms", self.config.poll_interval_ms as f64);

        Ok(metrics)
    }

    async fn shutdown(&mut self) -> PluginResult<()> {
        info!("Shutting down API poller discovery plugin");

        if self.is_monitoring {
            self.stop_monitoring().await?;
        }

        self.is_initialized = false;
        info!("API poller discovery plugin shutdown complete");
        Ok(())
    }

    fn config_schema(&self) -> PluginConfigSchema {
        PluginConfigSchema::new()
            .required("api_url", ConfigFieldType::String, "Base URL of the API to poll")
            .optional(
                "poll_interval_ms",
                ConfigFieldType::Number,
                "Polling interval in milliseconds",
                Some(ConfigValue::from(5000i64)),
            )
            .optional(
                "timeout_ms",
                ConfigFieldType::Number,
                "HTTP timeout in milliseconds",
                Some(ConfigValue::from(10000i64)),
            )
            .required("chain_id", ConfigFieldType::Number, "Chain ID for discovered orders")
            .optional(
                "source_name",
                ConfigFieldType::String,
                "Source identifier for events",
                Some(ConfigValue::from("api_poller")),
            )
            .optional(
                "max_orders_per_request",
                ConfigFieldType::Number,
                "Maximum orders to fetch per request",
                Some(ConfigValue::from(100i64)),
            )
            .optional(
                "headers",
                ConfigFieldType::Object,
                "HTTP headers to include in requests",
                None,
            )
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[async_trait]
impl DiscoveryPlugin for ApiPollerDiscoveryPlugin {
    async fn start_monitoring(&mut self, sink: EventSink<Event>) -> PluginResult<()> {
        if !self.is_initialized {
            return Err(PluginError::ExecutionFailed("Plugin not initialized".to_string()));
        }

        if self.is_monitoring {
            return Err(PluginError::ExecutionFailed("Already monitoring".to_string()));
        }

        debug!("Starting API poller discovery monitoring");

        let (shutdown_tx, shutdown_rx) = mpsc::unbounded_channel();
        self.shutdown_tx = Some(shutdown_tx);

        // Start polling task
        let config = self.config.clone();
        let events_discovered = self.events_discovered.clone();
        let errors_count = self.errors_count.clone();
        let last_cursor = self.last_cursor.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::polling_task(
                config,
                sink,
                shutdown_rx,
                events_discovered,
                errors_count,
                last_cursor,
            ).await {
                error!("API polling task failed: {}", e);
            }
        });

        self.is_monitoring = true;
        info!("API poller discovery monitoring started for {}", self.config.api_url);
        Ok(())
    }

    async fn stop_monitoring(&mut self) -> PluginResult<()> {
        if !self.is_monitoring {
            return Ok(());
        }

        info!("Stopping API poller discovery monitoring");

        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        self.is_monitoring = false;
        info!("API poller discovery monitoring stopped");
        Ok(())
    }

    async fn get_status(&self) -> PluginResult<DiscoveryStatus> {
        Ok(DiscoveryStatus {
            is_running: self.is_monitoring,
            current_block: None, // Not applicable for API polling
            target_block: None,  // Not applicable for API polling
            events_discovered: *self.events_discovered.read().await,
            last_event_timestamp: None, // Could be tracked if needed
            errors_count: *self.errors_count.read().await,
            average_processing_time_ms: 0.0, // Could be calculated
        })
    }

    async fn discover_range(
        &self,
        _from_block: u64,
        _to_block: u64,
        _sink: EventSink<Event>,
    ) -> PluginResult<u64> {
        // API polling doesn't support block-based historical discovery
        Err(PluginError::NotSupported(
            "Block-based historical discovery not supported for API polling".to_string(),
        ))
    }

    fn supported_event_types(&self) -> Vec<EventType> {
        vec![
            EventType::OrderCreated,
            EventType::OrderFilled,
            EventType::OrderUpdated,
            EventType::OrderCancelled,
        ]
    }

    fn chain_id(&self) -> ChainId {
        self.config.chain_id
    }

    async fn can_monitor_contract(&self, _contract_address: &String) -> PluginResult<bool> {
        // API polling can work with any "contract" (source)
        Ok(true)
    }

    async fn subscribe_to_events(&mut self, _filters: Vec<EventFilter>) -> PluginResult<()> {
        // Event filtering could be implemented in API requests
        Ok(())
    }

    async fn unsubscribe_from_events(&mut self, _filters: Vec<EventFilter>) -> PluginResult<()> {
        Ok(())
    }
} 