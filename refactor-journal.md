# OIF Solver Refactoring Journal

## Overview
This document tracks the progress, decisions, concerns, and placeholders during the refactoring from a generic plugin-based architecture to a cleaner domain-based architecture.

## Key Changes from Original Guide
1. ~**solver-state renamed to solver-storage** - Better reflects its write-only nature~ (Reverted)
2. ~**Event-based data flow** - Events now carry ALL necessary data, no reading from storage~ (Reverted)
3. ~**New solver-account module** - Abstracts key management for different providers~ (Removed)

**Note**: Continuing with the original refactoring guide architecture

## Progress Log

### 2025-01-24: Initial Setup

#### Created Directory Structure
- Set up new crate structure following domain-based organization
- Created directories for:
  - Core services: solver-core, solver-types, solver-config, solver-storage, solver-service, solver-account
  - Service implementations: core-delivery, core-discovery, core-order, core-settlement
  - Domain implementations: delivery/, discovery/, order/, settlement/

#### Key Architectural Decisions
1. **Event-Driven Architecture**: All services communicate via events that carry complete data
2. **Write-Only Storage**: solver-storage only supports write/update operations, no reads
3. **Account Abstraction**: New solver-account module to handle key management across different providers

#### Completed Components
1. **solver-types**: All core interfaces and event definitions with storage interface
2. **solver-config**: Configuration management with validation
3. **core-storage**: Storage orchestration service following the same pattern as other core services
4. **core-delivery**: Delivery orchestration with strategy pattern
5. **storage-memory**: In-memory storage implementation

## Implementation Notes

### CoreEvent Structure
The CoreEvent enum now includes all necessary data in each variant:
```rust
pub enum CoreEvent {
    IntentDiscovered {
        intent: IntentDiscovered,
    },
    OrderValidated {
        order: Order,
        expected_profit: U256,
        processor: String,
    },
    TransactionSubmitted {
        order: Order,  // Full order, not just ID
        tx_hash: H256,
        tx_type: TxType,
    },
    TransactionConfirmed {
        order: Order,  // Full order
        receipt: TransactionReceipt,
        tx_type: TxType,
    },
    SettlementReady {
        order: Order,  // Full order
        fill_tx_hash: H256,
        claim_data: Vec<u8>,
    },
}
```

### Account Interface Design
The AccountInterface trait provides abstraction over different key management solutions:
- LocalAccount: For development/testing with local wallets
- HashicorpVaultAccount: For production with Vault integration
- AwsKmsAccount: For AWS KMS integration

## Concerns & Placeholders

### Current Placeholders
1. **Account Implementations**: LocalAccount implemented, others (Vault, KMS) are placeholders
2. **Metrics Collection**: Basic structure in place, needs full implementation
3. **Health Checks**: Interface defined but implementation is minimal
4. **Price Oracle Integration**: Placeholder in profit calculation
5. **Core Services**: Interfaces defined, implementations pending

### Technical Concerns
1. **Event Bus Performance**: Need to monitor performance with high event volume
2. **Storage Consistency**: Write-only pattern may need careful error handling
3. **Account Service Integration**: Need to ensure proper error handling for key operations

## Next Steps
1. Complete core type definitions with updated event structure
2. Implement solver-storage with write-only interface
3. Create solver-account module with LocalAccount implementation
4. Update all core services to use event-based data flow

## Testing Strategy
- Unit tests for each module
- Integration tests for event flow
- Mock implementations for external dependencies

## Migration Notes
- Old plugin system completely removed
- All implementations now use domain-specific interfaces
- Registry pattern for dynamic loading maintained but simplified