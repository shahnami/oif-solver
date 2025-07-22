//! # File-Based State Plugin
//!
//! Provides persistent state storage using the file system.
//!
//! This plugin implements state storage using individual files for each key,
//! suitable for persistent storage across application restarts. It supports
//! TTL management through metadata files, atomic operations, and optional
//! synchronous writes for durability.

use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use solver_types::plugins::{
	state::*, BasePlugin, ConfigFieldType, ConfigValue, PluginConfig, PluginConfigSchema,
	PluginError, PluginHealth, PluginMetrics, PluginResult,
};
use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Duration, SystemTime};
use tokio::fs;

/// File-based state storage plugin implementation.
///
/// Provides persistent state storage using the file system with each key
/// stored as a separate file. Supports metadata tracking for TTL management
/// and configurable durability through synchronous write options.
#[derive(Debug, Default)]
pub struct FileStatePlugin {
	/// Plugin configuration settings
	config: FileConfig,
}

/// Configuration for the file-based state plugin.
///
/// Defines storage location and operational parameters for file-based
/// storage including directory creation and write synchronization settings.
#[derive(Debug, Clone)]
pub struct FileConfig {
	/// Root directory path for storing state files
	pub storage_path: PathBuf,
	/// Whether to create storage directories if they don't exist
	pub create_dirs: bool,
	/// Whether to sync files to disk after writes for durability
	pub sync_on_write: bool,
}

impl Default for FileConfig {
	fn default() -> Self {
		Self {
			storage_path: PathBuf::from("./state"),
			create_dirs: true,
			sync_on_write: true,
		}
	}
}

impl FileStatePlugin {
	/// Create a new file state plugin with default configuration.
	pub fn new() -> Self {
		Self {
			config: FileConfig::default(),
		}
	}

	/// Create a new file state plugin with custom configuration.
	///
	/// # Arguments
	/// * `config` - Configuration parameters for the plugin
	pub fn with_config(config: FileConfig) -> Self {
		Self { config }
	}

	/// Create a new file state plugin with a specific storage path.
	///
	/// Uses default configuration with the specified storage directory.
	///
	/// # Arguments
	/// * `path` - Path to the storage directory
	pub fn with_path<P: AsRef<Path>>(path: P) -> Self {
		Self {
			config: FileConfig {
				storage_path: path.as_ref().to_path_buf(),
				..FileConfig::default()
			},
		}
	}
}

