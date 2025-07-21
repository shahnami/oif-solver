// solver-types/src/plugins/state.rs

use crate::PluginConfig;

use super::{BasePlugin, PluginResult, Timestamp};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

/// Core state store interface - simple key-value operations
#[async_trait]
pub trait StateStore: Send + Sync + Debug {
	/// Get value by key
	async fn get(&self, key: &str) -> PluginResult<Option<Bytes>>;

	/// Set value for key
	async fn set(&self, key: &str, value: Bytes) -> PluginResult<()>;

	/// Set value with TTL (time to live)
	async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()>;

	/// Delete key
	async fn delete(&self, key: &str) -> PluginResult<()>;

	/// Check if key exists
	async fn exists(&self, key: &str) -> PluginResult<bool>;

	/// List keys with optional prefix filter
	async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>>;

	/// Batch operations for efficiency
	async fn batch_get(&self, keys: &[String]) -> PluginResult<Vec<Option<Bytes>>>;
	async fn batch_set(&self, items: &[(String, Bytes)]) -> PluginResult<()>;
	async fn batch_delete(&self, keys: &[String]) -> PluginResult<()>;

	/// Atomic update operation
	/// The updater function takes the current value and returns the new value
	/// If None is returned, the key is deleted
	async fn atomic_update(
		&self,
		key: &str,
		updater: Box<dyn FnOnce(Option<Bytes>) -> PluginResult<Option<Bytes>> + Send>,
	) -> PluginResult<()>;

	/// Get storage statistics
	async fn get_stats(&self) -> PluginResult<StorageStats>;

	/// Cleanup expired entries (for stores that support TTL)
	async fn cleanup(&self) -> PluginResult<CleanupStats>;
}

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
	pub total_keys: u64,
	pub total_size_bytes: u64,
	pub memory_usage_bytes: Option<u64>,
	pub hit_rate: Option<f64>,                  // Cache hit rate if applicable
	pub operations_count: HashMap<String, u64>, // get, set, delete counts
}

/// Cleanup operation statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupStats {
	pub keys_removed: u64,
	pub bytes_freed: u64,
	pub duration_ms: u64,
}

/// State plugin interface extending the base plugin
#[async_trait]
pub trait StatePlugin: BasePlugin {
	/// Get the backend type (memory, file, redis, etc.)
	fn backend_type(&self) -> &'static str;

	/// Create a new store instance
	async fn create_store(&self) -> PluginResult<Box<dyn StateStore>>;

	/// Check if the backend supports TTL
	fn supports_ttl(&self) -> bool;

	/// Check if the backend supports transactions
	fn supports_transactions(&self) -> bool;

	/// Check if the backend supports atomic operations
	fn supports_atomic_operations(&self) -> bool;

	/// Get backend-specific configuration
	async fn get_backend_config(&self) -> PluginResult<BackendConfig>;

	/// Optimize the backend (defrag, compact, etc.)
	async fn optimize(&self) -> PluginResult<OptimizationResult>;

	/// Create a backup of the state
	async fn backup(&self, destination: &str) -> PluginResult<BackupResult>;

	/// Restore state from backup
	async fn restore(&self, source: &str) -> PluginResult<RestoreResult>;
}

/// Backend configuration information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
	pub backend_type: String,
	pub version: String,
	pub features: Vec<String>,
	pub limits: BackendLimits,
	pub settings: HashMap<String, String>,
}

/// Backend operational limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendLimits {
	pub max_key_size: Option<usize>,
	pub max_value_size: Option<usize>,
	pub max_keys: Option<u64>,
	pub max_storage_size: Option<u64>,
	pub max_ttl: Option<u64>, // seconds
}

/// Optimization operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
	pub optimization_type: String,
	pub bytes_freed: u64,
	pub time_taken_ms: u64,
	pub performance_improvement: Option<f64>, // percentage
}

/// Backup operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
	pub backup_id: String,
	pub destination: String,
	pub size_bytes: u64,
	pub keys_backed_up: u64,
	pub timestamp: Timestamp,
	pub checksum: Option<String>,
}

