//! # State Service
//!
//! Provides persistent state management with pluggable storage backends.
//!
//! This crate offers a unified state management service that can use various
//! storage backends through a plugin system. It supports operations like
//! key-value storage, batch operations, TTL management, atomic updates,
//! and background cleanup processes.
//!
//! ## Features
//!
//! - **Multi-Backend Support**: File, memory, and database storage options
//! - **TTL Management**: Automatic expiration of time-sensitive data
//! - **Batch Operations**: Efficient bulk operations for performance
//! - **Atomic Updates**: Safe concurrent modifications
//! - **Background Cleanup**: Automatic expired data removal
//! - **Health Monitoring**: Service and plugin health checks
//! - **Metrics Collection**: Performance and usage statistics

use bytes::Bytes;
use solver_types::configs::StateConfig;
use solver_types::plugins::{
	BackendConfig, CleanupStats, PluginError, PluginResult, StatePlugin, StateStore, StorageStats,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// State service that orchestrates multiple storage backend plugins.
///
/// The state service provides a unified interface to various storage backends
/// while managing plugin lifecycle, backend switching, and background operations.
/// It supports both synchronous and asynchronous operations with comprehensive
/// error handling and monitoring capabilities.
#[derive(Debug)]
pub struct StateService {
	/// Registry of available state plugins by name
	plugins: Arc<RwLock<HashMap<String, Arc<dyn StatePlugin>>>>,
	/// Currently active backend plugin name
	active_backend: Arc<RwLock<Option<String>>>,
	/// Active storage instance for operations
	active_store: Arc<RwLock<Option<Arc<dyn StateStore>>>>,
	/// Service configuration including backend preferences
	config: StateConfig,
}

impl StateService {
	/// Create a new state service with the provided configuration.
	///
	/// Initializes all internal data structures but does not activate any
	/// backend plugins. Use `initialize()` to activate the default backend
	/// or `activate_backend()` to activate a specific backend.
	///
	/// # Arguments
	/// * `config` - State service configuration including backend preferences
	pub fn new(config: StateConfig) -> Self {
		Self {
			plugins: Arc::new(RwLock::new(HashMap::new())),
			active_backend: Arc::new(RwLock::new(None)),
			active_store: Arc::new(RwLock::new(None)),
			config,
		}
	}

	/// Configure the default backend for this service instance.
	///
	/// Updates the service configuration to use the specified backend as
	/// the default when `initialize()` is called. This is a builder method
	/// for fluent configuration.
	///
	/// # Arguments
	/// * `backend` - Name of the backend plugin to use as default
	pub fn with_default_backend(mut self, backend: String) -> Self {
		self.config.default_backend = backend;
		self
	}

	/// Configure the cleanup interval for expired data removal.
	///
	/// Sets how frequently the background cleanup task runs to remove
	/// expired entries. This is a builder method for fluent configuration.
	///
	/// # Arguments
	/// * `interval_seconds` - Cleanup interval in seconds
	pub fn with_cleanup_interval(mut self, interval_seconds: u64) -> Self {
		self.config.cleanup_interval_seconds = interval_seconds;
		self
	}

	/// Register a new state plugin with the service.
	///
	/// Adds the plugin to the registry, making it available for activation.
	/// The plugin must be initialized before registration. Registration
	/// does not automatically activate the backend.
	///
	/// # Arguments
	/// * `name` - Unique name to identify the plugin
	/// * `plugin` - Initialized state plugin instance
	pub async fn register_plugin(&self, name: String, plugin: Arc<dyn StatePlugin>) {
		info!("Registering state plugin: {}", name);
		self.plugins.write().await.insert(name, plugin);
	}

	/// Activate a specific backend plugin for state operations.
	///
	/// Creates a storage instance from the specified plugin and makes it
	/// the active backend for all state operations. Any previously active
	/// backend is replaced. The plugin must be registered before activation.
	///
	/// # Arguments
	/// * `backend_name` - Name of the registered plugin to activate
	///
	/// # Returns
	/// Success if the backend is activated, error if plugin not found or activation fails
	///
	/// # Errors
	/// Returns error if the plugin is not registered or store creation fails
	pub async fn activate_backend(&self, backend_name: &str) -> PluginResult<()> {
		let plugins = self.plugins.read().await;
		let plugin = plugins.get(backend_name).ok_or_else(|| {
			PluginError::NotFound(format!("State plugin not found: {}", backend_name))
		})?;

		// Create store from plugin
		let store = plugin.create_store().await?;

		// Update active backend and store
		*self.active_backend.write().await = Some(backend_name.to_string());
		*self.active_store.write().await = Some(Arc::from(store));

		info!("{} started successfully", backend_name);
		Ok(())
	}

	/// Get the name of the currently active backend plugin.
	///
	/// Returns the name of the backend plugin that is currently handling
	/// state operations, or None if no backend is currently active.
	///
	/// # Returns
	/// The active backend name, or None if no backend is active
	pub async fn get_active_backend(&self) -> Option<String> {
		self.active_backend.read().await.clone()
	}

	/// Switch to a different backend plugin.
	///
	/// Deactivates the current backend (if any) and activates the specified
	/// backend plugin. This operation may involve cleanup of the previous
	/// backend and initialization of the new one.
	///
	/// # Arguments
	/// * `backend_name` - Name of the backend plugin to switch to
	///
	/// # Returns
	/// Success if the backend switch completes, error if the new backend fails to activate
	///
	/// # Errors
	/// Returns error if the target plugin is not registered or activation fails
	pub async fn switch_backend(&self, backend_name: &str) -> PluginResult<()> {
		info!("Switching state backend to: {}", backend_name);

		// Cleanup current backend if any
		if let Some(current_backend) = self.get_active_backend().await {
			info!("Cleaning up current backend: {}", current_backend);
			// TODO: Add proper cleanup logic
		}

		self.activate_backend(backend_name).await
	}

	/// Get the active storage instance or return an error.
	///
	/// Internal helper method that retrieves the currently active storage
	/// instance. Returns an error if no backend is currently active.
	///
	/// # Returns
	/// The active storage instance wrapped in Arc
	///
	/// # Errors
	/// Returns error if no backend is currently active
	async fn get_active_store(&self) -> PluginResult<Arc<dyn StateStore>> {
		self.active_store
			.read()
			.await
			.clone()
			.ok_or_else(|| PluginError::ExecutionFailed("No active state backend".to_string()))
	}

	/// Retrieve a value by its key from the active storage backend.
	///
	/// Fetches the value associated with the specified key from the currently
	/// active storage backend. Returns None if the key does not exist or has expired.
	///
	/// # Arguments
	/// * `key` - The key to retrieve the value for
	///
	/// # Returns
	/// The value as bytes if found, None if not found, or error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or storage operation fails
	pub async fn get(&self, key: &str) -> PluginResult<Option<Bytes>> {
		let store = self.get_active_store().await?;
		debug!("Getting key: {}", key);
		store.get(key).await
	}

	/// Store a value with the specified key in the active storage backend.
	///
	/// Saves the provided value under the given key in the currently active
	/// storage backend. If the key already exists, its value is replaced.
	///
	/// # Arguments
	/// * `key` - The key to store the value under
	/// * `value` - The value to store as bytes
	///
	/// # Returns
	/// Success if the value is stored, error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or storage operation fails
	pub async fn set(&self, key: &str, value: Bytes) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Setting key: {}", key);
		store.set(key, value).await
	}

	/// Store a value with an expiration time (TTL).
	///
	/// Saves the provided value under the given key with a time-to-live
	/// duration. The value will be automatically removed after the TTL expires.
	///
	/// # Arguments
	/// * `key` - The key to store the value under
	/// * `value` - The value to store as bytes
	/// * `ttl` - Time-to-live duration for the key
	///
	/// # Returns
	/// Success if the value is stored with TTL, error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or storage operation fails
	pub async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Setting key with TTL: {} ({}s)", key, ttl.as_secs());
		store.set_with_ttl(key, value, ttl).await
	}

	/// Delete a key and its value from the active storage backend.
	///
	/// Removes the specified key and its associated value from the currently
	/// active storage backend. No error is returned if the key does not exist.
	///
	/// # Arguments
	/// * `key` - The key to delete
	///
	/// # Returns
	/// Success if the deletion operation completes, error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or storage operation fails
	pub async fn delete(&self, key: &str) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Deleting key: {}", key);
		store.delete(key).await
	}

	/// Check if a key exists in the active storage backend.
	///
	/// Tests whether the specified key exists in the currently active storage
	/// backend, regardless of its value. Expired keys are considered non-existent.
	///
	/// # Arguments
	/// * `key` - The key to check for existence
	///
	/// # Returns
	/// True if the key exists, false if it doesn't, or error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or storage operation fails
	pub async fn exists(&self, key: &str) -> PluginResult<bool> {
		let store = self.get_active_store().await?;
		debug!("Checking existence of key: {}", key);
		store.exists(key).await
	}

	/// List all keys or keys with a specific prefix.
	///
	/// Retrieves a list of all keys in the storage backend, optionally filtered
	/// by a prefix. Only non-expired keys are included in the results.
	///
	/// # Arguments
	/// * `prefix` - Optional prefix to filter keys, or None to list all keys
	///
	/// # Returns
	/// Vector of key names matching the criteria, or error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or storage operation fails
	pub async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>> {
		let store = self.get_active_store().await?;
		debug!("Listing keys with prefix: {:?}", prefix);
		store.list_keys(prefix).await
	}

	/// Retrieve multiple values in a single batch operation.
	///
	/// Efficiently fetches the values for multiple keys in one operation,
	/// which may be faster than individual get operations depending on the
	/// storage backend implementation.
	///
	/// # Arguments
	/// * `keys` - Slice of key names to retrieve values for
	///
	/// # Returns
	/// Vector of optional values corresponding to each key, in the same order
	///
	/// # Errors
	/// Returns error if no backend is active or batch operation fails
	pub async fn batch_get(&self, keys: &[String]) -> PluginResult<Vec<Option<Bytes>>> {
		let store = self.get_active_store().await?;
		debug!("Batch getting {} keys", keys.len());
		store.batch_get(keys).await
	}

	/// Store multiple key-value pairs in a single batch operation.
	///
	/// Efficiently saves multiple key-value pairs in one operation, which
	/// may provide better performance than individual set operations.
	/// All operations in the batch are typically atomic.
	///
	/// # Arguments
	/// * `items` - Slice of key-value pairs to store
	///
	/// # Returns
	/// Success if all items are stored, error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or batch operation fails
	pub async fn batch_set(&self, items: &[(String, Bytes)]) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Batch setting {} items", items.len());
		store.batch_set(items).await
	}

	/// Delete multiple keys in a single batch operation.
	///
	/// Efficiently removes multiple keys and their values in one operation,
	/// which may be faster than individual delete operations. Non-existent
	/// keys are ignored without causing errors.
	///
	/// # Arguments
	/// * `keys` - Slice of key names to delete
	///
	/// # Returns
	/// Success if batch deletion completes, error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or batch operation fails
	pub async fn batch_delete(&self, keys: &[String]) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Batch deleting {} keys", keys.len());
		store.batch_delete(keys).await
	}

	/// Perform an atomic update operation on a key's value.
	///
	/// Executes a user-defined update function atomically, ensuring that
	/// the read-modify-write operation is thread-safe. The updater function
	/// receives the current value (if any) and returns the new value to store.
	///
	/// # Arguments
	/// * `key` - The key to update
	/// * `updater` - Function that transforms the current value to a new value
	///
	/// # Returns
	/// Success if the atomic update completes, error on failure
	///
	/// # Errors
	/// Returns error if no backend is active, update function fails, or storage operation fails
	pub async fn atomic_update(
		&self,
		key: &str,
		updater: Box<dyn FnOnce(Option<Bytes>) -> PluginResult<Option<Bytes>> + Send>,
	) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Atomic update for key: {}", key);
		store.atomic_update(key, updater).await
	}

	/// Get storage statistics from the active backend.
	///
	/// Retrieves usage and performance statistics from the currently active
	/// storage backend, including metrics like total entries, memory usage,
	/// and operation counters.
	///
	/// # Returns
	/// Storage statistics structure with current metrics, or error on failure
	///
	/// # Errors
	/// Returns error if no backend is active or statistics retrieval fails
	pub async fn get_stats(&self) -> PluginResult<StorageStats> {
		let store = self.get_active_store().await?;
		debug!("Getting storage statistics");
		store.get_stats().await
	}

	/// Clean up expired entries in the active storage backend.
	///
	/// Manually triggers cleanup of expired entries, freeing storage space
	/// and removing stale data. Returns statistics about the cleanup operation
	/// including how many keys were removed and bytes freed.
	///
	/// # Returns
	/// Cleanup statistics with counts of removed keys and freed bytes
	///
	/// # Errors
	/// Returns error if no backend is active or cleanup operation fails
	pub async fn cleanup(&self) -> PluginResult<CleanupStats> {
		let store = self.get_active_store().await?;
		debug!("Running cleanup operation");
		let stats = store.cleanup().await?;
		debug!(
			"Cleanup completed: {} keys removed, {} bytes freed",
			stats.keys_removed, stats.bytes_freed
		);
		Ok(stats)
	}

	/// Get configuration information for the active backend plugin.
	///
	/// Retrieves backend-specific configuration details from the currently
	/// active storage plugin, including supported features and operational
	/// parameters.
	///
	/// # Returns
	/// Backend configuration structure with plugin details
	///
	/// # Errors
	/// Returns error if no backend is active or configuration retrieval fails
	pub async fn get_backend_config(&self) -> PluginResult<BackendConfig> {
		let backend_name = self
			.get_active_backend()
			.await
			.ok_or_else(|| PluginError::ExecutionFailed("No active backend".to_string()))?;

		let plugins = self.plugins.read().await;
		let plugin = plugins
			.get(&backend_name)
			.ok_or_else(|| PluginError::NotFound(format!("Plugin not found: {}", backend_name)))?;

		plugin.get_backend_config().await
	}

	/// List the names of all registered state plugins.
	///
	/// Returns a vector of plugin names that are currently registered
	/// with the service and available for activation.
	///
	/// # Returns
	/// Vector of registered plugin names
	pub async fn list_plugins(&self) -> Vec<String> {
		self.plugins.read().await.keys().cloned().collect()
	}

	/// Check if a specific plugin is registered with the service.
	///
	/// Tests whether a plugin with the given name is available for
	/// activation. Registration does not imply the plugin is active.
	///
	/// # Arguments
	/// * `name` - Name of the plugin to check
	///
	/// # Returns
	/// True if the plugin is registered, false otherwise
	pub async fn has_plugin(&self, name: &str) -> bool {
		self.plugins.read().await.contains_key(name)
	}

	/// Perform health checks on all registered state plugins.
	///
	/// Executes health check operations on all registered plugins to assess
	/// their operational status. Returns a map of plugin names to their health
	/// status, including any error information for unhealthy plugins.
	///
	/// # Returns
	/// Map of plugin names to their health status information
	///
	/// # Errors
	/// Individual plugin health check failures are captured in the health status,
	/// this method only fails for system-level errors
	pub async fn health_check(
		&self,
	) -> PluginResult<HashMap<String, solver_types::plugins::PluginHealth>> {
		let all_plugins = self.plugins.read().await;
		let mut health_status = HashMap::new();

		for (plugin_name, plugin) in all_plugins.iter() {
			match plugin.health_check().await {
				Ok(health) => {
					health_status.insert(plugin_name.clone(), health);
				}
				Err(error) => {
					health_status.insert(
						plugin_name.clone(),
						solver_types::plugins::PluginHealth::unhealthy(format!(
							"Health check failed: {}",
							error
						)),
					);
				}
			}
		}

		Ok(health_status)
	}

	/// Collect performance metrics from all registered state plugins.
	///
	/// Gathers operational metrics from all registered plugins, including
	/// performance counters, usage statistics, and resource consumption data.
	/// Plugins that fail to provide metrics are logged but do not fail the operation.
	///
	/// # Returns
	/// Map of plugin names to their performance metrics
	///
	/// # Errors
	/// Returns error only for system-level failures, individual plugin
	/// metric collection failures are logged and skipped
	pub async fn get_metrics(
		&self,
	) -> PluginResult<HashMap<String, solver_types::plugins::PluginMetrics>> {
		let all_plugins = self.plugins.read().await;
		let mut metrics = HashMap::new();

		for (plugin_name, plugin) in all_plugins.iter() {
			match plugin.get_metrics().await {
				Ok(plugin_metrics) => {
					metrics.insert(plugin_name.clone(), plugin_metrics);
				}
				Err(error) => {
					warn!(
						"Failed to get metrics from plugin {}: {}",
						plugin_name, error
					);
				}
			}
		}

		Ok(metrics)
	}

	/// Initialize the service by activating the default backend.
	///
	/// Attempts to activate the backend specified in the service configuration
	/// as the default backend. If the default backend is not registered, a warning
	/// is logged but no error is returned, leaving the service without an active backend.
	///
	/// # Returns
	/// Success if the default backend is activated or not found, error if activation fails
	///
	/// # Errors
	/// Returns error if the default backend is registered but activation fails
	pub async fn initialize(&self) -> PluginResult<()> {
		let default_backend = self.config.default_backend.clone();

		if self.has_plugin(&default_backend).await {
			info!("Starting {}", default_backend);
			self.activate_backend(&default_backend).await?;
		} else {
			warn!(
				"Default backend '{}' not found, no backend activated",
				default_backend
			);
		}

		Ok(())
	}

	/// Start a background task for automatic cleanup of expired entries.
	///
	/// Spawns a background tokio task that periodically runs cleanup operations
	/// on the active storage backend based on the configured cleanup interval.
	/// The task runs until the service is dropped or the application exits.
	pub async fn start_cleanup_task(&self) {
		let cleanup_interval = Duration::from_secs(self.config.cleanup_interval_seconds);
		let service = self.clone();

		tokio::spawn(async move {
			let mut interval = tokio::time::interval(cleanup_interval);
			loop {
				interval.tick().await;

				if let Err(e) = service.cleanup().await {
					error!("Background cleanup failed: {}", e);
				}
			}
		});
	}
}