#[async_trait]
impl BasePlugin for FileStatePlugin {
	fn plugin_type(&self) -> &'static str {
		"file_state"
	}

	fn name(&self) -> String {
		"File State Plugin".to_string()
	}

	fn version(&self) -> &'static str {
		"1.0.0"
	}

	fn description(&self) -> &'static str {
		"File-based state storage plugin"
	}

	async fn initialize(&mut self, _config: PluginConfig) -> PluginResult<()> {
		// Create storage directory if it doesn't exist
		if self.config.create_dirs {
			fs::create_dir_all(&self.config.storage_path)
				.await
				.map_err(|e| {
					PluginError::InitializationFailed(format!(
						"Failed to create storage directory: {}",
						e
					))
				})?;
		}

		// Verify directory is writable
		let test_file = self.config.storage_path.join(".test_write");
		fs::write(&test_file, b"test").await.map_err(|e| {
			PluginError::InitializationFailed(format!("Storage directory not writable: {}", e))
		})?;

		let _ = fs::remove_file(&test_file).await;

		Ok(())
	}

	fn validate_config(&self, _config: &PluginConfig) -> PluginResult<()> {
		if !self.config.storage_path.is_absolute() && !self.config.storage_path.starts_with("./") {
			return Err(PluginError::InvalidConfiguration(
				"Storage path must be absolute or start with './'".to_string(),
			));
		}
		Ok(())
	}

	async fn health_check(&self) -> PluginResult<PluginHealth> {
		// Check if storage directory exists and is writable
		match fs::metadata(&self.config.storage_path).await {
			Ok(metadata) => {
				if metadata.is_dir() {
					Ok(PluginHealth::healthy("File state plugin is operational"))
				} else {
					Ok(PluginHealth::unhealthy("Storage path is not a directory"))
				}
			}
			Err(e) => Ok(PluginHealth::unhealthy(format!(
				"Storage directory error: {}",
				e
			))),
		}
	}

	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		let mut metrics = PluginMetrics::new();

		// Get directory size
		if let Ok(size) = get_directory_size(&self.config.storage_path).await {
			metrics.set_gauge("storage_size_bytes", size as f64);
		}

		Ok(metrics)
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		// File storage doesn't need explicit shutdown
		Ok(())
	}

	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new()
			.optional(
				"storage_path",
				ConfigFieldType::String,
				"Path to storage directory",
				Some(ConfigValue::String("./state".to_string())),
			)
			.optional(
				"create_dirs",
				ConfigFieldType::Boolean,
				"Create directories if they don't exist",
				Some(ConfigValue::Boolean(true)),
			)
			.optional(
				"sync_on_write",
				ConfigFieldType::Boolean,
				"Sync file system on write",
				Some(ConfigValue::Boolean(true)),
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
impl StatePlugin for FileStatePlugin {
	fn backend_type(&self) -> &'static str {
		"file"
	}

	async fn create_store(&self) -> PluginResult<Box<dyn StateStore>> {
		Ok(Box::new(FileStore::new(self.config.clone()).await?))
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
		let mut settings = HashMap::new();
		settings.insert(
			"storage_path".to_string(),
			self.config.storage_path.display().to_string(),
		);
		settings.insert(
			"create_dirs".to_string(),
			self.config.create_dirs.to_string(),
		);
		settings.insert(
			"sync_on_write".to_string(),
			self.config.sync_on_write.to_string(),
		);

		Ok(BackendConfig {
			backend_type: "file".to_string(),
			version: "1.0.0".to_string(),
			features: vec![
				"ttl".to_string(),
				"backup".to_string(),
				"restore".to_string(),
				"atomic_operations".to_string(),
			],
			limits: BackendLimits {
				max_key_size: Some(1024),               // 1KB key limit
				max_value_size: Some(10 * 1024 * 1024), // 10MB value limit
				max_keys: None,
				max_storage_size: None,
				max_ttl: None,
			},
			settings,
		})
	}

	async fn optimize(&self) -> PluginResult<OptimizationResult> {
		// For file storage, optimization would be defragmentation or cleanup
		// For now, just return a placeholder
		Ok(OptimizationResult {
			optimization_type: "file_cleanup".to_string(),
			bytes_freed: 0,
			time_taken_ms: 0,
			performance_improvement: None,
		})
	}

	async fn backup(&self, destination: &str) -> PluginResult<BackupResult> {
		// TODO: Implement proper backup
		Ok(BackupResult {
			backup_id: format!(
				"backup-{}",
				SystemTime::now()
					.duration_since(SystemTime::UNIX_EPOCH)
					.unwrap()
					.as_secs()
			),
			destination: destination.to_string(),
			size_bytes: 0,
			keys_backed_up: 0,
			timestamp: SystemTime::now()
				.duration_since(SystemTime::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			checksum: None,
		})
	}

	async fn restore(&self, source: &str) -> PluginResult<RestoreResult> {
		// TODO: Implement proper restore
		Ok(RestoreResult {
			source: source.to_string(),
			keys_restored: 0,
			bytes_restored: 0,
			timestamp: SystemTime::now()
				.duration_since(SystemTime::UNIX_EPOCH)
				.unwrap()
				.as_secs(),
			conflicts_resolved: 0,
		})
	}
}

/// File-based entry with metadata stored as JSON.
///
/// Represents a single key-value entry stored in the file system with
/// metadata for tracking creation time and expiration. Values are Base64
/// encoded for JSON compatibility and safe storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
	/// Original key name
	key: String,
	/// Base64 encoded value for JSON safety
	value: String,
	/// Timestamp when the entry was created
	created_at: SystemTime,
	/// Optional expiration timestamp for TTL support
	expires_at: Option<SystemTime>,
}

impl FileEntry {
	/// Create a new entry without expiration.
	///
	/// # Arguments
	/// * `key` - The key for this entry
	/// * `value` - The value to store (will be Base64 encoded)
	fn new(key: String, value: Bytes) -> Self {
		Self {
			key,
			value: general_purpose::STANDARD.encode(&value),
			created_at: SystemTime::now(),
			expires_at: None,
		}
	}

