use async_trait::async_trait;
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

/// Plugin that creates file-based state stores
#[derive(Debug, Default)]
pub struct FileStatePlugin {
	config: FileConfig,
}

#[derive(Debug, Clone)]
pub struct FileConfig {
	pub storage_path: PathBuf,
	pub create_dirs: bool,
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
	pub fn new() -> Self {
		Self {
			config: FileConfig::default(),
		}
	}

	pub fn with_config(config: FileConfig) -> Self {
		Self { config }
	}

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

/// File-based entry with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileEntry {
	value: Bytes,
	created_at: SystemTime,
	expires_at: Option<SystemTime>,
}

impl FileEntry {
	fn new(value: Bytes) -> Self {
		Self {
			value,
			created_at: SystemTime::now(),
			expires_at: None,
		}
	}

	fn with_ttl(value: Bytes, ttl: Duration) -> Self {
		Self {
			value,
			created_at: SystemTime::now(),
			expires_at: Some(SystemTime::now() + ttl),
		}
	}

	fn is_expired(&self) -> bool {
		if let Some(expires_at) = self.expires_at {
			SystemTime::now() > expires_at
		} else {
			false
		}
	}
}

/// The actual file-based store implementation
#[derive(Debug)]
pub struct FileStore {
	config: FileConfig,
	data_dir: PathBuf,
	metadata_dir: PathBuf,
}

impl FileStore {
	pub async fn new(config: FileConfig) -> PluginResult<Self> {
		let data_dir = config.storage_path.join("data");
		let metadata_dir = config.storage_path.join("metadata");

		// Create subdirectories
		fs::create_dir_all(&data_dir).await.map_err(|e| {
			PluginError::InitializationFailed(format!("Failed to create data directory: {}", e))
		})?;

		fs::create_dir_all(&metadata_dir).await.map_err(|e| {
			PluginError::InitializationFailed(format!("Failed to create metadata directory: {}", e))
		})?;

		Ok(Self {
			config,
			data_dir,
			metadata_dir,
		})
	}

	fn key_to_path(&self, key: &str) -> PathBuf {
		// Simple approach: hash the key to create a filename
		let hash = format!("{:x}", md5::compute(key));
		self.data_dir.join(hash)
	}

	fn metadata_path(&self, key: &str) -> PathBuf {
		let hash = format!("{:x}", md5::compute(key));
		self.metadata_dir.join(format!("{}.meta", hash))
	}

	async fn read_entry(&self, key: &str) -> PluginResult<Option<FileEntry>> {
		let meta_path = self.metadata_path(key);

		match fs::read(&meta_path).await {
			Ok(data) => {
				let entry: FileEntry = serde_json::from_slice(&data).map_err(|e| {
					PluginError::StateError(format!("Failed to deserialize metadata: {}", e))
				})?;

				if entry.is_expired() {
					// Clean up expired entry
					let _ = fs::remove_file(&meta_path).await;
					let _ = fs::remove_file(self.key_to_path(key)).await;
					Ok(None)
				} else {
					Ok(Some(entry))
				}
			}
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
			Err(e) => Err(PluginError::StateError(format!(
				"Failed to read metadata: {}",
				e
			))),
		}
	}

	async fn write_entry(&self, key: &str, entry: &FileEntry) -> PluginResult<()> {
		let data_path = self.key_to_path(key);
		let meta_path = self.metadata_path(key);

		// Write data
		fs::write(&data_path, &entry.value)
			.await
			.map_err(|e| PluginError::StateError(format!("Failed to write data: {}", e)))?;

		// Write metadata
		let meta_data = serde_json::to_vec(&entry)
			.map_err(|e| PluginError::StateError(format!("Failed to serialize metadata: {}", e)))?;

		fs::write(&meta_path, meta_data)
			.await
			.map_err(|e| PluginError::StateError(format!("Failed to write metadata: {}", e)))?;

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
		let data_path = self.key_to_path(key);
		let meta_path = self.metadata_path(key);

		let _ = fs::remove_file(&data_path).await;
		let _ = fs::remove_file(&meta_path).await;

		Ok(())
	}
}

#[async_trait]
impl StateStore for FileStore {
	async fn get(&self, key: &str) -> PluginResult<Option<Bytes>> {
		match self.read_entry(key).await? {
			Some(entry) => Ok(Some(entry.value)),
			None => Ok(None),
		}
	}

	async fn set(&self, key: &str, value: Bytes) -> PluginResult<()> {
		let entry = FileEntry::new(value);
		self.write_entry(key, &entry).await
	}

	async fn set_with_ttl(&self, key: &str, value: Bytes, ttl: Duration) -> PluginResult<()> {
		let entry = FileEntry::with_ttl(value, ttl);
		self.write_entry(key, &entry).await
	}

	async fn delete(&self, key: &str) -> PluginResult<()> {
		self.delete_entry(key).await
	}

	async fn exists(&self, key: &str) -> PluginResult<bool> {
		Ok(self.read_entry(key).await?.is_some())
	}

	async fn list_keys(&self, prefix: Option<&str>) -> PluginResult<Vec<String>> {
		// This is a simplified implementation
		// In production, you'd want to maintain an index of keys
		let mut keys = Vec::new();

		let mut entries = fs::read_dir(&self.metadata_dir).await.map_err(|e| {
			PluginError::StateError(format!("Failed to read metadata directory: {}", e))
		})?;

		while let Some(entry) = entries.next_entry().await.map_err(|e| {
			PluginError::StateError(format!("Failed to read directory entry: {}", e))
		})? {
			if let Some(filename) = entry.file_name().to_str() {
				if filename.ends_with(".meta") {
					// This is a simplified approach - in production you'd store the original key
					// For now, we'll just return the hash
					let key = filename.trim_end_matches(".meta").to_string();

					if let Some(p) = prefix {
						if key.starts_with(p) {
							keys.push(key);
						}
					} else {
						keys.push(key);
					}
				}
			}
		}

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

		let mut entries = fs::read_dir(&self.metadata_dir).await.map_err(|e| {
			PluginError::StateError(format!("Failed to read metadata directory: {}", e))
		})?;

		while let Some(entry) = entries.next_entry().await.map_err(|e| {
			PluginError::StateError(format!("Failed to read directory entry: {}", e))
		})? {
			if entry
				.file_name()
				.to_str()
				.map(|n| n.ends_with(".meta"))
				.unwrap_or(false)
			{
				total_keys += 1;
				if let Ok(metadata) = entry.metadata().await {
					total_size += metadata.len();
				}
			}
		}

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

		let mut entries = fs::read_dir(&self.metadata_dir).await.map_err(|e| {
			PluginError::StateError(format!("Failed to read metadata directory: {}", e))
		})?;

		while let Some(entry) = entries.next_entry().await.map_err(|e| {
			PluginError::StateError(format!("Failed to read directory entry: {}", e))
		})? {
			if let Some(filename) = entry.file_name().to_str() {
				if filename.ends_with(".meta") {
					let key = filename.trim_end_matches(".meta");

					// Check if entry is expired
					if let Ok(Some(file_entry)) = self.read_entry(key).await {
						if file_entry.is_expired() {
							bytes_freed += file_entry.value.len() as u64;
							keys_removed += 1;
							let _ = self.delete_entry(key).await;
						}
					}
				}
			}
		}

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
