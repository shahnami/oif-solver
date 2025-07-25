# Solver Settlement - Settlement Orchestration Service

The `solver-settlement` crate provides a plugin-based orchestration service that monitors fills and determines when they are ready for settlement. It manages oracle attestations, claim windows, and settlement conditions, emitting events when settlements can proceed.

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                    SETTLEMENT ORCHESTRATION SERVICE                      │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Core Components                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  Plugin     │  │  Monitored   │  │     Event              │  │  │
│  │  │  Registry   │  │   Fills      │  │     Emitter            │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                    Orchestration Flow                              │  │
│  │  ┌────────┐  ┌──────────┐  ┌───────────┐  ┌─────────────────┐  │  │
│  │  │Monitor │→ │  Check   │→ │  Verify   │→ │     Emit        │  │  │
│  │  │ Fill   │  │ Oracle   │  │Conditions │  │ Ready Event     │  │  │
│  │  └────────┘  └──────────┘  └───────────┘  └─────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
                        ┌───────────────────────┐
                        │ SettlementReadyEvent  │
                        └───────────────────────┘
                                    │
                                    ▼
                        ┌───────────────────────┐
                        │   Delivery Service    │
                        │ (Executes Settlement) │
                        └───────────────────────┘
```

## Module Structure

```
solver-settlement/
├── src/
│   └── lib.rs          # Service implementation and plugin orchestration
├── Cargo.toml          # Dependencies
└── README.md           # This file
```

## Key Components

### 1. **SettlementService** (`lib.rs`)

The main orchestration service that monitors fills and determines when settlements are ready.

**Key Responsibilities:**

- Monitor confirmed fills for settlement readiness
- Check oracle attestations for fills
- Verify claim windows and timing constraints
- Emit events when settlements can proceed
- Manage settlement plugins

**Internal Structure:**

```rust
pub struct SettlementService {
    // Thread-safe plugin registry
    settlement_plugins: Arc<RwLock<HashMap<String, Arc<dyn SettlementPlugin>>>>,
    
    // Configuration
    config: SettlementConfig,
    
    // Fills being monitored for settlement
    monitored_fills: Arc<RwLock<HashMap<String, MonitoredFill>>>,
    
    // Active disputes tracking
    active_disputes: Arc<RwLock<HashMap<String, DisputeTracker>>>,
    
    // Event sink for emitting settlement ready events
    event_sink: Option<EventSink<Event>>,
}
```

### 2. **MonitoredFill**

Tracks fills being monitored for settlement readiness:

```rust
pub struct MonitoredFill {
    pub fill_event: FillEvent,
    pub fill_data: FillData,
    pub order_type: String,
    pub plugin_name: String,
    pub last_check: u64,
    pub attestation_status: Option<AttestationStatus>,
    pub claim_window: Option<ClaimWindow>,
    pub readiness: Option<SettlementReadiness>,
}
```

### 3. **SettlementReadyEvent**

Event emitted when a fill is ready for settlement:

```rust
pub struct SettlementReadyEvent {
    pub fill_event: FillEvent,
    pub settlement_type: SettlementType,
    pub oracle_attestation_id: Option<String>,
    pub claim_window_start: Timestamp,
    pub claim_window_end: Timestamp,
    pub metadata: HashMap<String, String>,
}
```

### 4. **Key Data Types**

```rust
// Oracle attestation status
pub struct AttestationStatus {
    pub is_attested: bool,
    pub attestation_id: Option<String>,
    pub oracle_address: Option<Address>,
    pub attestation_time: Option<Timestamp>,
    pub dispute_period_end: Option<Timestamp>,
    pub is_disputed: bool,
}

// Claim window information
pub struct ClaimWindow {
    pub start: Timestamp,
    pub end: Timestamp,
    pub is_active: bool,
    pub remaining_time: Option<u64>,
}