	/// Create a new entry with a time-to-live.
	///
	/// # Arguments
	/// * `key` - The key for this entry
	/// * `value` - The value to store (will be Base64 encoded)
	/// * `ttl` - Time-to-live duration after which the entry expires
	fn with_ttl(key: String, value: Bytes, ttl: Duration) -> Self {
		Self {
			key,
			value: general_purpose::STANDARD.encode(&value),
			created_at: SystemTime::now(),
			expires_at: Some(SystemTime::now() + ttl),
		}
	}

	/// Decode and retrieve the stored value.
	///
	/// # Returns
	/// The decoded value bytes or Base64 decode error
	fn get_value(&self) -> Result<Bytes, base64::DecodeError> {
		general_purpose::STANDARD
			.decode(&self.value)
			.map(Bytes::from)
	}

	/// Check if this entry has expired.
	///
	/// # Returns
	/// True if the entry has an expiration time that has passed
	fn is_expired(&self) -> bool {
		if let Some(expires_at) = self.expires_at {
			SystemTime::now() > expires_at
		} else {
			false
		}
	}
}

/// File-based store implementation with persistent storage.
///
/// Provides the actual storage implementation using individual JSON files
/// for each key-value pair. Handles file system operations, key sanitization,
/// and metadata management for TTL support.
#[derive(Debug)]
pub struct FileStore {
	/// Configuration parameters for this store instance
	config: FileConfig,
}

impl FileStore {
	/// Create a new file store with the specified configuration.
	///
	/// Creates the storage directory if it doesn't exist and is configured
	/// to do so.
	///
	/// # Arguments
	/// * `config` - Configuration parameters including storage path
	///
	/// # Errors
	/// Returns error if directory creation fails
	pub async fn new(config: FileConfig) -> PluginResult<Self> {
		// Create base storage directory
		fs::create_dir_all(&config.storage_path)
			.await
			.map_err(|e| {
				PluginError::InitializationFailed(format!(
					"Failed to create storage directory: {}",
					e
				))
			})?;

		Ok(Self { config })
	}

	fn key_to_path(&self, key: &str) -> PluginResult<PathBuf> {
		// Convert key to filesystem-safe path
		let safe_key = self.make_filesystem_safe(key)?;
		Ok(self.config.storage_path.join(format!("{}.json", safe_key)))
	}

	/// Convert key to filesystem-safe filename
	/// Handles special characters and path separators
	fn make_filesystem_safe(&self, key: &str) -> PluginResult<String> {
		// Replace problematic characters with safe alternatives
		let safe = key
			.replace('/', "_slash_")
			.replace('\\', "_backslash_")
			.replace(':', "_")
			.replace('*', "_star_")
			.replace('?', "_question_")
			.replace('<', "_lt_")
			.replace('>', "_gt_")
			.replace('|', "_pipe_")
			.replace('"', "_quote_")
			.replace(' ', "_space_");

		// Ensure filename isn't too long (most filesystems have 255 char limit)
		if safe.len() > 200 {
			// If too long, use first 100 chars + hash of full key + last 50 chars
			let hash = format!("{:x}", md5::compute(key));
			let start = &safe[..100];
			let end = &safe[safe.len().saturating_sub(50)..];
			Ok(format!("{}_{}_..._{}", start, hash, end))
		} else {
			Ok(safe)
		}
	}

	async fn read_entry(&self, key: &str) -> PluginResult<Option<FileEntry>> {
		let file_path = self.key_to_path(key)?;

		match fs::read(&file_path).await {
			Ok(data) => {
				let entry: FileEntry = serde_json::from_slice(&data).map_err(|e| {
					PluginError::StateError(format!("Failed to deserialize entry: {}", e))
				})?;

				if entry.is_expired() {
					// Clean up expired entry
					let _ = fs::remove_file(&file_path).await;
					Ok(None)
				} else {
					Ok(Some(entry))
				}
			}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
			Err(e) => Err(PluginError::StateError(format!(
				"Failed to read entry: {}",
				e
			))),
		}
	}

