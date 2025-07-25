# Minimalistic Solver Implementation

## Overview

A minimalistic, protocol-agnostic, event-driven solver architecture with clear separation of concerns and extensible design.

## Architecture

```
solver-core (orchestrator)
    ├── solver-config (configuration management)
    ├── solver-storage (storage abstraction)
    ├── solver-account (signing capabilities)
    ├── solver-delivery (transaction management)
    ├── solver-discovery (intent discovery)
    ├── solver-order (order processing & strategies)
    ├── solver-settlement (settlement orchestration)
    └── event-bus (inter-service communication)
```

## Implementation

### 1. Solver Config

**File: `crates/solver-config/src/lib.rs`**

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub solver: SolverConfig,
    pub storage: StorageConfig,
    pub delivery: DeliveryConfig,
    pub account: AccountConfig,
    pub discovery: DiscoveryConfig,
    pub order: OrderConfig,
    pub settlement: SettlementConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SolverConfig {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    pub backend: String,
    pub config: toml::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeliveryConfig {
    pub providers: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccountConfig {
    pub provider: String,
    pub config: toml::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscoveryConfig {
    pub sources: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OrderConfig {
    pub implementations: HashMap<String, toml::Value>,
    pub execution_strategy: StrategyConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StrategyConfig {
    pub strategy_type: String,
    pub config: toml::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SettlementConfig {
    pub implementations: HashMap<String, toml::Value>,
}

impl Config {
    pub fn from_file(path: &str) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path)?;
        Self::from_str(&content)
    }

    pub fn from_str(content: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(content)?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if self.solver.id.is_empty() {
            return Err(ConfigError::Validation("Solver ID cannot be empty".into()));
        }
        if self.delivery.providers.is_empty() {
            return Err(ConfigError::Validation("At least one delivery provider required".into()));
        }
        Ok(())
    }
}
```

### 2. Solver Storage

**File: `crates/solver-storage/src/lib.rs`**

```rust
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Not found")]
    NotFound,
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait StorageInterface: Send + Sync {
    async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<T, StorageError>;
    async fn set<T: Serialize>(&self, key: &str, value: &T, ttl: Option<Duration>) -> Result<(), StorageError>;
    async fn delete(&self, key: &str) -> Result<(), StorageError>;
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
}

pub struct StorageService {
    backend: Box<dyn StorageInterface>,
}

impl StorageService {
    pub fn new(backend: Box<dyn StorageInterface>) -> Self {
        Self { backend }
    }

    pub async fn store<T: Serialize>(&self, namespace: &str, id: &str, data: &T) -> Result<(), StorageError> {
        let key = format!("{}:{}", namespace, id);
        self.backend.set(&key, data, None).await
    }

    pub async fn retrieve<T: DeserializeOwned>(&self, namespace: &str, id: &str) -> Result<T, StorageError> {
        let key = format!("{}:{}", namespace, id);
        self.backend.get(&key).await
    }

    pub async fn remove(&self, namespace: &str, id: &str) -> Result<(), StorageError> {
        let key = format!("{}:{}", namespace, id);
        self.backend.delete(&key).await
    }
}
```

### 3. Solver Account

**File: `crates/solver-account/src/lib.rs`**

```rust
use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("Signing failed: {0}")]
    SigningFailed(String),
    #[error("Invalid key: {0}")]
    InvalidKey(String),
    #[error("Provider error: {0}")]
    Provider(String),
}

#[derive(Debug, Clone)]
pub struct Address(pub Vec<u8>);

#[derive(Debug, Clone)]
pub struct Signature(pub Vec<u8>);

#[derive(Debug, Clone)]
pub struct Transaction {
    pub to: Option<Address>,
    pub data: Vec<u8>,
    pub value: Vec<u8>,
    pub chain_id: u64,
}

#[async_trait]
pub trait AccountInterface: Send + Sync {
    async fn address(&self) -> Result<Address, AccountError>;
    async fn sign_transaction(&self, tx: &Transaction) -> Result<Signature, AccountError>;
    async fn sign_message(&self, message: &[u8]) -> Result<Signature, AccountError>;
}

pub struct AccountService {
    provider: Box<dyn AccountInterface>,
}

impl AccountService {
    pub fn new(provider: Box<dyn AccountInterface>) -> Self {
        Self { provider }
    }

    pub async fn get_address(&self) -> Result<Address, AccountError> {
        self.provider.address().await
    }

    pub async fn sign(&self, tx: &Transaction) -> Result<Signature, AccountError> {
        self.provider.sign_transaction(tx).await
    }
}
```

### 4. Solver Delivery

**File: `crates/solver-delivery/src/lib.rs`**

```rust
use async_trait::async_trait;
use solver_account::{Transaction, AccountService};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeliveryError {
    #[error("Network error: {0}")]
    Network(String),
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),
    #[error("No provider available")]
    NoProviderAvailable,
}

#[derive(Debug, Clone)]
pub struct TransactionHash(pub Vec<u8>);

#[derive(Debug, Clone)]
pub struct TransactionReceipt {
    pub hash: TransactionHash,
    pub block_number: u64,
    pub success: bool,
}

#[async_trait]
pub trait DeliveryInterface: Send + Sync {
    async fn submit(&self, tx: Transaction) -> Result<TransactionHash, DeliveryError>;
    async fn wait_for_confirmation(&self, hash: &TransactionHash, confirmations: u64) -> Result<TransactionReceipt, DeliveryError>;
    async fn get_status(&self, hash: &TransactionHash) -> Result<TransactionReceipt, DeliveryError>;
}

pub struct DeliveryService {
    providers: Vec<Box<dyn DeliveryInterface>>,
    account: Arc<AccountService>,
}

impl DeliveryService {
    pub fn new(providers: Vec<Box<dyn DeliveryInterface>>, account: Arc<AccountService>) -> Self {
        Self { providers, account }
    }

    pub async fn deliver(&self, mut tx: Transaction) -> Result<TransactionHash, DeliveryError> {
        // Sign transaction
        let signature = self.account.sign(&tx).await
            .map_err(|e| DeliveryError::Network(e.to_string()))?;

        // Try providers in order
        for provider in &self.providers {
            match provider.submit(tx.clone()).await {
                Ok(hash) => return Ok(hash),
                Err(e) => log::warn!("Provider failed: {}", e),
            }
        }

        Err(DeliveryError::NoProviderAvailable)
    }

    pub async fn confirm(&self, hash: &TransactionHash, confirmations: u64) -> Result<TransactionReceipt, DeliveryError> {
        // Use first available provider
        self.providers.first()
            .ok_or(DeliveryError::NoProviderAvailable)?
            .wait_for_confirmation(hash, confirmations)
            .await
    }
}
```

### 5. Solver Discovery

**File: `crates/solver-discovery/src/lib.rs`**

```rust
use async_trait::async_trait;
use tokio::sync::mpsc;
use serde::{Serialize, Deserialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Already monitoring")]
    AlreadyMonitoring,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: String,
    pub source: String,
    pub standard: String,
    pub metadata: IntentMetadata,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentMetadata {
    pub requires_auction: bool,
    pub exclusive_until: Option<u64>,
    pub discovered_at: u64,
}

#[async_trait]
pub trait DiscoveryInterface: Send + Sync {
    async fn start_monitoring(&self, sender: mpsc::UnboundedSender<Intent>) -> Result<(), DiscoveryError>;
    async fn stop_monitoring(&self) -> Result<(), DiscoveryError>;
}

pub struct DiscoveryService {
    sources: Vec<Box<dyn DiscoveryInterface>>,
}

impl DiscoveryService {
    pub fn new(sources: Vec<Box<dyn DiscoveryInterface>>) -> Self {
        Self { sources }
    }

    pub async fn start_all(&self, sender: mpsc::UnboundedSender<Intent>) -> Result<(), DiscoveryError> {
        for source in &self.sources {
            source.start_monitoring(sender.clone()).await?;
        }
        Ok(())
    }

    pub async fn stop_all(&self) -> Result<(), DiscoveryError> {
        for source in &self.sources {
            source.stop_monitoring().await?;
        }
        Ok(())
    }
}
```

### 6. Solver Order

**File: `crates/solver-order/src/lib.rs`**

```rust
use async_trait::async_trait;
use solver_account::{Transaction, Address};
use solver_discovery::Intent;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrderError {
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    #[error("Insufficient balance")]
    InsufficientBalance,
    #[error("Cannot satisfy order")]
    CannotSatisfyOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub standard: String,
    pub created_at: u64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct ExecutionParams {
    pub gas_price: U256,
    pub priority_fee: Option<U256>,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub gas_price: U256,
    pub timestamp: u64,
    pub solver_balance: HashMap<Address, U256>,
}

#[derive(Debug)]
pub enum ExecutionDecision {
    Execute(ExecutionParams),
    Skip(String),
    Defer(std::time::Duration),
}

#[derive(Debug, Clone)]
pub struct FillProof {
    pub tx_hash: TransactionHash,
    pub block_number: u64,
    pub attestation_data: Option<Vec<u8>>,
}

// For numeric operations
#[derive(Debug, Clone)]
pub struct U256(pub [u64; 4]);

// Standard-specific implementation
#[async_trait]
pub trait OrderInterface: Send + Sync {
    async fn validate_intent(&self, intent: &Intent) -> Result<Order, OrderError>;
    async fn generate_fill_transaction(&self, order: &Order, params: &ExecutionParams) -> Result<Transaction, OrderError>;
    async fn generate_claim_transaction(&self, order: &Order, fill_proof: &FillProof) -> Result<Transaction, OrderError>;
}

// Solver's execution strategy
#[async_trait]
pub trait ExecutionStrategy: Send + Sync {
    async fn should_execute(&self, order: &Order, context: &ExecutionContext) -> ExecutionDecision;
}

pub struct OrderService {
    implementations: HashMap<String, Box<dyn OrderInterface>>,
    strategy: Box<dyn ExecutionStrategy>,
}

impl OrderService {
    pub fn new(
        implementations: HashMap<String, Box<dyn OrderInterface>>,
        strategy: Box<dyn ExecutionStrategy>
    ) -> Self {
        Self { implementations, strategy }
    }

    pub async fn validate_intent(&self, intent: &Intent) -> Result<Order, OrderError> {
        let implementation = self.implementations
            .get(&intent.standard)
            .ok_or_else(|| OrderError::ValidationFailed("Unknown standard".into()))?;

        implementation.validate_intent(intent).await
    }

    pub async fn should_execute(&self, order: &Order, context: &ExecutionContext) -> ExecutionDecision {
        self.strategy.should_execute(order, context).await
    }

    pub async fn generate_fill_transaction(&self, order: &Order, params: &ExecutionParams) -> Result<Transaction, OrderError> {
        let implementation = self.implementations
            .get(&order.standard)
            .ok_or_else(|| OrderError::ValidationFailed("Unknown standard".into()))?;

        implementation.generate_fill_transaction(order, params).await
    }

    pub async fn generate_claim_transaction(&self, order: &Order, proof: &FillProof) -> Result<Transaction, OrderError> {
        let implementation = self.implementations
            .get(&order.standard)
            .ok_or_else(|| OrderError::ValidationFailed("Unknown standard".into()))?;

        implementation.generate_claim_transaction(order, proof).await
    }
}

// Example strategies
pub struct AlwaysExecuteStrategy;

#[async_trait]
impl ExecutionStrategy for AlwaysExecuteStrategy {
    async fn should_execute(&self, _order: &Order, context: &ExecutionContext) -> ExecutionDecision {
        ExecutionDecision::Execute(ExecutionParams {
            gas_price: context.gas_price,
            priority_fee: None,
        })
    }
}

pub struct LimitOrderStrategy {
    min_profit_bps: u32,
    max_gas_price: U256,
}

#[async_trait]
impl ExecutionStrategy for LimitOrderStrategy {
    async fn should_execute(&self, order: &Order, context: &ExecutionContext) -> ExecutionDecision {
        // Check gas price limit
        if context.gas_price > self.max_gas_price {
            return ExecutionDecision::Defer(std::time::Duration::from_secs(60));
        }

        // In reality, would calculate actual profit
        // For now, just execute
        ExecutionDecision::Execute(ExecutionParams {
            gas_price: context.gas_price,
            priority_fee: None,
        })
    }
}

use solver_delivery::TransactionHash;
```

### 7. Solver Settlement

**File: `crates/solver-settlement/src/lib.rs`**

```rust
use async_trait::async_trait;
use solver_order::{Order, FillProof};
use solver_delivery::TransactionHash;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettlementError {
    #[error("Monitoring failed: {0}")]
    MonitoringFailed(String),
    #[error("Invalid proof")]
    InvalidProof,
}

#[async_trait]
pub trait SettlementInterface: Send + Sync {
    async fn monitor_fill(&self, order: &Order, tx_hash: &TransactionHash) -> Result<FillProof, SettlementError>;
    async fn can_claim(&self, order: &Order, fill_proof: &FillProof) -> bool;
}

pub struct SettlementService {
    implementations: HashMap<String, Box<dyn SettlementInterface>>,
}

impl SettlementService {
    pub fn new(implementations: HashMap<String, Box<dyn SettlementInterface>>) -> Self {
        Self { implementations }
    }

    pub async fn monitor_fill(&self, order: &Order, tx_hash: &TransactionHash) -> Result<FillProof, SettlementError> {
        let implementation = self.implementations
            .get(&order.standard)
            .ok_or_else(|| SettlementError::MonitoringFailed("Unknown standard".into()))?;

        implementation.monitor_fill(order, tx_hash).await
    }

    pub async fn can_claim(&self, order: &Order, fill_proof: &FillProof) -> bool {
        if let Some(implementation) = self.implementations.get(&order.standard) {
            implementation.can_claim(order, fill_proof).await
        } else {
            false
        }
    }
}
```

### 8. Event Bus

**File: `crates/solver-core/src/event_bus.rs`**

```rust
use tokio::sync::broadcast;
use serde::{Serialize, Deserialize};
use solver_discovery::Intent;
use solver_order::{Order, ExecutionParams, FillProof};
use solver_delivery::{TransactionHash, TransactionReceipt};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SolverEvent {
    Discovery(DiscoveryEvent),
    Order(OrderEvent),
    Delivery(DeliveryEvent),
    Settlement(SettlementEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryEvent {
    IntentDiscovered {
        intent: Intent
    },
    IntentValidated {
        intent_id: String,
        order: Order
    },
    IntentRejected {
        intent_id: String,
        reason: String
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderEvent {
    Executing {
        order: Order,
        params: ExecutionParams
    },
    Skipped {
        order_id: String,
        reason: String
    },
    Deferred {
        order_id: String,
        retry_after: Duration
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryEvent {
    TransactionPending {
        order_id: String,
        tx_hash: TransactionHash,
        tx_type: TransactionType,
    },
    TransactionConfirmed {
        tx_hash: TransactionHash,
        receipt: TransactionReceipt
    },
    TransactionFailed {
        tx_hash: TransactionHash,
        error: String
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SettlementEvent {
    FillDetected {
        order_id: String,
        tx_hash: TransactionHash
    },
    ProofReady {
        order_id: String,
        proof: FillProof
    },
    ClaimReady {
        order_id: String
    },
    Completed {
        order_id: String
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Fill,
    Claim,
}

pub struct EventBus {
    sender: broadcast::Sender<SolverEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SolverEvent> {
        self.sender.subscribe()
    }

    pub fn publish(&self, event: SolverEvent) -> Result<(), broadcast::error::SendError<SolverEvent>> {
        self.sender.send(event)?;
        Ok(())
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}
```

### 9. Solver Core

**File: `crates/solver-core/src/lib.rs`**

```rust
use solver_config::Config;
use solver_storage::StorageService;
use solver_delivery::DeliveryService;
use solver_account::AccountService;
use solver_discovery::{DiscoveryService, Intent};
use solver_order::{OrderService, Order, ExecutionContext, ExecutionDecision};
use solver_settlement::SettlementService;
use std::sync::Arc;
use std::collections::HashMap;
use thiserror::Error;
use tokio::sync::mpsc;

pub mod event_bus;
use event_bus::{EventBus, SolverEvent, DiscoveryEvent, OrderEvent, DeliveryEvent, SettlementEvent, TransactionType};

#[derive(Debug, Error)]
pub enum SolverError {
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Service error: {0}")]
    Service(String),
}

pub struct SolverEngine {
    config: Config,
    storage: Arc<StorageService>,
    delivery: Arc<DeliveryService>,
    discovery: Arc<DiscoveryService>,
    order: Arc<OrderService>,
    settlement: Arc<SettlementService>,
    event_bus: EventBus,
}

impl SolverEngine {
    pub async fn run(&self) -> Result<(), SolverError> {
        // Start discovery monitoring
        let (intent_tx, mut intent_rx) = mpsc::unbounded_channel();
        self.discovery.start_all(intent_tx).await
            .map_err(|e| SolverError::Service(e.to_string()))?;

        // Subscribe to events
        let mut event_receiver = self.event_bus.subscribe();

        // Batch claim processing
        let mut claim_batch = Vec::new();

        loop {
            tokio::select! {
                // Handle discovered intents
                Some(intent) = intent_rx.recv() => {
                    self.handle_intent(intent).await?;
                }

                // Handle events
                Ok(event) = event_receiver.recv() => {
                    match event {
                        SolverEvent::Order(OrderEvent::Executing { order, params }) => {
                            self.handle_order_execution(order, params).await?;
                        }

                        SolverEvent::Delivery(DeliveryEvent::TransactionConfirmed { tx_hash, receipt }) => {
                            self.handle_transaction_confirmed(tx_hash, receipt).await?;
                        }

                        SolverEvent::Settlement(SettlementEvent::ClaimReady { order_id }) => {
                            claim_batch.push(order_id);

                            // Batch claims for efficiency
                            if claim_batch.len() >= 10 {
                                self.process_claim_batch(&mut claim_batch).await?;
                            }
                        }

                        _ => {} // Handle other events as needed
                    }
                }

                // Shutdown signal
                _ = tokio::signal::ctrl_c() => {
                    log::info!("Shutting down solver");
                    break;
                }
            }
        }

        // Cleanup
        self.discovery.stop_all().await
            .map_err(|e| SolverError::Service(e.to_string()))?;

        Ok(())
    }

    async fn handle_intent(&self, intent: Intent) -> Result<(), SolverError> {
        // Validate intent
        match self.order.validate_intent(&intent).await {
            Ok(order) => {
                self.event_bus.publish(SolverEvent::Discovery(DiscoveryEvent::IntentValidated {
                    intent_id: intent.id.clone(),
                    order: order.clone(),
                })).ok();

                // Store order
                self.storage.store("orders", &order.id, &order).await
                    .map_err(|e| SolverError::Service(e.to_string()))?;

                // Check execution strategy
                let context = self.build_execution_context().await?;
                match self.order.should_execute(&order, &context).await {
                    ExecutionDecision::Execute(params) => {
                        self.event_bus.publish(SolverEvent::Order(OrderEvent::Executing {
                            order,
                            params,
                        })).ok();
                    }
                    ExecutionDecision::Skip(reason) => {
                        self.event_bus.publish(SolverEvent::Order(OrderEvent::Skipped {
                            order_id: order.id,
                            reason,
                        })).ok();
                    }
                    ExecutionDecision::Defer(duration) => {
                        self.event_bus.publish(SolverEvent::Order(OrderEvent::Deferred {
                            order_id: order.id,
                            retry_after: duration,
                        })).ok();
                    }
                }
            }
            Err(e) => {
                self.event_bus.publish(SolverEvent::Discovery(DiscoveryEvent::IntentRejected {
                    intent_id: intent.id,
                    reason: e.to_string(),
                })).ok();
            }
        }

        Ok(())
    }

    async fn handle_order_execution(&self, order: Order, params: solver_order::ExecutionParams) -> Result<(), SolverError> {
        // Generate fill transaction
        let tx = self.order.generate_fill_transaction(&order, &params).await
            .map_err(|e| SolverError::Service(e.to_string()))?;

        // Submit transaction
        let tx_hash = self.delivery.deliver(tx).await
            .map_err(|e| SolverError::Service(e.to_string()))?;

        self.event_bus.publish(SolverEvent::Delivery(DeliveryEvent::TransactionPending {
            order_id: order.id.clone(),
            tx_hash: tx_hash.clone(),
            tx_type: TransactionType::Fill,
        })).ok();

        // Store fill transaction
        self.storage.store("fills", &order.id, &tx_hash).await
            .map_err(|e| SolverError::Service(e.to_string()))?;

        Ok(())
    }

    async fn handle_transaction_confirmed(&self, tx_hash: solver_delivery::TransactionHash, receipt: solver_delivery::TransactionReceipt) -> Result<(), SolverError> {
        // Find associated order
        // In production, would have proper tx->order mapping

        Ok(())
    }

    async fn process_claim_batch(&self, batch: &mut Vec<String>) -> Result<(), SolverError> {
        for order_id in batch.drain(..) {
            // Retrieve order and proof
            let order: Order = self.storage.retrieve("orders", &order_id).await
                .map_err(|e| SolverError::Service(e.to_string()))?;

            // Generate claim transaction
            // ... implementation
        }
        Ok(())
    }

    async fn build_execution_context(&self) -> Result<ExecutionContext, SolverError> {
        // In production, would fetch real data
        Ok(ExecutionContext {
            gas_price: solver_order::U256([20, 0, 0, 0]), // 20 gwei
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            solver_balance: HashMap::new(),
        })
    }

    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }
}

// Factory pattern for creating services from config
pub struct SolverBuilder {
    config: Config,
    storage_factory: Option<Box<dyn Fn(&toml::Value) -> Box<dyn solver_storage::StorageInterface> + Send>>,
    account_factory: Option<Box<dyn Fn(&toml::Value) -> Box<dyn solver_account::AccountInterface> + Send>>,
    delivery_factories: HashMap<String, Box<dyn Fn(&toml::Value) -> Box<dyn solver_delivery::DeliveryInterface> + Send>>,
    discovery_factories: HashMap<String, Box<dyn Fn(&toml::Value) -> Box<dyn solver_discovery::DiscoveryInterface> + Send>>,
    order_factories: HashMap<String, Box<dyn Fn(&toml::Value) -> Box<dyn solver_order::OrderInterface> + Send>>,
    settlement_factories: HashMap<String, Box<dyn Fn(&toml::Value) -> Box<dyn solver_settlement::SettlementInterface> + Send>>,
    strategy_factory: Option<Box<dyn Fn(&toml::Value) -> Box<dyn solver_order::ExecutionStrategy> + Send>>,
}

impl SolverBuilder {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            storage_factory: None,
            account_factory: None,
            delivery_factories: HashMap::new(),
            discovery_factories: HashMap::new(),
            order_factories: HashMap::new(),
            settlement_factories: HashMap::new(),
            strategy_factory: None,
        }
    }

    pub fn with_storage_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn(&toml::Value) -> Box<dyn solver_storage::StorageInterface> + Send + 'static,
    {
        self.storage_factory = Some(Box::new(factory));
        self
    }

    pub fn with_account_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn(&toml::Value) -> Box<dyn solver_account::AccountInterface> + Send + 'static,
    {
        self.account_factory = Some(Box::new(factory));
        self
    }

    pub fn with_delivery_factory<F>(mut self, name: &str, factory: F) -> Self
    where
        F: Fn(&toml::Value) -> Box<dyn solver_delivery::DeliveryInterface> + Send + 'static,
    {
        self.delivery_factories.insert(name.to_string(), Box::new(factory));
        self
    }

    pub fn with_discovery_factory<F>(mut self, name: &str, factory: F) -> Self
    where
        F: Fn(&toml::Value) -> Box<dyn solver_discovery::DiscoveryInterface> + Send + 'static,
    {
        self.discovery_factories.insert(name.to_string(), Box::new(factory));
        self
    }

    pub fn with_order_factory<F>(mut self, name: &str, factory: F) -> Self
    where
        F: Fn(&toml::Value) -> Box<dyn solver_order::OrderInterface> + Send + 'static,
    {
        self.order_factories.insert(name.to_string(), Box::new(factory));
        self
    }

    pub fn with_settlement_factory<F>(mut self, name: &str, factory: F) -> Self
    where
        F: Fn(&toml::Value) -> Box<dyn solver_settlement::SettlementInterface> + Send + 'static,
    {
        self.settlement_factories.insert(name.to_string(), Box::new(factory));
        self
    }

    pub fn with_strategy_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn(&toml::Value) -> Box<dyn solver_order::ExecutionStrategy> + Send + 'static,
    {
        self.strategy_factory = Some(Box::new(factory));
        self
    }

    pub fn build(self) -> Result<SolverEngine, SolverError> {
        // Create storage backend
        let storage_backend = self.storage_factory
            .ok_or_else(|| SolverError::Config("Storage factory not provided".into()))?
            (&self.config.storage.config);
        let storage = Arc::new(StorageService::new(storage_backend));

        // Create account provider
        let account_provider = self.account_factory
            .ok_or_else(|| SolverError::Config("Account factory not provided".into()))?
            (&self.config.account.config);
        let account = Arc::new(AccountService::new(account_provider));

        // Create delivery providers
        let mut delivery_providers = Vec::new();
        for (name, config) in &self.config.delivery.providers {
            if let Some(factory) = self.delivery_factories.get(name) {
                delivery_providers.push(factory(config));
            }
        }

        if delivery_providers.is_empty() {
            return Err(SolverError::Config("No delivery providers configured".into()));
        }

        let delivery = Arc::new(DeliveryService::new(delivery_providers, account.clone()));

        // Create discovery sources
        let mut discovery_sources = Vec::new();
        for (name, config) in &self.config.discovery.sources {
            if let Some(factory) = self.discovery_factories.get(name) {
                discovery_sources.push(factory(config));
            }
        }

        let discovery = Arc::new(DiscoveryService::new(discovery_sources));

        // Create order implementations
        let mut order_impls = HashMap::new();
        for (name, config) in &self.config.order.implementations {
            if let Some(factory) = self.order_factories.get(name) {
                order_impls.insert(name.clone(), factory(config));
            }
        }

        // Create execution strategy
        let strategy = self.strategy_factory
            .ok_or_else(|| SolverError::Config("Strategy factory not provided".into()))?
            (&self.config.order.execution_strategy.config);

        let order = Arc::new(OrderService::new(order_impls, strategy));

        // Create settlement implementations
        let mut settlement_impls = HashMap::new();
        for (name, config) in &self.config.settlement.implementations {
            if let Some(factory) = self.settlement_factories.get(name) {
                settlement_impls.insert(name.clone(), factory(config));
            }
        }

        let settlement = Arc::new(SettlementService::new(settlement_impls));

        Ok(SolverEngine {
            config: self.config,
            storage,
            delivery,
            discovery,
            order,
            settlement,
            event_bus: EventBus::new(1000),
        })
    }
}
```

## Usage Example

```rust
// Configuration file: solver.toml
/*
[solver]
id = "solver-001"

[storage]
backend = "memory"
[storage.config]

[account]
provider = "local"
[account.config]
private_key = "0x..."

[delivery.providers.primary]
rpc_url = "https://..."
chain_id = 1

[discovery.sources.erc7683_mainnet]
standard = "erc7683"
chain_id = 1
contract_address = "0x..."

[order.implementations.erc7683]
input_settler = "0x..."
output_settler = "0x..."

[order.execution_strategy]
strategy_type = "limit_order"
[order.execution_strategy.config]
min_profit_bps = 10
max_gas_price = "100000000000"

[settlement.implementations.erc7683]
oracle_endpoint = "https://..."
*/

// Main application
use solver_config::Config;
use solver_core::SolverBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = Config::from_file("solver.toml")?;

    // Build solver with factories
    let solver = SolverBuilder::new(config)
        .with_storage_factory(|config| {
            create_storage_backend(config)
        })
        .with_account_factory(|config| {
            create_account_provider(config)
        })
        .with_delivery_factory("primary", |config| {
            create_rpc_delivery(config)
        })
        .with_discovery_factory("erc7683_mainnet", |config| {
            create_erc7683_discovery(config)
        })
        .with_order_factory("erc7683", |config| {
            create_erc7683_order(config)
        })
        .with_settlement_factory("erc7683", |config| {
            create_erc7683_settlement(config)
        })
        .with_strategy_factory(|config| {
            match config.get("strategy_type").and_then(|v| v.as_str()) {
                Some("always") => Box::new(AlwaysExecuteStrategy),
                Some("limit_order") => Box::new(create_limit_order_strategy(config)),
                _ => Box::new(AlwaysExecuteStrategy),
            }
        })
        .build()?;

    // Run solver
    solver.run().await?;

    Ok(())
}
```

## Key Design Decisions

1. **Interface-based**: All components expose traits, no concrete implementations
2. **Event-driven**: Communication via typed events grouped by domain
3. **Protocol-agnostic**: No hardcoded protocol logic or match statements
4. **Factory pattern**: Services created via factories registered at startup
5. **Clear separation**: Discovery → Order → Delivery → Settlement flow
6. **Extensible**: Easy to add auction support or new standards
7. **Batch-friendly**: Events enable batch processing (e.g., claim batching)

## Future Extensions

1. **Auction Support**: Add `AuctionInterface` between Discovery and Order
2. **Multi-standard**: Each service supports multiple implementations
3. **Advanced Strategies**: Composite strategies, ML-based decisions
4. **Monitoring**: Metrics and health checks via event subscriptions
5. **State Recovery**: Persist events for restart/recovery