// Settlement readiness check result
pub struct SettlementReadiness {
    pub is_ready: bool,
    pub reasons: Vec<String>,
    pub oracle_status: AttestationStatus,
    pub claim_window: ClaimWindow,
    pub estimated_profit: i64,
    pub risks: Vec<SettlementRisk>,
}
```

## Settlement Orchestration Flow

```text
FillEvent (Confirmed) → Monitor Fill → Check Every 10s
                              │
                              ▼
                    ┌─────────────────────┐
                    │ Check Conditions:    │
                    │ • Oracle Attestation │
                    │ • Dispute Period     │
                    │ • Claim Window       │
                    └─────────────────────┘
                              │
                          All Ready?
                              │
                      ┌───────┴────────┐
                      │                │
                     No               Yes
                      │                │
                   Continue         Emit Event
                   Monitoring           │
                                       ▼
                              SettlementReadyEvent
                                       │
                                       ▼
                              Delivery Service
                              Executes Settlement
```

### Flow Steps:

1. **Fill Monitoring**: When a fill is confirmed, start monitoring it
2. **Oracle Check**: Verify oracle has attested to the fill
3. **Timing Verification**: Check dispute period and claim window
4. **Condition Assessment**: Verify all settlement conditions are met
5. **Event Emission**: Emit SettlementReadyEvent when ready
6. **Remove from Monitoring**: Stop monitoring once event is emitted

## Plugin System

### SettlementPlugin Interface (Refactored):

```rust
#[async_trait]
pub trait SettlementPlugin: BasePlugin {
    // Check if this plugin can handle the given chain and order type
    async fn can_handle(&self, chain_id: ChainId, order_type: &str) -> PluginResult<bool>;
    
    // Check oracle attestation status for a fill
    async fn check_oracle_attestation(&self, fill: &FillData) -> PluginResult<AttestationStatus>;
    
    // Get claim window timing for this order type
    async fn get_claim_window(&self, order_type: &str, fill: &FillData) -> PluginResult<ClaimWindow>;
    
    // Verify all settlement conditions are met
    async fn verify_settlement_conditions(&self, fill: &FillData) -> PluginResult<SettlementReadiness>;
    
    // Handle dispute or challenge if applicable
    async fn handle_dispute(&self, fill: &FillData, dispute_data: &DisputeData) -> PluginResult<DisputeResolution>;
    
    // Get settlement requirements for this strategy
    fn get_settlement_requirements(&self) -> SettlementRequirements;
    
    // Get supported settlement types
    fn supported_settlement_types(&self) -> Vec<SettlementType>;
}
```

### Configuration:

```rust
pub struct SettlementConfig {
    pub default_strategy: String,         // Default plugin to use
    pub fallback_strategies: Vec<String>, // Fallback plugin order
    pub profit_threshold_wei: String,     // Minimum profit required
}
```

## Usage Example

```rust
use solver_settlement::{SettlementService, SettlementServiceBuilder};
use solver_types::configs::SettlementConfig;
use solver_types::events::{Event, FillEvent};

// Build service with plugins and event sink
let (event_tx, mut event_rx) = mpsc::unbounded_channel();
let event_sink = EventSink::new(event_tx);

let service = SettlementServiceBuilder::new()
    .with_config(SettlementConfig {
        default_strategy: "direct_settlement".to_string(),
        fallback_strategies: vec![],
        profit_threshold_wei: "0".to_string(),
    })
    .with_event_sink(event_sink)
    .with_plugin("direct_settlement".to_string(), Box::new(direct_plugin), direct_config)
    .build()
    .await;

// Start monitoring loop
service.start_monitoring().await;

// When a fill is confirmed, monitor it
let fill_event = FillEvent {
    order_id: "order123".to_string(),
    fill_id: "fill456".to_string(),
    chain_id: 1,
    tx_hash: "0x...".to_string(),
    timestamp: 1234567890,
    status: FillStatus::Confirmed,
    source: "eip7683_onchain".to_string(),
    order_data: Some(order_bytes),
};

