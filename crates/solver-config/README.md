# solver-config

## Overview

The `solver-config` module handles configuration parsing, validation, and hot-reloading for the OIF solver. It provides a flexible configuration system that supports multiple sources (files, environment variables, CLI arguments) with schema validation and runtime updates.

## Architecture

### Core Components

1. **ConfigLoader** - Merges configuration from multiple sources
2. **ConfigSchema** - Defines and validates configuration structure
3. **ConfigWatcher** - Monitors and applies configuration changes
4. **ConfigSources** - File, environment, and CLI configuration sources

### Design Principles

- **Layered Configuration**: Override precedence (CLI > ENV > File)
- **Type Safety**: Strongly typed configuration structures
- **Validation First**: Schema validation before deserialization
- **Hot Reload**: Update configuration without restart
- **Backward Compatibility**: Graceful handling of old configs

## Structure

```rust
// Main configuration structure
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct Config {
    // Core settings
    pub solver_name: String,
    pub log_level: String,
    pub http_port: u16,
    pub metrics_port: u16,

    // Order types to support
    pub order_types: Vec<String>,

    // Component configs
    pub state: StateConfig,
    pub discovery: DiscoveryConfig,
    pub delivery: DeliveryConfig,
    pub settlement: SettlementConfig,
}

// Component-specific configurations
#[derive(Debug, Deserialize, Serialize)]
pub struct StateConfig {
    #[serde(rename = "type")]
    pub backend_type: String,
    pub max_items: Option<usize>,
    pub ttl_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DiscoveryConfig {
    pub sources: Vec<String>,
    #[serde(flatten)]
    pub source_configs: HashMap<String, Value>,
}
```

## Abstractions

### Configuration Sources

```rust
#[async_trait]
pub trait ConfigSource: Send + Sync {
    async fn load(&self) -> Result<Value>;
    fn priority(&self) -> i32;
}

// File-based configuration
pub struct FileSource {
    path: PathBuf,
    format: ConfigFormat,
}

// Environment variable configuration
pub struct EnvSource {
    prefix: String,
}

// Command-line arguments
pub struct CliSource {
    args: Vec<String>,
}
```

### Configuration Validation

```rust
pub trait ConfigValidator: Send + Sync {
    fn validate(&self, config: &Value) -> Result<()>;
}

pub struct SchemaValidator {
    schema: JsonSchema,
}

pub struct BusinessRuleValidator {
    rules: Vec<Box<dyn ValidationRule>>,
}
```

## Usage

### Basic Usage

```rust
// Load configuration
let loader = ConfigLoader::new()
    .with_source(FileSource::new("config.toml"))
    .with_source(EnvSource::with_prefix("SOLVER_"))
    .with_validator(SchemaValidator::default());

let config: Config = loader.load().await?;

// Use configuration
let solver = Solver::builder()
    .with_config(config)
    .build()?;
```

### Configuration File Example

```toml
# config.toml
solver_name = "production-solver"
log_level = "info"
http_port = 8080
metrics_port = 9090

order_types = ["eip7683", "uniswapx"]

[state]
type = "redis://localhost:6379"
max_items = 10000
ttl_seconds = 3600

[discovery]
sources = ["onchain", "webhook"]

[discovery.onchain]
chain_id = 1
contracts = ["0x1234...", "0x5678..."]
start_block = 18000000
poll_interval = "12s"

[discovery.webhook]
port = 8081
auth_token = "${WEBHOOK_AUTH_TOKEN}"

[delivery]
methods = ["rpc", "flashbots"]
strategy = "fastest"

[delivery.rpc]
endpoints = { 1 = "https://eth.rpc", 137 = "https://polygon.rpc" }
max_retries = 3

[settlement]
strategy = "direct"
oracle_address = "0x..."
claim_delay = "1h"
```

### Environment Variables

```bash
# Override configuration via environment
export SOLVER_LOG_LEVEL=debug
export SOLVER_STATE_TYPE=memory
export SOLVER_DISCOVERY_SOURCES=webhook
export SOLVER_HTTP_PORT=8888
```

### Hot Reload

```rust
// Create config watcher
let watcher = ConfigWatcher::new(loader)
    .with_interval(Duration::from_secs(30))
    .on_change(|old_config, new_config| {
        info!("Configuration updated");
        solver.update_config(new_config).await?;
    });

// Start watching
watcher.start().await?;
```

## Pros

1. **Flexibility**: Multiple configuration sources
2. **Validation**: Catch errors before runtime
3. **Hot Reload**: Update without downtime
4. **Type Safety**: Compile-time guarantees
5. **Environment Support**: Easy deployment configuration

## Cons

1. **Complexity**: Multiple layers of configuration
2. **Validation Overhead**: Schema checking adds startup time
3. **Merge Conflicts**: Complex override rules
4. **Type Constraints**: Some dynamic configs harder to express

## Implementation Details

### Configuration Merging

