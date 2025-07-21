use async_trait::async_trait;
use bytes::Bytes;
use solver_types::plugins::{
	state::*, BasePlugin, ConfigFieldType, PluginConfig, PluginConfigSchema, PluginError,
	PluginHealth, PluginMetrics, PluginResult,
};
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Plugin that creates in-memory state stores
#[derive(Debug, Default)]
pub struct InMemoryStatePlugin {
	config: InMemoryConfig,
}

#[derive(Debug, Clone, Default)]
pub struct InMemoryConfig {
	pub max_entries: Option<usize>,
	pub default_ttl: Option<Duration>,
}

impl InMemoryStatePlugin {
	pub fn new() -> Self {
		Self {
			config: InMemoryConfig::default(),
		}
	}

	pub fn with_config(config: InMemoryConfig) -> Self {
		Self { config }
	}
}

#[async_trait]
impl BasePlugin for InMemoryStatePlugin {
	fn plugin_type(&self) -> &'static str {
		"memory_state"
	}

	fn name(&self) -> String {
		"In-Memory State Plugin".to_string()
	}

	fn version(&self) -> &'static str {
		"1.0.0"
	}

	fn description(&self) -> &'static str {
		"In-memory state storage plugin"
	}

	async fn initialize(&mut self, _config: PluginConfig) -> PluginResult<()> {
		// Extract configuration from PluginConfig if needed
		Ok(())
	}

	fn validate_config(&self, _config: &PluginConfig) -> PluginResult<()> {
		Ok(())
	}

	async fn health_check(&self) -> PluginResult<PluginHealth> {
		Ok(PluginHealth::healthy(
			"In-memory state plugin is operational",
		))
	}

	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		Ok(PluginMetrics::new())
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		Ok(())
	}

	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new().optional(
			"max_entries",
			ConfigFieldType::Number,
			"Maximum number of entries",
			None,
		)
	}

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn as_any_mut(&mut self) -> &mut dyn Any {
		self
	}
}

#[async_trait]
impl StatePlugin for InMemoryStatePlugin {
	fn backend_type(&self) -> &'static str {
		"memory"
	}

	async fn create_store(&self) -> PluginResult<Box<dyn StateStore>> {
		Ok(Box::new(InMemoryStore::new(self.config.clone())))
	}

	fn supports_ttl(&self) -> bool {
		true
	}

	fn supports_transactions(&self) -> bool {
		false
	}

	fn supports_atomic_operations(&self) -> bool {
		true
	}

	async fn get_backend_config(&self) -> PluginResult<BackendConfig> {
		Ok(BackendConfig {
			backend_type: "memory".to_string(),
			version: "1.0.0".to_string(),
			features: vec!["ttl".to_string(), "atomic_operations".to_string()],
			limits: BackendLimits {
				max_key_size: None,
				max_value_size: None,
				max_keys: self.config.max_entries.map(|n| n as u64),
				max_storage_size: None,
				max_ttl: self.config.default_ttl.map(|d| d.as_secs()),
			},
			settings: HashMap::new(),
		})
	}

	async fn optimize(&self) -> PluginResult<OptimizationResult> {
		Ok(OptimizationResult {
			optimization_type: "memory_optimization".to_string(),
			bytes_freed: 0,
			time_taken_ms: 0,
			performance_improvement: None,
		})
	}

	async fn backup(&self, _destination: &str) -> PluginResult<BackupResult> {
		Err(PluginError::NotSupported(
			"Backup not supported for in-memory storage".to_string(),
		))
	}

	async fn restore(&self, _source: &str) -> PluginResult<RestoreResult> {
		Err(PluginError::NotSupported(
			"Restore not supported for in-memory storage".to_string(),
		))
	}
}

type InMemoryData = HashMap<String, (Bytes, Option<SystemTime>)>;

/// The actual in-memory store implementation
#[derive(Debug)]
pub struct InMemoryStore {
	data: Arc<RwLock<InMemoryData>>,
	config: InMemoryConfig,
}

impl InMemoryStore {
	pub fn new(config: InMemoryConfig) -> Self {
		Self {
			data: Arc::new(RwLock::new(HashMap::new())),
			config,
		}
	}

	fn is_expired(&self, expiry: &Option<SystemTime>) -> bool {
		if let Some(exp) = expiry {
			SystemTime::now() > *exp
		} else {
			false
		}
	}

	fn check_capacity(
		&self,
		data: &HashMap<String, (Bytes, Option<SystemTime>)>,
	) -> PluginResult<()> {
		if let Some(max_entries) = self.config.max_entries {
			if data.len() >= max_entries {
				return Err(PluginError::StateError(
					"Memory store capacity exceeded".to_string(),
				));
			}
		}
		Ok(())
	}
}

#[async_trait]
impl StateStore for InMemoryStore {
	async fn get(&self, key: &str) -> PluginResult<Option<Bytes>> {
		let data = self
			.data
			.read()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire read lock: {}", e)))?;

		if let Some((value, expiry)) = data.get(key) {
			if self.is_expired(expiry) {
				return Ok(None);
			}
			Ok(Some(value.clone()))
		} else {
			Ok(None)
		}
	}

