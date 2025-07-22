// solver-state/src/lib.rs

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

/// State service that orchestrates multiple state plugins
#[derive(Debug)]
pub struct StateService {
	plugins: Arc<RwLock<HashMap<String, Arc<dyn StatePlugin>>>>,
	active_backend: Arc<RwLock<Option<String>>>,
	active_store: Arc<RwLock<Option<Arc<dyn StateStore>>>>,
	config: StateConfig,
}

impl StateService {
	pub fn new(config: StateConfig) -> Self {
		Self {
			plugins: Arc::new(RwLock::new(HashMap::new())),
			active_backend: Arc::new(RwLock::new(None)),
			active_store: Arc::new(RwLock::new(None)),
			config,
		}
	}

	pub fn with_default_backend(mut self, backend: String) -> Self {
		self.config.default_backend = backend;
		self
	}

	pub fn with_cleanup_interval(mut self, interval_seconds: u64) -> Self {
		self.config.cleanup_interval_seconds = interval_seconds;
		self
	}

	/// Register a new state plugin
	pub async fn register_plugin(&self, name: String, plugin: Arc<dyn StatePlugin>) {
		info!("Registering state plugin: {}", name);
		self.plugins.write().await.insert(name, plugin);
	}

	/// Activate a specific backend
	pub async fn activate_backend(&self, backend_name: &str) -> PluginResult<()> {
		let plugins = self.plugins.read().await;
		let plugin = plugins.get(backend_name).ok_or_else(|| {
			PluginError::NotFound(format!("State plugin not found: {}", backend_name))
		})?;

		info!("Activating state backend: {}", backend_name);

		// Create store from plugin
		let store = plugin.create_store().await?;

		// Update active backend and store
		*self.active_backend.write().await = Some(backend_name.to_string());
		*self.active_store.write().await = Some(Arc::from(store));

		info!("State backend activated successfully: {}", backend_name);
		Ok(())
	}

	/// Get the currently active backend name
	pub async fn get_active_backend(&self) -> Option<String> {
		self.active_backend.read().await.clone()
	}

	/// Switch to a different backend
	pub async fn switch_backend(&self, backend_name: &str) -> PluginResult<()> {
		info!("Switching state backend to: {}", backend_name);

		// Cleanup current backend if any
		if let Some(current_backend) = self.get_active_backend().await {
			info!("Cleaning up current backend: {}", current_backend);
			// TODO: Add proper cleanup logic
		}

		self.activate_backend(backend_name).await
	}

	/// Get active store or return error
	async fn get_active_store(&self) -> PluginResult<Arc<dyn StateStore>> {
		self.active_store
			.read()
			.await
			.clone()
			.ok_or_else(|| PluginError::ExecutionFailed("No active state backend".to_string()))
	}

	/// Get value by key
	pub async fn get(&self, key: &str) -> PluginResult<Option<Bytes>> {
		let store = self.get_active_store().await?;
		debug!("Getting key: {}", key);
		store.get(key).await
	}

	/// Set value for key
	pub async fn set(&self, key: &str, value: Bytes) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Setting key: {}", key);
		store.set(key, value).await
	}

	/// Set value with TTL
	pub async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Setting key with TTL: {} ({}s)", key, ttl.as_secs());
		store.set_with_ttl(key, value, ttl).await
	}

	/// Delete key
	pub async fn delete(&self, key: &str) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Deleting key: {}", key);
		store.delete(key).await
	}

	/// Check if key exists
	pub async fn exists(&self, key: &str) -> PluginResult<bool> {
		let store = self.get_active_store().await?;
		debug!("Checking existence of key: {}", key);
		store.exists(key).await
	}

	/// List keys with optional prefix
	pub async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>> {
		let store = self.get_active_store().await?;
		debug!("Listing keys with prefix: {:?}", prefix);
		store.list_keys(prefix).await
	}

	/// Batch get multiple keys
	pub async fn batch_get(&self, keys: &[String]) -> PluginResult<Vec<Option<Bytes>>> {
		let store = self.get_active_store().await?;
		debug!("Batch getting {} keys", keys.len());
		store.batch_get(keys).await
	}

	/// Batch set multiple key-value pairs
	pub async fn batch_set(&self, items: &[(String, Bytes)]) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Batch setting {} items", items.len());
		store.batch_set(items).await
	}

	/// Batch delete multiple keys
	pub async fn batch_delete(&self, keys: &[String]) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Batch deleting {} keys", keys.len());
		store.batch_delete(keys).await
	}

	/// Atomic update operation
	pub async fn atomic_update(
		&self,
		key: &str,
		updater: Box<dyn FnOnce(Option<Bytes>) -> PluginResult<Option<Bytes>> + Send>,
	) -> PluginResult<()> {
		let store = self.get_active_store().await?;
		debug!("Atomic update for key: {}", key);
		store.atomic_update(key, updater).await
	}

	/// Get storage statistics
	pub async fn get_stats(&self) -> PluginResult<StorageStats> {
		let store = self.get_active_store().await?;
		debug!("Getting storage statistics");
		store.get_stats().await
	}

	/// Cleanup expired entries
	pub async fn cleanup(&self) -> PluginResult<CleanupStats> {
		let store = self.get_active_store().await?;
		info!("Running cleanup operation");
		let stats = store.cleanup().await?;
		info!(
			"Cleanup completed: {} keys removed, {} bytes freed",
			stats.keys_removed, stats.bytes_freed
		);
		Ok(stats)
	}

	/// Get backend configuration for active backend
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

	/// List all registered plugins
	pub async fn list_plugins(&self) -> Vec<String> {
		self.plugins.read().await.keys().cloned().collect()
	}

	/// Check if a plugin is registered
	pub async fn has_plugin(&self, name: &str) -> bool {
		self.plugins.read().await.contains_key(name)
	}

	/// Health check all state plugins
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

	/// Get metrics from all plugins
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

	/// Initialize with default backend
	pub async fn initialize(&self) -> PluginResult<()> {
		let default_backend = self.config.default_backend.clone();

		if self.has_plugin(&default_backend).await {
			info!("Initializing with default backend: {}", default_backend);
			self.activate_backend(&default_backend).await?;
		} else {
			warn!(
				"Default backend '{}' not found, no backend activated",
				default_backend
			);
		}

		Ok(())
	}

	/// Start background cleanup task
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

/// Builder for StateService
pub struct StateServiceBuilder {
	plugins: Vec<(
		String,
		Arc<dyn StatePlugin>,
		solver_types::plugins::PluginConfig,
	)>,
	config: StateConfig,
}

impl StateServiceBuilder {
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

	pub fn with_plugin(
		mut self,
		name: String,
		plugin: Arc<dyn StatePlugin>,
		config: solver_types::plugins::PluginConfig,
	) -> Self {
		self.plugins.push((name, plugin, config));
		self
	}

	pub fn with_config(mut self, config: StateConfig) -> Self {
		self.config = config;
		self
	}

	pub fn with_default_backend(mut self, backend: String) -> Self {
		self.config.default_backend = backend;
		self
	}

	pub fn with_cleanup_interval(mut self, interval_seconds: u64) -> Self {
		self.config.cleanup_interval_seconds = interval_seconds;
		self
	}

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