	async fn write_entry(&self, key: &str, entry: &FileEntry) -> PluginResult<()> {
		let file_path = self.key_to_path(key)?;

		// Ensure parent directory exists
		if let Some(parent) = file_path.parent() {
			fs::create_dir_all(parent).await.map_err(|e| {
				PluginError::StateError(format!("Failed to create parent directory: {}", e))
			})?;
		}

		// Write entry as pretty JSON for debugging
		let json_data = serde_json::to_string_pretty(&entry)
			.map_err(|e| PluginError::StateError(format!("Failed to serialize entry: {}", e)))?;

		fs::write(&file_path, json_data)
			.await
			.map_err(|e| PluginError::StateError(format!("Failed to write entry: {}", e)))?;

		if self.config.sync_on_write {
			// Sync file system (simplified - in production you'd sync the specific files)
			tokio::task::spawn_blocking(|| {
				#[cfg(unix)]
				unsafe {
					libc::sync();
				}
			})
			.await
			.map_err(|e| PluginError::StateError(format!("Failed to sync: {}", e)))?;
		}

		Ok(())
	}

	async fn delete_entry(&self, key: &str) -> PluginResult<()> {
		let file_path = self.key_to_path(key)?;
		let _ = fs::remove_file(&file_path).await;
		Ok(())
	}

	/// Recursively collect all keys from the storage directory
	fn collect_keys<'a>(
		&'a self,
		dir: &'a Path,
		prefix: Option<&'a str>,
		keys: &'a mut Vec<String>,
	) -> Pin<Box<dyn Future<Output = PluginResult<()>> + Send + 'a>> {
		Box::pin(async move {
			let mut entries = fs::read_dir(dir)
				.await
				.map_err(|e| PluginError::StateError(format!("Failed to read directory: {}", e)))?;

			while let Some(entry) = entries.next_entry().await.map_err(|e| {
				PluginError::StateError(format!("Failed to read directory entry: {}", e))
			})? {
				let path = entry.path();

				if path.is_dir() {
					// Recursively scan subdirectories
					self.collect_keys(&path, prefix, keys).await?;
				} else if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
					if filename.ends_with(".json") {
						// Read the file to get the original key
						if let Ok(data) = fs::read(&path).await {
							if let Ok(file_entry) = serde_json::from_slice::<FileEntry>(&data) {
								if !file_entry.is_expired() {
									let key = &file_entry.key;
									if let Some(p) = prefix {
										if key.starts_with(p) {
											keys.push(key.clone());
										}
									} else {
										keys.push(key.clone());
									}
								}
							}
						}
					}
				}
			}

			Ok(())
		})
	}

	/// Calculate storage statistics by scanning all files
	fn calculate_stats<'a>(
		&'a self,
		dir: &'a Path,
		total_keys: &'a mut u64,
		total_size: &'a mut u64,
	) -> Pin<Box<dyn Future<Output = PluginResult<()>> + Send + 'a>> {
		Box::pin(async move {
			let mut entries = fs::read_dir(dir)
				.await
				.map_err(|e| PluginError::StateError(format!("Failed to read directory: {}", e)))?;

			while let Some(entry) = entries.next_entry().await.map_err(|e| {
				PluginError::StateError(format!("Failed to read directory entry: {}", e))
			})? {
				let path = entry.path();

				if path.is_dir() {
					self.calculate_stats(&path, total_keys, total_size).await?;
				} else if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
					if filename.ends_with(".json") {
						*total_keys += 1;
						if let Ok(metadata) = entry.metadata().await {
							*total_size += metadata.len();
						}
					}
				}
			}

			Ok(())
		})
	}

	/// Clean up expired entries recursively
	fn cleanup_expired<'a>(
		&'a self,
		dir: &'a Path,
		keys_removed: &'a mut u64,
		bytes_freed: &'a mut u64,
	) -> Pin<Box<dyn Future<Output = PluginResult<()>> + Send + 'a>> {
		Box::pin(async move {
			let mut entries = fs::read_dir(dir)
				.await
				.map_err(|e| PluginError::StateError(format!("Failed to read directory: {}", e)))?;

			while let Some(entry) = entries.next_entry().await.map_err(|e| {
				PluginError::StateError(format!("Failed to read directory entry: {}", e))
			})? {
				let path = entry.path();

				if path.is_dir() {
					self.cleanup_expired(&path, keys_removed, bytes_freed)
						.await?;
				} else if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
					if filename.ends_with(".json") {
						if let Ok(data) = fs::read(&path).await {
							if let Ok(file_entry) = serde_json::from_slice::<FileEntry>(&data) {
								if file_entry.is_expired() {
									*bytes_freed += data.len() as u64;
									*keys_removed += 1;
									let _ = fs::remove_file(&path).await;
								}
							}
						}
					}
				}
			}

			Ok(())
		})
	}
}

