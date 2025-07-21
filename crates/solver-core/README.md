# Solver Core - Orchestrator Engine

The orchestrator engine is the central coordination component of the OIF solver. After refactoring, it now works directly with services instead of managing plugins itself, which simplifies the architecture.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      Orchestrator                           │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                Lifecycle Manager                     │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                Strategy Manager                      │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                Event Processor                       │   │
│  └─────────────────────────────────────────────────────┘   │
│  ┌─────────────────────────────────────────────────────┐   │
│  │                Metrics Collector                     │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
┌───────▼────────┐   ┌────────▼────────┐   ┌───────▼────────┐
│   Discovery    │   │    Delivery     │   │     State      │
│    Service     │   │    Service      │   │    Service     │
├────────────────┤   ├─────────────────┤   ├────────────────┤
│ Manages own    │   │ Manages own     │   │ Manages own    │
│ plugins        │   │ plugins         │   │ plugins        │
└────────────────┘   └─────────────────┘   └────────────────┘
```

## Key Benefits of the Refactored Architecture

1. **Separation of Concerns**: Each service manages its own plugins internally
2. **Simplified Orchestration**: The orchestrator focuses on coordination, not plugin management
3. **Better Encapsulation**: Plugin details are hidden within their respective services
4. **Easier Testing**: Services can be tested independently
5. **Cleaner Dependencies**: No central plugin manager means fewer circular dependencies

## Usage Example

```rust
use solver_core::{Orchestrator, OrchestratorBuilder};
use solver_config::ConfigLoader;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load configuration
    let config = ConfigLoader::new()
        .with_file("config.toml")
        .load()
        .await?;
    
    // Build orchestrator - services are created internally
    let orchestrator = Orchestrator::builder()
        .with_config(config)
        .build()
        .await?;
    
    // Start the orchestrator
    orchestrator.start().await?;
    
    // The orchestrator now coordinates:
    // - Discovery service for finding orders
    // - Delivery service for submitting transactions  
    // - State service for storing order data
    // - Strategy manager for execution decisions
    // - Event processor for order processing pipeline
    
    // Monitor health
    let health = orchestrator.health_check().await?;
    println!("Orchestrator health: {:?}", health.status);
    
    // Get metrics
    let metrics = orchestrator.get_metrics().await;
    println!("Orders processed: {}", metrics.orders_processed);
    
    // Graceful shutdown
    orchestrator.stop().await?;
    
    Ok(())
}
```

## Service Interaction

Each service handles its own plugin lifecycle:

### Discovery Service
- Loads and manages discovery plugins based on configuration
- Provides unified interface for order discovery
- Handles plugin health checks and metrics internally

### Delivery Service  
- Manages delivery plugins for different chains
- Implements delivery strategies (fastest, cheapest, redundant)
- Handles transaction submission and monitoring

### State Service
- Manages storage plugins (memory, file, database)
- Provides key-value interface for order and metadata storage
- Handles persistence and caching

## Strategy Management

The Strategy Manager now works directly with services:

```rust
// Strategies delegate to services instead of managing plugins
impl DeliveryStrategy for FastestDeliveryStrategy {
    async fn execute(
        &self,
        request: DeliveryRequest,
        delivery_service: &DeliveryService,
        chain_id: ChainId,
    ) -> Result<DeliveryReceipt, StrategyError> {
        // Service handles plugin selection internally
        delivery_service.deliver_fastest(request).await
            .map_err(|e| StrategyError::ServiceError(format!("Delivery failed: {}", e)))
    }
}
```

This approach keeps the orchestrator focused on high-level coordination while services handle the details of plugin management.
