# Solver Service - Main Binary and API Server

The `solver-service` crate is the main executable that brings together all components of the OIF solver into a running service. It provides HTTP APIs, metrics endpoints, health checks, and graceful lifecycle management for the entire solver system.

## Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         SOLVER SERVICE                                   │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                    Service Components                              │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │    CLI      │  │   Config     │  │   Orchestrator         │  │  │
│  │  │  (clap)     │─▶│   Loader     │─▶│    Builder             │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                     Runtime Services                               │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │  HTTP API   │  │   Metrics    │  │   Signal               │  │  │
│  │  │  Server     │  │   Server     │  │   Handler              │  │  │
│  │  │  (port 8080)│  │  (port 9090) │  │  (SIGTERM/SIGINT)      │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                          ┌─────────┴─────────┐
                          │                   │
                 ┌────────▼────────┐ ┌────────▼────────┐
                 │ Orchestrator    │ │ Service Wrapper │
                 │ (solver-core)   │ │ (SolverService) │
                 └────────┬────────┘ └────────┬────────┘
                          │                   │
                 ┌────────▼────────┐ ┌────────▼────────┐
                 │ All Services    │ │ Health & Config │
                 │ & Plugins       │ │ Access          │
                 └─────────────────┘ └─────────────────┘
```

## Module Structure

```
solver-service/
├── src/
│   ├── main.rs         # Entry point with CLI and startup logic
│   ├── service.rs      # Service wrapper for orchestrator
│   └── api.rs          # HTTP API endpoints and handlers
├── Cargo.toml          # Binary configuration
└── README.md           # This file
```

## Key Components

### 1. **Main Entry Point** (`main.rs`)

The application entry point with CLI parsing and service initialization:

**CLI Structure:**

```rust
struct Cli {
    command: Option<Commands>,       // start (default) or validate
    config: PathBuf,                // Config file path
    log_level: String,              // Logging level
}
```

**Commands:**

- `start` (default) - Start the solver service
- `validate` - Validate configuration without starting

**Startup Flow:**

```text
1. Parse CLI args
2. Setup tracing/logging
3. Load configuration
4. Build orchestrator
5. Start orchestrator
6. Spawn HTTP server
7. Spawn metrics server
8. Wait for shutdown signal
9. Graceful shutdown
```

### 2. **Service Wrapper** (`service.rs`)

Simple wrapper that holds the orchestrator and configuration:

```rust
pub struct SolverService {
    orchestrator: Arc<Orchestrator>,
    config: SolverConfig,
}
```

**Purpose:**

- Provides shared state for HTTP handlers
- Exposes health check functionality
- Holds configuration for admin endpoints

### 3. **API Server** (`api.rs`)

HTTP API implementation using Axum framework:

**Endpoints:**

```text
Health Endpoints:
├── GET /health              → Detailed health status
├── GET /health/live         → Kubernetes liveness (always 200)
└── GET /health/ready        → Kubernetes readiness

Order Endpoints:
└── GET /api/v1/orders/{id}  → Get order status (TODO)

Admin Endpoints:
└── GET /api/v1/admin/config → Current configuration

Metrics Server (separate port):
└── GET /metrics             → Prometheus metrics (TODO)
```

**Middleware Stack:**

- CORS (permissive)
- HTTP tracing
- State injection

## Service Lifecycle

```text
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ CLI Parsing  │────▶│ Config Load  │────▶│ Orchestrator │
│              │     │              │     │    Build     │
└──────────────┘     └──────────────┘     └──────────────┘
                                                  │
                                                  ▼
┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│   Shutdown   │◀────│  Run Until   │◀────│    Start     │
│   Cleanup    │     │   Signal     │     │   Services   │
└──────────────┘     └──────────────┘     └──────────────┘
```

### Startup Sequence:

1. **CLI Parsing**: Parse command-line arguments
2. **Tracing Setup**: Initialize logging framework
3. **Config Loading**: Load and validate TOML config
4. **Orchestrator Build**: Create orchestrator with plugins
5. **Service Start**: Start orchestrator and spawn servers
6. **Signal Wait**: Run until SIGTERM/SIGINT

### Shutdown Sequence:

1. **Signal Received**: Ctrl+C or SIGTERM
2. **Orchestrator Shutdown**: Stop all services gracefully
3. **Server Abort**: Cancel HTTP/metrics servers
4. **Exit**: Clean process termination

## Usage Examples

### Starting the Service:

```bash
# Start with default config
cargo run -p solver-service

