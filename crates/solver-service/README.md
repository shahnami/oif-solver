# Solver Service

A complete implementation of the OIF solver using the solver-core framework.

## Features

- **File Storage**: Persistent storage using the filesystem
- **Alloy RPC Delivery**: EVM transaction submission using Alloy
- **Local Wallet**: ECDSA signing with k256
- **EIP-7683 Discovery**: On-chain order discovery for EIP-7683 orders
- **Direct Settlement**: Oracle-based settlement verification

## Configuration

See `config.example.toml` for a complete configuration example.

## Running

```bash
cargo run -- --config config.toml
```

## Components

### Storage
- `FileStorage`: Stores data as files with atomic writes

### Account
- `LocalWallet`: Uses k256 for ECDSA signing

### Delivery
- `AlloyDelivery`: Submits transactions via Alloy RPC

### Discovery
- `Eip7683Discovery`: Monitors on-chain events for new orders

### Order
- `Eip7683OrderImpl`: Validates and generates transactions for EIP-7683 orders
- `SimpleStrategy`: Basic execution strategy with gas price limits

### Settlement
- `DirectSettlement`: Validates fills and manages claim eligibility