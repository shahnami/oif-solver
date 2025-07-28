# OIF Solver Overview

## Summary

The OIF Solver is a high-performance cross-chain order execution system designed for the Open Intents Framework. It discovers intents across multiple blockchains, validates and executes orders through optimal paths, and manages settlement processes. The solver operates as an autonomous service that monitors blockchain events, makes intelligent execution decisions, and ensures reliable transaction delivery across heterogeneous blockchain networks.

## High-Level Project Structure

The OIF Solver uses Rust for its performance-critical requirements in cross-chain transaction execution. Rust provides zero-cost abstractions for the modular architecture, compile-time guarantees for concurrent blockchain monitoring across multiple chains, and predictable latency without garbage collection pauses, essential when competing to execute orders. The memory safety guarantees are crucial when handling cryptographic operations simultaneously.

```
oif-solver/
├── Cargo.toml                   # Workspace definition
├── crates/                      # Modular components
│   ├── solver-account/          # Cryptographic operations
│   ├── solver-config/           # Configuration management
│   ├── solver-core/             # Orchestration engine
│   ├── solver-delivery/         # Transaction submission
│   ├── solver-discovery/        # Intent monitoring
│   ├── solver-order/            # Order processing
│   ├── solver-service/          # Main executable
│   ├── solver-settlement/       # Settlement verification
│   ├── solver-storage/          # State persistence
│   └── solver-types/            # Shared types
├── config/                      # Configuration examples
└── scripts/                     # Deployment and demo scripts
```

## Directory Responsibilities

### Core Infrastructure

- **solver-types**: Common data structures and trait definitions shared across all modules
- **solver-config**: TOML-based configuration parsing and validation
- **solver-storage**: Persistent state management with custom backends
- **solver-account**: Secure key management and transaction signing

### Service Layer

- **solver-discovery**: Multi-chain event monitoring and intent detection
- **solver-order**: Intent validation, strategy evaluation, and transaction generation
- **solver-delivery**: Reliable transaction submission and confirmation monitoring
- **solver-settlement**: Fill validation and claim transaction management

### Orchestration

- **solver-core**: Event-driven orchestration of the entire order lifecycle
- **solver-service**: Binary entry point that wires up all components

## High-Level System Flow

```mermaid
sequenceDiagram
    participant User as User/DApp
    participant Origin as Origin Chain
    participant Discovery as Discovery Service
    participant Core as Core Engine
    participant Order as Order Service
    participant Delivery as Delivery Service
    participant Destination as Destination Chain
    participant Settlement as Settlement Service

    User->>Origin: Submit Intent
    Origin->>Discovery: Emit Intent Event
    Discovery->>Core: New Intent Detected
    Core->>Order: Validate & Strategy Check
    Order->>Core: Execute Decision
    Core->>Delivery: Submit Fill Transaction
    Delivery->>Destination: Execute Fill
    Destination->>Delivery: Transaction Confirmed
    Delivery->>Core: Fill Complete
    Core->>Settlement: Validate Fill
    Settlement->>Core: Ready to Claim
    Core->>Delivery: Submit Claim
    Delivery->>Origin: Claim Rewards
```

## Module Deep Dive

### solver-discovery

Monitors multiple chains simultaneously for new intent events.

```mermaid
sequenceDiagram
    participant Chain as Blockchain
    participant Discovery as DiscoveryService
    participant Core as Core Engine

    loop Event Monitoring
        Discovery->>Chain: Poll for Events
        Chain->>Discovery: Return New Events
        Discovery->>Discovery: Filter & Validate
        Discovery->>Core: Push Valid Intents
    end
```

### solver-order

Processes intents through validation, strategy evaluation, and transaction generation.

```mermaid
sequenceDiagram
    participant Core as Core Engine
    participant Order as OrderService
    participant Strategy as Execution Strategy
    participant Implementation as Order Implementation

    Core->>Order: validate_intent(intent)
    Order->>Implementation: Parse & Validate
    Implementation->>Order: Return Order
    Core->>Order: should_execute(order)
    Order->>Strategy: Evaluate Conditions
    Strategy->>Order: Execution Decision
    Core->>Order: generate_fill_transaction()
    Order->>Implementation: Build Transaction
    Implementation->>Order: Return Transaction
```

### solver-delivery

Handles reliable transaction submission with monitoring and retry logic.

```mermaid
sequenceDiagram
    participant Core as Core Engine
    participant Delivery as DeliveryService
    participant Provider as Chain Provider
    participant Chain as Blockchain

    Core->>Delivery: deliver(transaction)
    Delivery->>Provider: Submit Transaction
    Provider->>Chain: Broadcast
    Chain->>Provider: Return TX Hash
    Provider->>Delivery: Return Hash
    Delivery->>Core: Transaction Submitted

    loop Monitor Confirmation
        Delivery->>Provider: Check Status
        Provider->>Chain: Query Receipt
        Chain->>Provider: Return Status
    end

    Delivery->>Core: Transaction Confirmed
```

### solver-settlement

Validates fills and manages the claim process for completed orders.

```mermaid
sequenceDiagram
    participant Core as Core Engine
    participant Settlement as SettlementService
    participant Chain as Blockchain

    Core->>Settlement: get_attestation(order, tx_hash)
    Settlement->>Chain: Get Transaction Receipt
    Settlement->>Settlement: Extract Fill Proof
    Settlement->>Core: Return Proof

    loop Check Claim Readiness
        Core->>Settlement: can_claim(order, proof)
        Settlement->>Chain: Check Conditions
        Settlement->>Core: Return Status
    end

    Core->>Settlement: Ready to Claim
```

### solver-core

Orchestrates the entire solver workflow through event-driven architecture.

```mermaid
sequenceDiagram
    participant Discovery as Discovery
    participant Core as Core Engine
    participant EventBus as Event Bus
    participant Services as Other Services

    Discovery->>Core: Intent Discovered
    Core->>EventBus: Publish Event
    EventBus->>Services: Broadcast Event
    Services->>EventBus: Publish Response Events
    EventBus->>Core: Deliver Events
    Core->>Core: Process & Coordinate
```

## Conclusion

The OIF Solver represents a robust, performant solution for cross-chain order execution. Its modular architecture allows for easy extension and maintenance, while Rust's performance characteristics ensure it can compete effectively in the MEV-competitive environment of cross-chain execution. Each module can be used independently, making the solver both a complete solution and a toolkit for building custom cross-chain infrastructure.

The event-driven architecture ensures responsive processing of intents, while the clear separation of concerns makes the system easy to understand and extend. Whether used as a complete solver or as individual components, the OIF Solver provides the building blocks for sophisticated cross-chain execution strategies.