# Start with custom config
cargo run -p solver-service -- --config config/production.toml

# Start with debug logging
cargo run -p solver-service -- --log-level debug

# Validate config only
cargo run -p solver-service -- validate --config config/test.toml
```

### Environment Variables:

```bash
# Override log level
export SOLVER_LOG_LEVEL=debug

# Tracing filter
export RUST_LOG=solver_service=debug,solver_core=info
```

### Health Check:

```bash
# Detailed health status
curl http://localhost:8080/health

# Response:
{
  "status": "healthy",
  "services": {
    "discovery": true,
    "delivery": true,
    "state": true,
    "event_processor": true
  }
}
```

## Critical Observations

### Strengths:

1. **Clean Architecture**: Clear separation of concerns
2. **Graceful Shutdown**: Proper signal handling
3. **Health Checks**: Kubernetes-ready probes
4. **Flexible CLI**: Command-based interface
5. **Structured Logging**: Tracing integration

### Areas of Concern:

1. **TODO Endpoints**: Order retrieval not implemented
2. **Placeholder Metrics**: No real metrics collection
3. **No Authentication**: Admin endpoints unprotected
4. **Error Handling**: Some unwraps in signal handling
5. **Missing Features**: No order submission endpoint

### Implementation Gaps:

- ❌ Order submission API (`POST /api/v1/orders`)
- ❌ Order status retrieval from state
- ❌ Proper metrics collection
- ❌ Request validation
- ❌ Rate limiting
- ❌ Authentication/authorization

## Dependencies

### Internal Crates:

- `solver-types`: Configuration and type definitions
- `solver-core`: Orchestrator implementation
- `solver-config`: Configuration loading
- `solver-delivery`: Transaction delivery (indirect)
- `solver-discovery`: Order discovery (indirect)
- `solver-plugin`: Plugin system (indirect)

### External Dependencies:

- `tokio`: Async runtime and signal handling
- `axum`: HTTP server framework
- `tower`/`tower-http`: HTTP middleware
- `clap`: Command-line parsing
- `tracing`/`tracing-subscriber`: Structured logging
- `anyhow`: Error handling
- `serde`/`serde_json`: JSON serialization

### Unused Dependencies:

- `toml`: Listed but not directly used
- `hex`, `bytes`, `chrono`, `regex`: Imported but unused

## Runtime Behavior

### Process Model:

```text
Main Thread
├── CLI parsing & config loading
├── Orchestrator initialization
└── Tokio runtime
    ├── HTTP server task
    ├── Metrics server task
    ├── Signal handler task
    └── Orchestrator tasks (from solver-core)
```

### Port Allocation:

- **8080**: Main HTTP API (configurable)
- **9090**: Metrics endpoint (configurable)

### Signal Handling:

- **SIGINT** (Ctrl+C): Graceful shutdown
- **SIGTERM**: Graceful shutdown (Kubernetes)
- Windows: Only Ctrl+C supported

## Known Issues & Cruft

1. **Hardcoded TODO**: Order endpoint returns dummy data
2. **Fake Metrics**: Metrics endpoint returns static text
3. **Unused Imports**: Several dependencies not used
4. **Error Unwraps**: Signal handler uses expect()
5. **No Timeout**: Shutdown has no timeout mechanism

## Future Improvements

1. **Complete API**: Implement all planned endpoints
2. **Real Metrics**: Integrate Prometheus properly
3. **Authentication**: Add API key or JWT support
4. **Rate Limiting**: Protect against abuse
5. **OpenAPI**: Generate API documentation
6. **WebSocket**: Real-time order updates
7. **Admin UI**: Web interface for monitoring
8. **Distributed Tracing**: OpenTelemetry integration

## Performance Considerations

- **Cloning Service**: State cloned for each request
- **No Connection Pooling**: Each plugin manages connections
- **Blocking Config Load**: Config loaded synchronously
- **No Request Limits**: Unbounded request size

## Security Considerations

- **No Auth**: All endpoints publicly accessible
- **Config Exposure**: Admin endpoint shows secrets
- **CORS Permissive**: Allows all origins
- **No HTTPS**: HTTP only (relies on proxy)
- **No Rate Limiting**: Vulnerable to DoS

## Monitoring & Observability

Current implementation provides:

- Basic health checks
- Placeholder metrics endpoint
- Structured logging with tracing

Missing:

- Real metrics collection
- Distributed tracing
- Error tracking
- Performance monitoring

The `solver-service` crate successfully ties together all solver components into a running service, though several API endpoints and features remain to be implemented for production readiness.
