//! # State Plugin Types
//!
//! Defines types and traits for persistent state management.
//!
//! This module provides the infrastructure for plugins that handle state
//! persistence across different backends including memory, file systems,
//! databases, and distributed caches.

use crate::PluginConfig;

use super::{BasePlugin, PluginResult, Timestamp};
use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::time::Duration;

/// Core state store interface for key-value operations.
///
/// Provides a generic interface for persisting solver state across
/// different storage backends with support for TTL, batching, and
/// atomic operations.
#[async_trait]
pub trait StateStore: Send + Sync + Debug {
	/// Get value by key.
	///
	/// Returns the stored value if the key exists, None otherwise.
	async fn get(&self, key: &str) -> PluginResult<Option<Bytes>>;

	/// Set value for key.
	///
	/// Stores or updates the value associated with the given key.
	async fn set(&self, key: &str, value: Bytes) -> PluginResult<()>;

	/// Set value with TTL (time to live).
	///
	/// Stores a value that will automatically expire after the
	/// specified duration.
	async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()>;

	/// Delete key.
	///
	/// Removes the key and its associated value from storage.
	async fn delete(&self, key: &str) -> PluginResult<()>;

	/// Check if key exists.
	///
	/// Returns true if the key exists in storage, false otherwise.
	async fn exists(&self, key: &str) -> PluginResult<bool>;

	/// List keys with optional prefix filter.
	///
	/// Returns all keys in storage, optionally filtered by prefix.
	async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>>;

	/// Batch get operation for efficiency.
	///
	/// Retrieves multiple values in a single operation.
	async fn batch_get(&self, keys: &[String]) -> PluginResult<Vec<Option<Bytes>>>;
	
	/// Batch set operation for efficiency.
	///
	/// Stores multiple key-value pairs in a single operation.
	async fn batch_set(&self, items: &[(String, Bytes)]) -> PluginResult<()>;
	
	/// Batch delete operation for efficiency.
	///
	/// Removes multiple keys in a single operation.
	async fn batch_delete(&self, keys: &[String]) -> PluginResult<()>;

	/// Atomic update operation.
	///
	/// Updates a value atomically using the provided function.
	/// If the function returns None, the key is deleted.
	async fn atomic_update(
		&self,
		key: &str,
		updater: Box<dyn FnOnce(Option<Bytes>) -> PluginResult<Option<Bytes>> + Send>,
	) -> PluginResult<()>;

	/// Get storage statistics.
	///
	/// Returns current storage usage and performance metrics.
	async fn get_stats(&self) -> PluginResult<StorageStats>;

	/// Cleanup expired entries.
	///
	/// Removes expired entries for stores that support TTL.
	async fn cleanup(&self) -> PluginResult<CleanupStats>;
}

/// Storage statistics.
///
/// Provides metrics about storage usage and performance
/// for monitoring and optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
	/// Total number of keys in storage
	pub total_keys: u64,
	/// Total size of stored data in bytes
	pub total_size_bytes: u64,
	/// Memory usage in bytes (if available)
	pub memory_usage_bytes: Option<u64>,
	/// Cache hit rate if applicable (0.0 to 1.0)
	pub hit_rate: Option<f64>,                  // Cache hit rate if applicable
	/// Operation counts by type (get, set, delete)
	pub operations_count: HashMap<String, u64>, // get, set, delete counts
}

/// Cleanup operation statistics.
///
/// Reports the results of a cleanup operation including
/// the amount of space reclaimed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupStats {
	/// Number of keys removed during cleanup
	pub keys_removed: u64,
	/// Amount of storage space freed in bytes
	pub bytes_freed: u64,
	/// Time taken for cleanup in milliseconds
	pub duration_ms: u64,
}

/// State plugin interface extending the base plugin.
///
/// Core trait that state plugins must implement to integrate with
/// the solver's state management system. Provides factory methods
/// and backend capabilities.
#[async_trait]
pub trait StatePlugin: BasePlugin {
	/// Get the backend type identifier.
	///
	/// Returns a string identifying the storage backend type
	/// (e.g., "memory", "file", "redis").
	fn backend_type(&self) -> &'static str;

	/// Create a new store instance.
	///
	/// Instantiates a new state store with the plugin's configuration.
	async fn create_store(&self) -> PluginResult<Box<dyn StateStore>>;

	/// Check if the backend supports TTL.
	///
	/// Returns true if the backend can automatically expire keys.
	fn supports_ttl(&self) -> bool;

	/// Check if the backend supports transactions.
	///
	/// Returns true if the backend supports transactional operations.
	fn supports_transactions(&self) -> bool;

	/// Check if the backend supports atomic operations.
	///
	/// Returns true if the backend can perform atomic updates.
	fn supports_atomic_operations(&self) -> bool;

	/// Get backend-specific configuration.
	///
	/// Returns detailed configuration and capability information.
	async fn get_backend_config(&self) -> PluginResult<BackendConfig>;

	/// Optimize the backend storage.
	///
	/// Performs backend-specific optimization like defragmentation
	/// or compaction.
	async fn optimize(&self) -> PluginResult<OptimizationResult>;

	/// Create a backup of the state.
	///
	/// Exports the current state to the specified destination.
	async fn backup(&self, destination: &str) -> PluginResult<BackupResult>;

	/// Restore state from backup.
	///
	/// Imports state from a previously created backup.
	async fn restore(&self, source: &str) -> PluginResult<RestoreResult>;
}

