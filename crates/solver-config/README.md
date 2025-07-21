# Solver Config - Configuration Management System

The `solver-config` crate provides a simple yet powerful configuration loading system for the OIF solver. It handles TOML file parsing, environment variable substitution, and validation to ensure the solver starts with a valid configuration.

## 🏗️ Architecture Overview

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                         CONFIG LOADER                                    │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                    Loading Pipeline                                │  │
│  │  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │  │
│  │  │ TOML File   │  │ Env Variable │  │   Validation           │  │  │
│  │  │ Loading     │─▶│ Substitution │─▶│   & Override           │  │  │
│  │  └─────────────┘  └──────────────┘  └────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                                                                          │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │                  Configuration Flow                                │  │
│  │                                                                    │  │
│  │   config.toml ─┐                                                  │  │
│  │                 ├─▶ ConfigLoader ─▶ SolverConfig ─▶ Services     │  │
│  │   ENV vars ────┘                                                  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
```

## 📁 Module Structure

```
solver-config/
├── src/
│   └── lib.rs          # Configuration loader implementation
├── config/             # Example configurations (in parent)
│   ├── example.toml    # Example with all options
│   └── local.toml      # Local development config
├── Cargo.toml
└── README.md
```

## 🔑 Key Components

### 1. **ConfigLoader** (`lib.rs`)

The main configuration loading struct that orchestrates the loading process:

```rust
pub struct ConfigLoader {
    file_path: Option<String>,    // Path to TOML config file
    env_prefix: String,           // Prefix for env var overrides (default: "SOLVER_")
}
```

**Key Responsibilities:**

- Load and parse TOML configuration files
- Substitute environment variables in config values
- Apply environment variable overrides
- Validate required plugins are enabled

### 2. **Configuration Pipeline**

The loading process follows these steps:

```text
1. Load TOML File
      ↓
2. Substitute ${VAR_NAME} placeholders
      ↓
3. Apply SOLVER_* env overrides
      ↓
4. Validate configuration
      ↓
5. Return SolverConfig
```

### 3. **Error Handling**

Well-defined error types for configuration issues:

```rust
pub enum ConfigError {
    FileNotFound(String),      // Config file doesn't exist
    ParseError(String),        // TOML parsing failed
    ValidationError(String),   // Config validation failed
    EnvVarNotFound(String),   // Required env var missing
    IoError(std::io::Error),  // File system errors
}
```

## 🔄 Configuration Loading Flow

```text
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│   config.toml    │────▶│  Read & Parse    │────▶│  Env Variable    │
│                  │     │   TOML File      │     │  Substitution    │
└──────────────────┘     └──────────────────┘     └──────────────────┘
                                                            │
                                                            ▼
┌──────────────────┐     ┌──────────────────┐     ┌──────────────────┐
│  SolverConfig    │◀────│    Validate      │◀────│   Apply Env      │
│   (validated)    │     │  Configuration   │     │   Overrides      │
└──────────────────┘     └──────────────────┘     └──────────────────┘
```

## 🚀 Usage Example

```rust
use solver_config::ConfigLoader;

// Basic usage
let config = ConfigLoader::new()
    .with_file("config/local.toml")
    .load()
    .await?;

// With custom environment prefix
let config = ConfigLoader::new()
    .with_file("config/production.toml")
    .with_env_prefix("MY_SOLVER_")
    .load()
    .await?;
```

## 📝 Configuration File Format

The configuration uses TOML format with the following structure:

```toml
# Main solver settings
[solver]
name = "my-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

# Plugin configurations
[plugins.state.memory_state]
enabled = true
plugin_type = "memory"
[plugins.state.memory_state.config]
max_entries = 10000

[plugins.delivery.eth_delivery]
enabled = true
plugin_type = "evm_ethers"
[plugins.delivery.eth_delivery.config]
chain_id = 1
rpc_url = "https://eth-mainnet.g.alchemy.com/v2/${ALCHEMY_KEY}"
private_key = "${SOLVER_PRIVATE_KEY}"

# Service configurations
[discovery]
historical_sync = false
realtime_monitoring = true
max_event_age_seconds = 3600

[state]
default_backend = "memory_state"
cleanup_interval_seconds = 300
```

## 🌍 Environment Variable Support

### Variable Substitution

Use `${VAR_NAME}` syntax in TOML values:

```toml
rpc_url = "https://eth-mainnet.g.alchemy.com/v2/${ALCHEMY_KEY}"
private_key = "${SOLVER_PRIVATE_KEY}"
```

### Environment Overrides

Override specific configuration values with environment variables:

```bash
# Override solver settings
export SOLVER_LOG_LEVEL=debug
export SOLVER_HTTP_PORT=8888
export SOLVER_METRICS_PORT=9999

