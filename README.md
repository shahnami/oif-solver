# OIF Solver

A high-performance cross-chain solver implementation for the Open Intents Framework (OIF). This solver enables efficient cross-chain order execution by discovering intents, finding optimal execution paths, and settling transactions across multiple blockchain networks.

## Overview

The OIF Solver is designed to:

- Discover and monitor cross-chain intents from multiple sources
- Find optimal execution paths across different chains and liquidity sources
- Execute transactions efficiently while minimizing costs
- Provide comprehensive monitoring and observability
- Support multiple order types and protocols (currently EIP-7683)

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────────────┐
│                                     External Services                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐  ┌────────────┐  │
│  │  Blockchain  │  │   Off-chain  │  │   Liquidity  │  │   Price     │  │  External  │  │
│  │    Nodes     │  │   Intent     │  │   Sources    │  │   Oracles   │  │  Settlers  │  │
│  │  (EVM, etc)  │  │   Sources    │  │  (DEX, AMM)  │  │  (APIs)     │  │  (Relays)  │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬──────┘  └─────┬──────┘  │
└─────────┼──────────────────┼──────────────────┼──────────────────┼──────────────┼───────┘
          │                  │                  │                  │              │
┌─────────┼──────────────────┼──────────────────┼──────────────────┼──────────────┼───────┐
│         ▼                  ▼                  ▼                  ▼              ▼       │
│  ┌─────────────────────────────────────────────────────────────────────────────────┐    │
│  │                           solver-service (HTTP API Layer)                       │    │
│  │  ┌─────────────┐  ┌──────────────┐  ┌─────────────┐  ┌───────────────────────┐  │    │
│  │  │    /api     │  │   /health    │  │  /metrics   │  │   CLI Arguments       │  │    │
│  │  │  REST API   │  │ Health Check │  │ Prometheus  │  │  --config, --log      │  │    │
│  │  └──────┬──────┘  └──────────────┘  └─────────────┘  └───────────────────────┘  │    │
│  └─────────┼───────────────────────────────────────────────────────────────────────┘    │
│            │                                                                            │
│            ▼                                                                            │
│  ┌─────────────────────────────────────────────────────────────────────────────────┐    │
│  │                        solver-core (Orchestration Engine)                       │    │
│  │  ┌────────────────┐     ┌──────────────────┐     ┌────────────────────────────┐ │    │
│  │  │ SolverEngine   │◄────┤ SolverCoordinator├────►│    OrderProcessor          │ │    │
│  │  │                │     │                  │     │ - Validate                 │ │    │
│  │  │ - Execute      │     │ - Route Orders   │     │ - Route                    │ │    │
│  │  │ - Track State  │     │ - Manage State   │     │ - Execute                  │ │    │
│  │  └───────┬────────┘     └────────┬─────────┘     └────────────┬───────────────┘ │    │
│  └──────────┼───────────────────────┼───────────────────────────┼──────────────────┘    │
│             │                       │                           │                       │
│  ┌──────────┼───────────────────────┼───────────────────────────┼──────────────────┐    │
│  │          ▼                       ▼                           ▼                  │    │
│  │  ┌────────────────┐     ┌─────────────────┐     ┌─────────────────────────┐     │    │
│  │  │solver-discovery│     │ solver-orders   │     │   solver-validators     │     │    │
│  │  ├────────────────┤     ├─────────────────┤     ├─────────────────────────┤     │    │
│  │  │ - Chain Events │     │ - Parse Orders  │     │ - Order Validation      │     │    │
│  │  │ - Off-chain    │     │ - Classify Type │     │ - Route Validation      │     │    │
│  │  │ - Stream APIs  │     │ - Registry      │     │ - Profitability Check   │     │    │
│  │  └────────────────┘     │ - EIP-7683 Impl │     └─────────────────────────┘     │    │
│  │                         └─────────────────┘                                     │    │
│  └─────────────────────────────────────────────────────────────────────────────────┘    │
│                                                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────────────┐    │
│  │                          Execution & Settlement Layer                           │    │
│  │  ┌───────────────┐     ┌─────────────────┐     ┌─────────────────────────┐      │    │
│  │  │solver-delivery│     │solver-settlement│     │   solver-strategies     │      │    │
│  │  ├───────────────┤     ├─────────────────┤     ├─────────────────────────┤      │    │
│  │  │ - Submit TX   │     │ - Claim Rewards │     │ - Route Optimization    │      │    │
│  │  │ - Track Status│     │ - Attestations  │     │ - Execution Planning    │      │    │
│  │  │ - Retry Logic │     │ - Direct/Relay  │     │ - Fallback Strategies   │      │    │
│  │  └───────┬───────┘     └─────────────────┘     └─────────────────────────┘      │    │
│  └──────────┼──────────────────────────────────────────────────────────────────────┘    │
│             │                                                                           │
│  ┌──────────┼──────────────────────────────────────────────────────────────────────┐    │
│  │          ▼                                                                      │    │
│  │  ┌───────────────┐     ┌─────────────────┐     ┌─────────────────────────┐      │    │
│  │  │ solver-chains │     │solver-liquidity │     │   solver-oracles        │      │    │
│  │  ├───────────────┤     ├─────────────────┤     ├─────────────────────────┤      │    │
│  │  │ - EVM Adapter │     │ - DEX Discovery │     │ - Price Feeds           │      │    │
│  │  │ - Chain APIs  │     │ - Route Finding │     │ - Attestation Data      │      │    │
│  │  │ - Registry    │     │ - Slippage Calc │     │ - External APIs         │      │    │
│  │  └───────────────┘     └─────────────────┘     └─────────────────────────┘      │    │
│  └─────────────────────────────────────────────────────────────────────────────────┘    │
│                                                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────────────┐    │
│  │                           Infrastructure & Support                              │    │
│  │  ┌───────────────┐     ┌─────────────────┐     ┌─────────────────────────┐      │    │
│  │  │ solver-state  │     │solver-monitoring│     │   solver-config         │      │    │
│  │  ├───────────────┤     ├─────────────────┤     ├─────────────────────────┤      │    │
│  │  │ - Order State │     │ - Metrics       │     │ - TOML Parsing          │      │    │
│  │  │ - Settlement  │     │ - Health Checks │     │ - Validation            │      │    │
│  │  │ - Persistence │     │ - Tracing       │     │ - Configuration         │      │    │
│  │  └───────────────┘     └─────────────────┘     └─────────────────────────┘      │    │
│  └─────────────────────────────────────────────────────────────────────────────────┘    │
│                                                                                         │
│  ┌─────────────────────────────────────────────────────────────────────────────────┐    │
│  │                            solver-types (Shared Types)                          │    │
│  │  Common types, traits, and interfaces used across all crates                    │    │
│  └─────────────────────────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────────────────────────┘