/// Restore operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
	pub source: String,
	pub keys_restored: u64,
	pub bytes_restored: u64,
	pub timestamp: Timestamp,
	pub conflicts_resolved: u64,
}

/// Features that state backends can support
#[derive(Debug, Clone, PartialEq, Hash, Eq, Serialize, Deserialize)]
pub enum StateFeature {
	TTL,
	Transactions,
	AtomicOperations,
	Backup,
	Restore,
	Compression,
	Encryption,
	Replication,
	Sharding,
	Indexing,
	QueryBuilder,
}

/// Type-safe wrapper for storing specific data types
pub struct TypedStateStore<T>
where
	T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
	store: Box<dyn StateStore>,
	prefix: String,
	_phantom: std::marker::PhantomData<T>,
}

impl<T> TypedStateStore<T>
where
	T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
	pub fn new(store: Box<dyn StateStore>, prefix: impl Into<String>) -> Self {
		Self {
			store,
			prefix: prefix.into(),
			_phantom: std::marker::PhantomData,
		}
	}

	fn make_key(&self, key: &str) -> String {
		format!("{}:{}", self.prefix, key)
	}

	pub async fn get(&self, key: &str) -> PluginResult<Option<T>> {
		let full_key = self.make_key(key);
		match self.store.get(&full_key).await? {
			Some(bytes) => {
				let value: T = serde_json::from_slice(&bytes).map_err(|e| {
					super::PluginError::StateError(format!("Deserialization failed: {}", e))
				})?;
				Ok(Some(value))
			}
			None => Ok(None),
		}
	}

	pub async fn set(&self, key: &str, value: &T) -> PluginResult<()> {
		let full_key = self.make_key(key);
		let bytes = serde_json::to_vec(value)
			.map_err(|e| super::PluginError::StateError(format!("Serialization failed: {}", e)))?;
		self.store.set(&full_key, bytes.into()).await
	}

	pub async fn set_with_ttl(&self, key: &str, value: &T, ttl: Duration) -> PluginResult<()> {
		let full_key = self.make_key(key);
		let bytes = serde_json::to_vec(value)
			.map_err(|e| super::PluginError::StateError(format!("Serialization failed: {}", e)))?;
		self.store.set_with_ttl(&full_key, bytes.into(), ttl).await
	}

	pub async fn delete(&self, key: &str) -> PluginResult<()> {
		let full_key = self.make_key(key);
		self.store.delete(&full_key).await
	}

	pub async fn list_keys(&self) -> PluginResult<Vec<String>> {
		let keys = self.store.list_keys(Some(&self.prefix)).await?;
		Ok(keys
			.into_iter()
			.filter_map(|k| {
				k.strip_prefix(&format!("{}:", self.prefix))
					.map(|s| s.to_string())
			})
			.collect())
	}

	pub async fn atomic_update<F>(&self, key: &str, updater: F) -> PluginResult<()>
	where
		F: FnOnce(Option<T>) -> PluginResult<Option<T>> + Send + 'static,
		T: 'static,
	{
		let full_key = self.make_key(key);

		// Create a boxed closure that handles serialization/deserialization
		let boxed_updater = Box::new(
			move |current_bytes: Option<Bytes>| -> PluginResult<Option<Bytes>> {
				// Deserialize current value if it exists
				let current_value = match current_bytes {
					Some(bytes) => {
						let value: T = serde_json::from_slice(&bytes).map_err(|e| {
							super::PluginError::StateError(format!("Deserialization failed: {}", e))
						})?;
						Some(value)
					}
					None => None,
				};

				// Call the user's updater
				let new_value = updater(current_value)?;

				// Serialize new value if it exists
				match new_value {
					Some(value) => {
						let bytes = serde_json::to_vec(&value).map_err(|e| {
							super::PluginError::StateError(format!("Serialization failed: {}", e))
						})?;
						Ok(Some(bytes.into()))
					}
					None => Ok(None),
				}
			},
		);

		self.store.atomic_update(&full_key, boxed_updater).await
	}
}

/// Factory trait for creating state plugins
pub trait StatePluginFactory: Send + Sync {
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn StatePlugin>>;
	fn plugin_type(&self) -> &'static str;
	fn supports_features(&self) -> Vec<StateFeature>;
}
