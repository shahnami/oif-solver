# Solver Types - Core Type Definitions and Plugin Interfaces

The `solver-types` crate serves as the foundational type system for the entire OIF solver ecosystem. It defines all shared types, configuration structures, event systems, and most importantly, the plugin interfaces that enable the solver's extensible architecture.

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                           SOLVER TYPES                                   │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                      Core Type System                              │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │   Configs   │  │    Events    │  │   Common Types         │  │  │
│  │  │  • Solver   │  │  • Discovery │  │  • ChainId             │  │  │
│  │  │  • Plugin   │  │  • Order     │  │  • Address             │  │  │
│  │  │  • Service  │  │  • Fill      │  │  • TxHash              │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Plugin Interface Layer                         │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │ BasePlugin  │  │ StatePlugin  │  │  DiscoveryPlugin       │  │  │
│  │  │  (trait)    │  │   (trait)    │  │     (trait)            │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │DeliveryPlug │  │SettlementPlug│  │   OrderPlugin          │  │  │
│  │  │  (trait)    │  │   (trait)    │  │     (trait)            │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                          ┌─────────┴─────────┐
                          │                   │
                 ┌────────▼────────┐ ┌────────▼────────┐
                 │ Concrete Plugin │ │ Concrete Plugin │
                 │ Implementation  │ │ Implementation  │
                 │ (other crates)  │ │ (other crates)  │
                 └─────────────────┘ └─────────────────┘
```

## Module Structure

```
solver-types/
├── src/
│   ├── lib.rs              # Module re-exports
│   ├── events.rs           # Event system definitions
│   ├── configs/            # Configuration structures
│   │   └── mod.rs          # Service and plugin configs
│   └── plugins/            # Plugin trait definitions
│       ├── mod.rs          # Common plugin types
│       ├── base.rs         # Base plugin interface
│       ├── state.rs        # State storage interface
│       ├── discovery.rs    # Order discovery interface
│       ├── delivery.rs     # Transaction delivery interface
│       ├── settlement.rs   # Settlement strategy interface
│       └── order.rs        # Order processing interface
├── Cargo.toml
└── README.md
```

## Key Components

### 1. **Configuration System** (`configs/`)

Defines all configuration structures for the solver and its services:

```rust
pub struct SolverConfig {
    pub solver: SolverSettings,      // Core solver settings
    pub plugins: PluginsConfig,       // Plugin configurations
    pub delivery: DeliveryConfig,     // Delivery service config
    pub settlement: SettlementConfig, // Settlement service config
    pub discovery: DiscoveryConfig,   // Discovery service config
    pub state: StateConfig,          // State service config
}
```

**Key Features:**

- Hierarchical configuration structure
- Service-specific settings
- Plugin configuration registry
- Serde-based serialization for TOML/JSON/YAML support

### 2. **Event System** (`events.rs`)

Central event bus for inter-service communication:

```rust
pub enum Event {
    Discovery(DiscoveryEvent),  // New order discovered
    OrderCreated(OrderEvent),   // Order processing started
    OrderFill(FillEvent),      // Order filled on chain
    Settlement(SettlementEvent), // Settlement transaction
    ServiceStatus(StatusEvent), // Service health updates
}
```

**Event Flow:**

```text
Discovery → OrderCreated → OrderFill → Settlement
    │            │            │            │
    └────────────┴────────────┴────────────┘
                      Event Bus
```

### 3. **Plugin System** (`plugins/`)

#### Base Plugin Interface (`base.rs`)

All plugins must implement the `BasePlugin` trait:

```rust
#[async_trait]
pub trait BasePlugin: Send + Sync + Debug {
    fn plugin_type(&self) -> &'static str;
    async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()>;
    async fn health_check(&self) -> PluginResult<PluginHealth>;
    async fn shutdown(&mut self) -> PluginResult<()>;
    // ... more methods
}
```

**Plugin Lifecycle:**

1. Creation → 2. Configuration → 3. Initialization → 4. Operation → 5. Shutdown

#### Specialized Plugin Traits

**State Plugin** - Key-value storage abstraction:

- Memory, File, Redis backends
- TTL support
- Atomic operations
- Batch operations

**Discovery Plugin** - Order discovery from various sources:

- On-chain event monitoring
- Off-chain APIs
- WebSocket streams
- Historical data sync

**Delivery Plugin** - Transaction submission strategies:

- Gas optimization
- Nonce management
- MEV protection
- Transaction replacement

**Settlement Plugin** - Settlement execution strategies:

- Direct settlement
- Optimistic settlement
- Cross-chain settlement
- Profitability validation

**Order Plugin** - Order type handling:

- Order parsing and validation
- Metadata extraction
- Fill/settlement request creation

### 4. **Common Types** (`plugins/mod.rs`)

Shared type definitions across the system:

```rust
pub type ChainId = u64;
pub type Address = String;
pub type TxHash = String;
pub type Timestamp = u64;
```

**ConfigValue** - Flexible configuration value type:

```rust
pub enum ConfigValue {
    String(String),
    Number(i64),
    Float(f64),
    Boolean(bool),
    Array(Vec<ConfigValue>),
    Object(HashMap<String, ConfigValue>),
    Null,
}
```

## Type Relationships

```text
┌─────────────────┐         ┌──────────────────┐
│  OrderEvent     │ ───────▶│  FillEvent       │
│  • order_id     │         │  • order_id      │
│  • chain_id     │         │  • fill_id       │
│  • raw_data     │         │  • tx_hash       │
└─────────────────┘         └──────────────────┘
         │                           │
         │                           │
         ▼                           ▼