Legend:
  ─────► Data flow
  ◄────► Bidirectional communication
  ┌────┐ Component/Module
  │    │ Crate boundary
```

## Architecture

The solver is built as a modular Rust workspace with specialized crates for different responsibilities:

### Core Components

- **solver-types**: Core types, traits, and common data structures used across all crates
- **solver-core**: Main orchestration engine that coordinates all solver components
- **solver-service**: HTTP API service and main executable binary

### Order Management

- **solver-orders**: Order parsing, classification, and protocol implementations (EIP-7683)
- **solver-discovery**: Intent discovery from on-chain events and off-chain sources
- **solver-validators**: Comprehensive validation pipeline for orders and solutions

### Execution Infrastructure

- **solver-chains**: Multi-chain blockchain adapters supporting various networks
- **solver-delivery**: Transaction submission and confirmation tracking
- **solver-strategies**: Strategy implementations for order execution and route optimization
- **solver-liquidity**: Liquidity source discovery and routing optimization

### Supporting Services

- **solver-settlement**: Settlement mechanisms and reward claiming
- **solver-state**: State management and persistence layer
- **solver-oracles**: Price oracle integrations for accurate valuations
- **solver-monitoring**: Metrics, health checks, and distributed tracing
- **solver-config**: Configuration management and validation

## Quick Start

```bash
# Build the project
cargo build

# Run tests
cargo test

# Run the solver service
cargo run --config config/local.toml --log-level debug
```

## Configuration

The solver can be configured using TOML files. See `config/` directory for examples:

- `config.toml` - Default configuration

### Validating Configuration

You can validate configuration files before running the solver using the `validate-config` binary:

```bash
# Validate a configuration file
cargo run --bin validate-config config/local.toml

# Or if you have already built the project
./target/debug/validate-config config/local.toml
```

This will check if the configuration file is valid and display key settings.

### Running with Custom Configuration

To use a custom configuration file:

```bash
# Using command line flag
cargo run -- --config path/to/your/config.toml

# Using environment variable
CONFIG_FILE=path/to/your/config.toml cargo run
```

### Log Levels

Available log levels (from most to least verbose):

- `trace` - Very detailed debugging information
- `debug` - Debugging information
- `info` - General information (default)
- `warn` - Warning messages
- `error` - Error messages only

Set the log level using:

```bash
# Command line flag
cargo run -- --log-level debug