```rust
impl ConfigLoader {
    pub async fn load<T: DeserializeOwned + Validate>(&self) -> Result<T> {
        let mut sources: Vec<_> = self.sources.iter().collect();
        sources.sort_by_key(|s| s.priority());

        let mut config = Value::Object(Map::new());

        // Merge in priority order
        for source in sources {
            let value = source.load().await?;
            merge_values(&mut config, value);
        }

        // Expand environment variables
        expand_env_vars(&mut config)?;

        // Validate against schema
        for validator in &self.validators {
            validator.validate(&config)?;
        }

        // Deserialize and validate
        let result: T = serde_json::from_value(config)?;
        result.validate()?;

        Ok(result)
    }
}

fn merge_values(base: &mut Value, other: Value) {
    match (base, other) {
        (Value::Object(base_map), Value::Object(other_map)) => {
            for (key, value) in other_map {
                match base_map.get_mut(&key) {
                    Some(base_value) => merge_values(base_value, value),
                    None => { base_map.insert(key, value); }
                }
            }
        }
        (base, other) => *base = other,
    }
}
```

### Environment Variable Expansion

```rust
fn expand_env_vars(value: &mut Value) -> Result<()> {
    match value {
        Value::String(s) => {
            if s.starts_with("${") && s.ends_with("}") {
                let var_name = &s[2..s.len()-1];
                *s = env::var(var_name)
                    .map_err(|_| Error::MissingEnvVar(var_name.to_string()))?;
            }
        }
        Value::Object(map) => {
            for (_, v) in map.iter_mut() {
                expand_env_vars(v)?;
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                expand_env_vars(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}
```

### Schema Validation

```rust
impl SchemaValidator {
    pub fn new() -> Self {
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "required": ["solver_name", "order_types", "state", "discovery", "delivery", "settlement"],
            "properties": {
                "solver_name": { "type": "string" },
                "log_level": { "enum": ["trace", "debug", "info", "warn", "error"] },
                "http_port": { "type": "integer", "minimum": 1, "maximum": 65535 },
                "order_types": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1
                },
                "state": {
                    "type": "object",
                    "required": ["type"],
                    "properties": {
                        "type": { "type": "string" },
                        "max_items": { "type": "integer", "minimum": 1 },
                        "ttl_seconds": { "type": "integer", "minimum": 1 }
                    }
                }
            }
        });

        Self {
            schema: JSONSchema::compile(&schema).unwrap()
        }
    }
}
```

### Configuration Updates

```rust
#[derive(Debug, Deserialize)]
pub struct ConfigUpdates {
    pub log_level: Option<String>,
    pub discovery: Option<DiscoveryUpdates>,
    pub delivery: Option<DeliveryUpdates>,
}

impl Config {
    pub fn apply_updates(&mut self, updates: ConfigUpdates) -> Result<()> {
        if let Some(log_level) = updates.log_level {
            self.validate_log_level(&log_level)?;
            self.log_level = log_level;
        }

        if let Some(discovery) = updates.discovery {
            self.discovery.apply_updates(discovery)?;
        }

        if let Some(delivery) = updates.delivery {
            self.delivery.apply_updates(delivery)?;
        }

        Ok(())
    }
}
```

### File Watching

```rust
pub struct ConfigWatcher {
    loader: ConfigLoader,
    interval: Duration,
    callbacks: Vec<Box<dyn Fn(&Config, &Config) + Send + Sync>>,
    current_config: Arc<RwLock<Config>>,
}

impl ConfigWatcher {
    pub async fn start(self) -> Result<()> {
        let mut interval = tokio::time::interval(self.interval);

        loop {
            interval.tick().await;

            match self.loader.load::<Config>().await {
                Ok(new_config) => {
                    let current = self.current_config.read().await;
                    
                    if !configs_equal(&current, &new_config) {
                        info!("Configuration change detected");
                        
                        for callback in &self.callbacks {
                            callback(&current, &new_config);
                        }
                        
                        drop(current);
                        *self.current_config.write().await = new_config;
                    }
                }
                Err(e) => {
                    warn!("Failed to reload configuration: {}", e);
                }
            }
        }
    }
}
```

## Error Handling

```rust
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),
    
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    
    #[error("Validation failed: {0}")]
    ValidationFailed(String),
    
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),
    
    #[error("Schema violation: {0}")]
    SchemaViolation(String),
}
```

## Metrics

The module exposes metrics for monitoring:

- `config_reload_total` - Configuration reload attempts
- `config_reload_success_total` - Successful reloads
- `config_validation_duration_seconds` - Validation time
- `config_source_errors_total` - Errors by source

## Future Enhancements

1. **Remote Configuration**: Fetch from etcd/Consul
2. **Configuration History**: Track changes over time
3. **A/B Testing**: Multiple configuration variants
4. **Feature Flags**: Dynamic feature toggles
5. **Configuration UI**: Web interface for updates