┌─────────────────┐         ┌──────────────────┐
│ TransactionReq  │         │ SettlementEvent  │
│  • transaction  │         │  • settlement_id │
│  • priority     │         │  • status        │
│  • metadata     │         │  • tx_hash       │
└─────────────────┘         └──────────────────┘
```

## Critical Observations

### Strengths:

1. **Clean Abstractions**: Well-defined trait boundaries between components
2. **Type Safety**: Strong typing with minimal use of `Any` types
3. **Extensibility**: Plugin system allows easy addition of new functionality
4. **Event-Driven**: Decoupled communication via event system
5. **Configuration Flexibility**: Nested configs with sensible defaults

### Areas of Concern:

1. **String-Based IDs**: Many IDs are strings instead of typed newtype wrappers
2. **Address Type**: Using `String` for addresses instead of proper type
3. **Error Propagation**: Single `PluginError` type loses context
4. **Trait Object Overhead**: Heavy use of dynamic dispatch
5. **Missing Validation**: Limited validation in configuration types

### Potential Optimizations:

1. **Newtype Patterns**: Wrap primitive types for type safety
2. **Generic Events**: Make event system generic over event types
3. **Static Dispatch**: Use generics instead of trait objects where possible
4. **Validation Layer**: Add configuration validation schemas
5. **Error Context**: Rich error types with proper context

## Dependencies

### External Dependencies:

- `async-trait`: Async trait support (fundamental for plugin system)
- `tokio`: Async runtime and utilities
- `serde`/`serde_json`: Serialization framework
- `thiserror`/`anyhow`: Error handling
- `chrono`: Time handling
- `rust_decimal`: Precise decimal arithmetic
- `bytes`: Efficient byte buffer handling
- `uuid`: Unique identifier generation
- `hex`: Hex encoding/decoding
- `sha3`: Hashing functionality

### Dependency Concerns:

2. **Redundant Dependencies**: Both thiserror and anyhow imported
3. **Missing Alloy**: Newer Ethereum types library not used

## Runtime Behavior

### Type Usage Flow:

1. **Configuration Loading**: TOML/JSON → `SolverConfig`
2. **Plugin Creation**: `PluginConfig` → Plugin Instance
3. **Event Generation**: Plugin → `Event` → Event Bus
4. **Type Conversion**: Internal types ↔ External types

### Memory Patterns:

- Most types are `Clone` for easy sharing
- Heavy use of `Arc` for shared ownership
- `Box<dyn Trait>` for trait objects
- `HashMap` for dynamic collections

## Known Issues & Cruft

1. **Empty Order Registry**: `OrderPluginRegistry::create_plugin` returns dummy value
2. **Unused Decimal**: `rust_decimal` imported but not used
3. **ConfigValue Complexity**: Nested enum makes parsing complex
4. **Missing Derive Macros**: Some types missing useful derives (Hash, Eq)
5. **Inconsistent Defaults**: Some configs have defaults, others don't

## Future Improvements

1. **Type-Safe IDs**: Implement newtype wrappers for all ID types
2. **Proper Address Type**: Use checksummed address type from alloy
3. **Event Streaming**: Add backpressure and filtering to event system
4. **Schema Validation**: JSON Schema for configuration validation
5. **Plugin Versioning**: Version compatibility checking
6. **Metric Types**: Prometheus-compatible metric types
7. **Tracing Integration**: OpenTelemetry trace context propagation

## Performance Considerations

- **Allocation Heavy**: Many heap allocations with Box/Arc
- **Dynamic Dispatch**: Trait objects prevent inlining
- **String Operations**: Frequent string allocations for IDs
- **HashMap Lookups**: O(1) average but with overhead
- **Clone Operations**: Frequent cloning of configurations

## Security Considerations

- **Input Validation**: Limited validation on external inputs
- **Type Confusion**: String-based types allow invalid data
- **Plugin Trust**: Plugins have full access to system
- **Serialization**: Potential for malformed config files
- **Error Leakage**: Errors might expose internal state

The `solver-types` crate provides a solid foundation for the solver's type system and plugin architecture, though improvements in type safety and validation would enhance robustness and security.
