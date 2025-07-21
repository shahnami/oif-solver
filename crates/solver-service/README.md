# Solver Service

The main binary that runs the OIF Solver Service, orchestrating all components.

## Features

- HTTP API server for order submission and status queries
- Metrics endpoint for monitoring
- Plugin-based architecture for discovery, delivery, and state management
- Graceful shutdown handling
- Configuration validation

## Usage

### Start the service

```bash
cargo run -p solver-service -- --config config/local.toml
```

### Validate configuration

```bash
cargo run -p solver-service -- validate --config config/local.toml
```

### CLI Options

- `--config` / `-c`: Path to configuration file (default: `config/local.toml`)
- `--log-level`: Logging level (default: `info`)

## API Endpoints

### Health Endpoints

- `GET /health` - Main health check
- `GET /health/live` - Kubernetes liveness probe
- `GET /health/ready` - Kubernetes readiness probe

### Order Endpoints

- `POST /api/v1/orders` - Create a new order
- `GET /api/v1/orders/:order_id` - Get order status

### Admin Endpoints

- `GET /api/v1/admin/config` - Get current configuration

### Metrics

- `GET /metrics` - Prometheus metrics endpoint (on separate port)

## Configuration

The service requires a TOML configuration file with the following structure:

```toml
[solver]
name = "my-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.discovery.evm_logs]
enabled = true
plugin_type = "evm_logs"
# ... plugin-specific config

[plugins.delivery.evm_rpc]
enabled = true
plugin_type = "evm_rpc"
# ... plugin-specific config

[plugins.state.memory]
enabled = true
plugin_type = "memory"
# ... plugin-specific config
```

## Architecture

The service uses `solver-core` to orchestrate:
- Discovery plugins for finding cross-chain intents
- Delivery plugins for executing settlements
- State plugins for tracking order status
- Event processing pipeline for coordinating components