impl Clone for StateService {
	fn clone(&self) -> Self {
		Self {
			plugins: Arc::clone(&self.plugins),
			active_backend: Arc::clone(&self.active_backend),
			active_store: Arc::clone(&self.active_store),
			config: self.config.clone(),
		}
	}
}

/// Builder for constructing StateService instances with plugins and configuration.
///
/// Provides a fluent interface for configuring state services with multiple
/// plugins and custom settings. Handles plugin initialization during the build
/// process and provides convenient configuration methods.
pub struct StateServiceBuilder {
	/// List of plugins to register with their configurations
	plugins: Vec<(
		String,
		Arc<dyn StatePlugin>,
		solver_types::plugins::PluginConfig,
	)>,
	/// Service configuration settings
	config: StateConfig,
}

impl StateServiceBuilder {
	/// Create a new state service builder with default configuration.
	///
	/// Initializes the builder with sensible defaults including memory backend,
	/// enabled metrics, 5-minute cleanup interval, and 100 max concurrent operations.
	pub fn new() -> Self {
		Self {
			plugins: Vec::new(),
			config: StateConfig {
				default_backend: "memory".to_string(),
				enable_metrics: true,
				cleanup_interval_seconds: 300,
				max_concurrent_operations: 100,
			},
		}
	}