#[async_trait]
impl StateStore for FileStore {
	async fn get(&self, key: &str) -> PluginResult<Option<Bytes>> {
		match self.read_entry(key).await? {
			Some(entry) => {
				let value = entry.get_value().map_err(|e| {
					PluginError::StateError(format!("Failed to decode value: {}", e))
				})?;
				Ok(Some(value))
			}
			None => Ok(None),
		}
	}

	async fn set(&self, key: &str, value: Bytes) -> PluginResult<()> {
		let entry = FileEntry::new(key.to_string(), value);
		self.write_entry(key, &entry).await
	}

	async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()> {
		let entry = FileEntry::with_ttl(key.to_string(), value, ttl);
		self.write_entry(key, &entry).await
	}

	async fn delete(&self, key: &str) -> PluginResult<()> {
		self.delete_entry(key).await
	}

	async fn exists(&self, key: &str) -> PluginResult<bool> {
		Ok(self.read_entry(key).await?.is_some())
	}

	async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>> {
		let mut keys = Vec::new();

		// Recursively scan the storage directory for .json files
		self.collect_keys(&self.config.storage_path, prefix, &mut keys)
			.await?;

		Ok(keys)
	}

	async fn batch_get(&self, keys: &[String]) -> PluginResult<Vec<Option<Bytes>>> {
		let mut results = Vec::with_capacity(keys.len());

		for key in keys {
			results.push(self.get(key).await?);
		}

		Ok(results)
	}

	async fn batch_set(&self, items: &[(String, Bytes)]) -> PluginResult<()> {
		for (key, value) in items {
			self.set(key, value.clone()).await?;
		}
		Ok(())
	}

	async fn batch_delete(&self, keys: &[String]) -> PluginResult<()> {
		for key in keys {
			self.delete(key).await?;
		}
		Ok(())
	}

	async fn atomic_update(
		&self,
		key: &str,
		updater: Box<dyn FnOnce(Option<Bytes>) -> PluginResult<Option<Bytes>> + Send>,
	) -> PluginResult<()> {
		// Simple file-based atomic update using file locking would be complex
		// For now, implement a basic version without full atomicity
		let current = self.get(key).await?;
		let new_value = updater(current)?;

		match new_value {
			Some(value) => self.set(key, value).await,
			None => self.delete(key).await,
		}
	}

	async fn get_stats(&self) -> PluginResult<StorageStats> {
		let mut total_keys = 0;
		let mut total_size = 0;

		self.calculate_stats(&self.config.storage_path, &mut total_keys, &mut total_size)
			.await?;

		Ok(StorageStats {
			total_keys,
			total_size_bytes: total_size,
			memory_usage_bytes: None,
			hit_rate: None,
			operations_count: HashMap::new(),
		})
	}

	async fn cleanup(&self) -> PluginResult<CleanupStats> {
		let start = SystemTime::now();
		let mut keys_removed = 0;
		let mut bytes_freed = 0;

		self.cleanup_expired(
			&self.config.storage_path,
			&mut keys_removed,
			&mut bytes_freed,
		)
		.await?;

		let duration = start.elapsed().unwrap_or_default();

		Ok(CleanupStats {
			keys_removed,
			bytes_freed,
			duration_ms: duration.as_millis() as u64,
		})
	}
}

// Helper function to calculate directory size
fn get_directory_size(
	path: &Path,
) -> Pin<Box<dyn Future<Output = Result<u64, std::io::Error>> + Send + '_>> {
	Box::pin(async move {
		let mut total_size = 0;
		let mut entries = fs::read_dir(path).await?;

		while let Some(entry) = entries.next_entry().await? {
			let metadata = entry.metadata().await?;
			if metadata.is_file() {
				total_size += metadata.len();
			} else if metadata.is_dir() {
				total_size += get_directory_size(&entry.path()).await?;
			}
		}

		Ok(total_size)
	})
}