	async fn set(&self, key: &str, value: Bytes) -> PluginResult<()> {
		let mut data = self
			.data
			.write()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire write lock: {}", e)))?;

		self.check_capacity(&data)?;
		data.insert(key.to_string(), (value, None));
		Ok(())
	}

	async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()> {
		let mut data = self
			.data
			.write()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire write lock: {}", e)))?;

		self.check_capacity(&data)?;
		let expiry = SystemTime::now() + ttl;
		data.insert(key.to_string(), (value, Some(expiry)));
		Ok(())
	}

	async fn delete(&self, key: &str) -> PluginResult<()> {
		let mut data = self
			.data
			.write()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire write lock: {}", e)))?;
		data.remove(key);
		Ok(())
	}

	async fn exists(&self, key: &str) -> PluginResult<bool> {
		let data = self
			.data
			.read()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire read lock: {}", e)))?;

		if let Some((_, expiry)) = data.get(key) {
			Ok(!self.is_expired(expiry))
		} else {
			Ok(false)
		}
	}

	async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>> {
		let data = self
			.data
			.read()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire read lock: {}", e)))?;

		let keys: Vec<String> = data
			.iter()
			.filter(|(k, (_, expiry))| {
				// Filter out expired entries
				if self.is_expired(expiry) {
					return false;
				}
				// Apply prefix filter if provided
				if let Some(p) = prefix {
					k.starts_with(p)
				} else {
					true
				}
			})
			.map(|(k, _)| k.clone())
			.collect();

		Ok(keys)
	}

	async fn batch_get(&self, keys: &[String]) -> PluginResult<Vec<Option<Bytes>>> {
		let data = self
			.data
			.read()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire read lock: {}", e)))?;

		let results = keys
			.iter()
			.map(|key| {
				if let Some((value, expiry)) = data.get(key) {
					if self.is_expired(expiry) {
						None
					} else {
						Some(value.clone())
					}
				} else {
					None
				}
			})
			.collect();

		Ok(results)
	}

	async fn batch_set(&self, items: &[(String, Bytes)]) -> PluginResult<()> {
		let mut data = self
			.data
			.write()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire write lock: {}", e)))?;

		// Check capacity for all items
		if let Some(max_entries) = self.config.max_entries {
			let new_entries = items.len();
			if data.len() + new_entries > max_entries {
				return Err(PluginError::StateError(
					"Memory store capacity would be exceeded".to_string(),
				));
			}
		}

		for (key, value) in items {
			data.insert(key.clone(), (value.clone(), None));
		}

		Ok(())
	}

	async fn batch_delete(&self, keys: &[String]) -> PluginResult<()> {
		let mut data = self
			.data
			.write()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire write lock: {}", e)))?;

		for key in keys {
			data.remove(key);
		}

		Ok(())
	}

	async fn atomic_update(
		&self,
		key: &str,
		updater: Box<dyn FnOnce(Option<Bytes>) -> PluginResult<Option<Bytes>> + Send>,
	) -> PluginResult<()> {
		let mut data = self
			.data
			.write()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire write lock: {}", e)))?;

		// Get current value
		let current_value = if let Some((value, expiry)) = data.get(key) {
			if self.is_expired(expiry) {
				None
			} else {
				Some(value.clone())
			}
		} else {
			None
		};

		// Apply updater function
		let new_value = updater(current_value)?;

		// Update storage
		match new_value {
			Some(value) => {
				self.check_capacity(&data)?;
				data.insert(key.to_string(), (value, None));
			}
			None => {
				data.remove(key);
			}
		}

		Ok(())
	}

	async fn get_stats(&self) -> PluginResult<StorageStats> {
		let data = self
			.data
			.read()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire read lock: {}", e)))?;

		let total_keys = data.len() as u64;
		let total_size_bytes: u64 = data.values().map(|(bytes, _)| bytes.len() as u64).sum();

		Ok(StorageStats {
			total_keys,
			total_size_bytes,
			memory_usage_bytes: Some(total_size_bytes),
			hit_rate: None,
			operations_count: HashMap::new(),
		})
	}

	async fn cleanup(&self) -> PluginResult<CleanupStats> {
		let start = SystemTime::now();
		let mut data = self
			.data
			.write()
			.map_err(|e| PluginError::StateError(format!("Failed to acquire write lock: {}", e)))?;

		let now = SystemTime::now();
		let mut keys_removed = 0;
		let mut bytes_freed = 0;

		// Collect expired keys
		let expired_keys: Vec<String> = data
			.iter()
			.filter_map(|(k, (v, expiry))| {
				if let Some(exp) = expiry {
					if now > *exp {
						bytes_freed += v.len() as u64;
						keys_removed += 1;
						Some(k.clone())
					} else {
						None
					}
				} else {
					None
				}
			})
			.collect();

		// Remove expired entries
		for key in expired_keys {
			data.remove(&key);
		}

		let duration = start.elapsed().unwrap_or_default();

		Ok(CleanupStats {
			keys_removed,
			bytes_freed,
			duration_ms: duration.as_millis() as u64,
		})
	}
}