# Short form
cargo run -- -l debug

# Environment variable
LOG_LEVEL=debug cargo run
```

## Running the Demo

The project includes a complete demo setup for testing cross-chain intent execution between two local chains.

### Prerequisites

- [Foundry](https://book.getfoundry.sh/getting-started/installation) (for Anvil, Forge, and Cast)
- Rust toolchain (stable)

### Step 1: Setup Local Test Environment

First, run the setup script to start two local blockchain nodes and deploy all necessary contracts:

```bash
# Make scripts executable (first time only)
chmod +x scripts/*.sh

# Setup two local chains with all contracts deployed
./scripts/setup_local_nodes.sh
```

This script will:

1. Start two Anvil instances:
   - Origin chain (ID: 31337) on port 8545
   - Destination chain (ID: 31338) on port 8546
2. Deploy test tokens on both chains
3. Deploy settler contracts (InputSettler, OutputSettler)
4. Deploy TheCompact contract for attestations
5. Create a `config/local.toml` configuration file
6. Fund test accounts with tokens

### Step 2: Start the Solver Service

In a new terminal, build and run the solver:

```bash
# Build the project
cargo build

# Run the solver with local configuration
cargo run --bin oif-solver -- --config config/local.toml --log-level debug
```

The solver will:

- Connect to both local chains
- Start monitoring for new intents
- Listen on port 8080 for API requests

### Step 3: Run the Demo

In another terminal, execute the demo script to create and observe a cross-chain intent:

```bash
# Run the demo
./scripts/demo.sh
```

The demo script will:

1. Show initial balances on both chains
2. Create a cross-chain intent (user deposits tokens on origin chain)
3. Wait for the solver to discover and fill the intent
4. Show final balances demonstrating successful execution

### What the Demo Demonstrates

1. **Intent Creation**: User deposits tokens into the InputSettler contract on the origin chain
2. **Discovery**: The solver detects the new intent through event monitoring
3. **Execution**: The solver fills the intent on the destination chain
4. **Settlement**: The solver claims rewards by providing attestations

### Monitoring the Demo

You can monitor the solver's activity through:

- Console logs (with debug level logging enabled)
- HTTP API endpoints:
  - `http://localhost:8080/health` - Health status
  - `http://localhost:8080/metrics` - Performance metrics
  - `http://localhost:8080/status` - Solver status

### Troubleshooting

If the demo doesn't work as expected:

1. Ensure all prerequisites are installed
2. Check that no other processes are using ports 8545, 8546, or 8080
3. Verify the solver is running and connected to both chains
4. Check solver logs for any error messages
5. Ensure you have sufficient balance in test accounts

## Development

This project uses a Rust workspace structure. Each crate is independently versioned and can be used separately.

### Building from Source

```bash
# Build all crates
cargo build --all

# Build in release mode
cargo build --release

# Run all tests
cargo test --all

# Run tests with output
cargo test --all -- --nocapture
```

### Code Structure

The codebase follows these conventions:

- Each crate has a focused responsibility
- Traits define interfaces between crates
- Types are shared through the `solver-types` crate
- Error handling uses the `Result` type with custom error variants
- Async runtime is Tokio
- Logging uses the `tracing` crate

## Current Implementation Status

### ✅ Implemented

- **Core Infrastructure**: Basic solver engine and coordination
- **Chain Adapters**: EVM chain support via ethers-rs
- **Order Discovery**: On-chain event monitoring for EIP-7683 intents
- **Transaction Delivery**: Direct RPC-based transaction submission
- **Basic Settlement**: Attestation generation and claim submission
- **Monitoring**: Metrics collection and health checks
- **API Service**: RESTful API for solver interaction

### 🚧 Partially Implemented

- **Order Types**: Only EIP-7683 cross-chain intents are supported
- **Chain Support**: Currently optimized for EVM chains only
- **Settlement**: Basic implementation without full reward optimization

### ❌ Not Yet Implemented

The following components have crate structure but require implementation:

- **Strategies**: Route optimization, execution planning, and fallback mechanisms
- **Validators**: Order validation, liquidity checks, and profitability assessment
- **Liquidity Sources**: DEX integrations and liquidity aggregation
- **Price Oracles**: External price feed integrations
- **Advanced Features**:
  - Multi-protocol support beyond EIP-7683
  - MEV protection strategies
  - Batch order processing
  - Advanced route optimization algorithms

## License

Licensed under MIT