# These will override values in the TOML file
```

Currently supported overrides:

- `SOLVER_LOG_LEVEL` - Override log level
- `SOLVER_HTTP_PORT` - Override HTTP API port
- `SOLVER_METRICS_PORT` - Override metrics port

## 🔍 Critical Observations

### Strengths:

1. **Simple Design**: Straightforward loading without over-engineering
2. **Environment Support**: Both substitution and override mechanisms
3. **Clear Errors**: Well-defined error types with context
4. **Async Loading**: Non-blocking file operations
5. **Validation**: Ensures required plugins are enabled

### Areas of Concern:

1. **Limited Overrides**: Only 3 fields support env overrides
2. **No Hot Reload**: Changes require restart
3. **No Schema Validation**: Beyond basic plugin checks
4. **Regex Performance**: Compiles regex on every substitution
5. **No Config Merging**: Can't layer multiple config files

### Actual vs Documented Implementation:

The existing README describes a much more complex system than what's implemented:

- ❌ No ConfigWatcher for hot reload
- ❌ No multiple configuration sources
- ❌ No schema validation
- ❌ No configuration history
- ✅ Simple TOML loading with env vars (actual implementation)

## 🔗 Dependencies

### Internal Crates:

- `solver-types`: Imports `SolverConfig` type definition

### External Dependencies:

- `tokio`: Async file operations
- `toml`: TOML parsing
- `regex`: Environment variable pattern matching
- `thiserror`: Error type derivation
- `serde`/`serde_json`: Serialization (though JSON not used)

### Dependency Concerns:

1. **Unused serde_json**: Imported but only TOML is used
2. **Regex Overhead**: Could use simpler string matching
3. **Missing config crate**: Could use standard config management

## 🏃 Runtime Behavior

### Loading Sequence:

1. **File Reading**: Async read of TOML file
2. **Variable Substitution**: Replace ${VAR} patterns
3. **TOML Parsing**: Deserialize to SolverConfig
4. **Override Application**: Apply SOLVER\_\* env vars
5. **Validation**: Check required plugins enabled

### Error Handling:

- File not found → Clear error with path
- Missing env var → Shows variable name
- Parse errors → TOML error details
- Validation → Specific requirement failures

## 🐛 Known Issues & Cruft

1. **Regex Compilation**: Regex compiled on every load (line 89)
2. **Limited Validation**: Only checks enabled plugins, not configs
3. **Hardcoded Overrides**: Override fields hardcoded in apply_env_overrides
4. **No Default File**: Must explicitly specify config file
5. **Unused Dependencies**: serde_json imported but not used

## 🔮 Future Improvements

1. **Expand Env Overrides**: Support all configuration fields
2. **Config Validation**: JSON Schema or similar validation
3. **Hot Reload**: Watch config file for changes
4. **Multiple Sources**: Layer configs (defaults → file → env → CLI)
5. **Config Templates**: Generate example configs
6. **Encrypted Secrets**: Support for encrypted values
7. **Remote Config**: Fetch from HTTP/S3/etcd

## 📊 Performance Considerations

- **Regex Cost**: Pattern matching for every substitution
- **File I/O**: Async but still blocks on parse
- **Validation**: Minimal overhead (just enabled checks)
- **Memory**: Entire config held in memory

## ⚠️ Security Considerations

- **Private Keys**: Stored in plaintext in config
- **Env Var Exposure**: All env vars accessible
- **No Encryption**: Sensitive data unprotected
- **File Permissions**: No checks on config file access
- **Injection Risk**: Env var substitution could be exploited

## 📋 Configuration Examples

### Minimal Configuration:

```toml
[solver]
name = "minimal-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

[plugins.state.memory]
enabled = true
plugin_type = "memory"

[plugins.delivery.local]
enabled = true
plugin_type = "evm_ethers"
[plugins.delivery.local.config]
chain_id = 1
rpc_url = "http://localhost:8545"
private_key = "${PRIVATE_KEY}"

[discovery]
realtime_monitoring = true

[state]
default_backend = "memory"

[delivery]
strategy = "RoundRobin"

[settlement]
default_strategy = "direct"
```

The `solver-config` crate provides a focused, practical configuration loading solution that prioritizes simplicity over features, making it easy to understand and maintain while providing the essential functionality needed for the solver.