// Start monitoring the fill
service.monitor_fill(fill_event).await?;

// The service will automatically check conditions every 10 seconds
// When ready, it emits a SettlementReadyEvent

// Listen for settlement ready events
tokio::spawn(async move {
    while let Some(event) = event_rx.recv().await {
        match event {
            Event::SettlementReady(ready_event) => {
                println!("Settlement ready for fill: {}", ready_event.fill_event.fill_id);
                // Delivery service will handle the actual settlement execution
            }
            _ => {}
        }
    }
});
```

## Key Design Decisions

### Architecture Benefits:

1. **Separation of Concerns**: Settlement orchestration separated from transaction execution
2. **Event-Driven**: Clean event-based communication between services
3. **Plugin-Based**: Flexible support for different settlement strategies
4. **Automatic Monitoring**: Background task monitors all fills continuously
5. **Stateless Plugins**: Plugins focus on rules/conditions, not state management

### Implementation Details:

1. **10-Second Check Interval**: Configurable monitoring frequency
2. **Automatic Cleanup**: Fills removed from monitoring after event emission
3. **Optimized Checks**: Single call to `verify_settlement_conditions` gets all data
4. **Dispute Handling**: Built-in support for dispute resolution
5. **Flexible Timing**: Configurable dispute periods and claim windows

## Dependencies

### Internal Crates:

- `solver-types`: Core type definitions and plugin traits

### External Dependencies:

- `tokio`: Async runtime
- `async-trait`: Async trait support
- `futures`: Async utilities
- `tracing`: Structured logging
- `uuid`: Unique identifier generation
- `bytes`: Byte buffer handling
- `chrono`: Timestamp handling
- `thiserror`/`anyhow`: Error handling

## Runtime Behavior

### Service Lifecycle:

1. **Plugin Registration**: Plugins initialized and registered during build
2. **Start Monitoring**: Background task begins checking fills every 10 seconds
3. **Fill Reception**: Confirmed fills added to monitoring queue
4. **Condition Checking**: Each fill checked for oracle attestation and timing
5. **Event Emission**: SettlementReadyEvent emitted when conditions met
6. **Cleanup**: Fill removed from monitoring after event emission

### Monitoring Flow:

1. **Oracle Check**: Query plugin for attestation status
2. **Timing Check**: Verify dispute period and claim window
3. **Readiness Assessment**: Combine all conditions
4. **Decision**: Either continue monitoring or emit event

### Plugin Selection:

1. **Default Strategy**: Try configured default first
2. **Fallback Strategies**: Try each fallback if default can't handle
3. **Chain/Type Support**: Verify plugin supports chain and order type

## Future Improvements

1. **Configurable Check Interval**: Make monitoring interval configurable
2. **Batch Monitoring**: Check multiple fills in parallel
3. **Persistent State**: Store monitored fills for recovery after restart
4. **Advanced Dispute Handling**: More sophisticated dispute resolution
5. **Multi-Oracle Support**: Support multiple oracle sources
6. **Analytics**: Settlement readiness metrics and reporting

## Performance Considerations

- **Efficient Monitoring**: Single background task for all fills
- **Minimal Lock Time**: Quick read/write operations on monitored fills
- **No Blocking Operations**: All plugin calls are async
- **Automatic Cleanup**: No memory leaks from completed fills

## Security Considerations

- **Plugin Trust**: Plugins determine settlement readiness
- **Oracle Dependency**: Relies on oracle attestation accuracy
- **Timing Attacks**: Claim windows must be carefully configured
- **Event Ordering**: Events processed in order received

## Summary

The refactored `solver-settlement` service is now a pure orchestration service that:
- Monitors confirmed fills for settlement readiness
- Checks oracle attestations and timing constraints
- Emits events when settlements can proceed
- Delegates actual execution to the delivery service

This clean separation of concerns makes the system more maintainable and flexible.