/// Backend configuration information.
///
/// Describes the capabilities and configuration of a
/// state storage backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
	/// Type of storage backend
	pub backend_type: String,
	/// Backend version string
	pub version: String,
	/// List of supported features
	pub features: Vec<String>,
	/// Operational limits of the backend
	pub limits: BackendLimits,
	/// Backend-specific configuration settings
	pub settings: HashMap<String, String>,
}

/// Backend operational limits.
///
/// Defines the constraints and limitations of a storage
/// backend to prevent misuse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendLimits {
	/// Maximum key size in bytes
	pub max_key_size: Option<usize>,
	/// Maximum value size in bytes
	pub max_value_size: Option<usize>,
	/// Maximum number of keys allowed
	pub max_keys: Option<u64>,
	/// Maximum total storage size in bytes
	pub max_storage_size: Option<u64>,
	/// Maximum TTL duration in seconds
	pub max_ttl: Option<u64>, // seconds
}

/// Optimization operation result.
///
/// Reports the outcome of a storage optimization operation
/// including space savings and performance improvements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationResult {
	/// Type of optimization performed
	pub optimization_type: String,
	/// Storage space freed in bytes
	pub bytes_freed: u64,
	/// Time taken for optimization in milliseconds
	pub time_taken_ms: u64,
	/// Performance improvement as a percentage
	pub performance_improvement: Option<f64>, // percentage
}

/// Backup operation result.
///
/// Contains information about a completed backup operation
/// including location and verification data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
	/// Unique identifier for this backup
	pub backup_id: String,
	/// Location where backup was stored
	pub destination: String,
	/// Size of the backup in bytes
	pub size_bytes: u64,
	/// Number of keys included in backup
	pub keys_backed_up: u64,
	/// Timestamp when backup was created
	pub timestamp: Timestamp,
	/// Checksum for backup verification
	pub checksum: Option<String>,
}

/// Restore operation result.
///
/// Reports the outcome of a state restoration from backup
/// including any conflicts encountered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
	/// Source location of the backup
	pub source: String,
	/// Number of keys restored
	pub keys_restored: u64,
	/// Total bytes of data restored
	pub bytes_restored: u64,
	/// Timestamp when restore completed
	pub timestamp: Timestamp,
	/// Number of conflicts resolved during restore
	pub conflicts_resolved: u64,
}

/// Features that state backends can support.
///
/// Enumerates advanced capabilities that state backends
/// may provide beyond basic key-value operations.
#[derive(Debug, Clone, PartialEq, Hash, Eq, Serialize, Deserialize)]
pub enum StateFeature {
	/// Support for automatic key expiration
	TTL,
	/// Support for transactional operations
	Transactions,
	/// Support for atomic read-modify-write
	AtomicOperations,
	/// Ability to create backups
	Backup,
	/// Ability to restore from backups
	Restore,
	/// Data compression support
	Compression,
	/// Data encryption at rest
	Encryption,
	/// Multi-node replication
	Replication,
	/// Data sharding across nodes
	Sharding,
	/// Secondary index support
	Indexing,
	/// Advanced query capabilities
	QueryBuilder,
}

/// Type-safe wrapper for storing specific data types.
///
/// Provides a strongly-typed interface over the generic StateStore,
/// handling serialization and deserialization automatically.
pub struct TypedStateStore<T>
where
	T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
	/// Underlying state store
	store: Box<dyn StateStore>,
	/// Key prefix for namespacing
	prefix: String,
	/// Phantom data for type safety
	_phantom: std::marker::PhantomData<T>,
}

impl<T> TypedStateStore<T>
where
	T: Serialize + for<'de> Deserialize<'de> + Send + Sync,
{
	/// Create a new typed state store with the given prefix.
	pub fn new(store: Box<dyn StateStore>, prefix: impl Into<String>) -> Self {
		Self {
			store,
			prefix: prefix.into(),
			_phantom: std::marker::PhantomData,
		}
	}

	/// Create a prefixed key for namespacing.
	fn make_key(&self, key: &str) -> String {
		format!("{}:{}", self.prefix, key)
	}

	/// Get a typed value by key.
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

	/// Set a typed value for the given key.
	pub async fn set(&self, key: &str, value: &T) -> PluginResult<()> {
		let full_key = self.make_key(key);
		let bytes = serde_json::to_vec(value)
			.map_err(|e| super::PluginError::StateError(format!("Serialization failed: {}", e)))?;
		self.store.set(&full_key, bytes.into()).await
	}

	/// Set a typed value with TTL.
	pub async fn set_with_ttl(&self, key: &str, value: &T, ttl: Duration) -> PluginResult<()> {
		let full_key = self.make_key(key);
		let bytes = serde_json::to_vec(value)
			.map_err(|e| super::PluginError::StateError(format!("Serialization failed: {}", e)))?;
		self.store.set_with_ttl(&full_key, bytes.into(), ttl).await
	}

	/// Delete a key from the store.
	pub async fn delete(&self, key: &str) -> PluginResult<()> {
		let full_key = self.make_key(key);
		self.store.delete(&full_key).await
	}

	/// List all keys in this typed store.
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

	/// Perform an atomic update on a typed value.
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

/// Factory trait for creating state plugins.
///
/// Provides a standardized interface for instantiating state
/// plugins with configuration and capability reporting.
pub trait StatePluginFactory: Send + Sync {
	/// Create a new instance of the state plugin with configuration.
	fn create_plugin(&self, config: PluginConfig) -> PluginResult<Box<dyn StatePlugin>>;
	
	/// Get the unique type identifier for this plugin factory.
	fn plugin_type(&self) -> &'static str;
	
	/// Get the list of features this state backend supports.
	fn supports_features(&self) -> Vec<StateFeature>;
}