	/// Add a state plugin to be registered with the service.
	///
	/// Registers a plugin with the builder to be initialized and registered
	/// during the build process. Plugins are registered in the order they are added.
	///
	/// # Arguments
	/// * `name` - Unique name for the plugin
	/// * `plugin` - State plugin instance
	/// * `config` - Plugin-specific configuration
	pub fn with_plugin(
		mut self,
		name: String,
		plugin: Arc<dyn StatePlugin>,
		config: solver_types::plugins::PluginConfig,
	) -> Self {
		self.plugins.push((name, plugin, config));
		self
	}

	/// Set the complete service configuration.
	///
	/// Replaces the current configuration with the provided settings.
	/// This is useful when you have a pre-built configuration structure.
	///
	/// # Arguments
	/// * `config` - Complete state service configuration
	pub fn with_config(mut self, config: StateConfig) -> Self {
		self.config = config;
		self
	}

	/// Set the default backend plugin name.
	///
	/// Specifies which backend plugin should be activated by default
	/// when the service is initialized. The plugin must be registered.
	///
	/// # Arguments
	/// * `backend` - Name of the default backend plugin
	pub fn with_default_backend(mut self, backend: String) -> Self {
		self.config.default_backend = backend;
		self
	}

	/// Set the cleanup interval for expired entry removal.
	///
	/// Configures how frequently the background cleanup task runs to
	/// remove expired entries from the storage backend.
	///
	/// # Arguments
	/// * `interval_seconds` - Cleanup interval in seconds
	pub fn with_cleanup_interval(mut self, interval_seconds: u64) -> Self {
		self.config.cleanup_interval_seconds = interval_seconds;
		self
	}

	/// Build the configured state service instance.
	///
	/// Creates the state service with the configured settings and registers
	/// all added plugins. Plugins are registered but not automatically activated;
	/// use `initialize()` to activate the default backend or `activate_backend()`
	/// for specific backend activation.
	///
	/// # Returns
	/// Configured state service ready for use
	pub async fn build(self) -> StateService {
		let service = StateService::new(self.config);

		// Initialize and register all plugins
		for (name, plugin, _) in self.plugins {
			// Since StatePlugin uses Arc, we need to initialize before storing
			// This is a bit tricky because we can't get mutable access to Arc contents
			// For now, we'll register and let the service handle initialization
			// when activating the backend
			service.register_plugin(name, plugin).await;
		}

		service
	}
}

impl Default for StateServiceBuilder {
	fn default() -> Self {
		Self::new()
	}
}
