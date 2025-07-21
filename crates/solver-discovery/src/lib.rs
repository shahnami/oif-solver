// solver-discovery/src/lib.rs

//! # Solver Discovery Library
//!
//! This library provides order discovery capabilities for the OIF Solver.
//! It orchestrates multiple discovery plugins to monitor various sources
//! for order events across different chains and protocols.
//!
//! ## Key Components
//!
//! - [`DiscoveryService`] - Low-level service that manages discovery plugins
//! - [`DiscoveryManager`] - High-level manager with simplified interface
//! - Event deduplication and filtering
//! - Historical discovery support
//! - Multi-chain monitoring

use serde::Serialize;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use solver_types::configs::DiscoveryConfig;
use solver_types::plugins::{
	ChainId, DiscoveryEvent, DiscoveryPlugin, EventSink, EventType, PluginError, PluginResult,
	Timestamp,
};
use solver_types::{Event, PluginConfig};
use std::collections::HashMap;
use std::sync::Arc;

type PluginMap = HashMap<String, Arc<Mutex<Box<dyn DiscoveryPlugin>>>>;
type PluginsType = Arc<RwLock<PluginMap>>;

/// Discovery service that orchestrates multiple discovery plugins
#[derive(Debug)]
pub struct DiscoveryService {
	plugins: PluginsType,
	active_sources: Arc<RwLock<HashMap<String, DiscoverySource>>>,
	event_sink: EventSink<Event>,
	discovery_stats: Arc<RwLock<DiscoveryStats>>,
	config: DiscoveryConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverySource {
	pub plugin_name: String,
	pub chain_id: ChainId,
	pub source_type: String,
	pub status: SourceStatus,
	pub stats: SourceStats,
}

#[derive(Debug, Clone, Serialize)]
pub enum SourceStatus {
	Stopped,
	Starting,
	Running,
	Error(String),
	Stopping,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SourceStats {
	pub events_discovered: u64,
	pub last_event_timestamp: Option<Timestamp>,
	pub current_block: Option<u64>,
	pub target_block: Option<u64>,
	pub errors_count: u64,
	pub started_at: Option<Timestamp>,
	pub average_processing_time_ms: f64,
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveryStats {
	pub total_events_discovered: u64,
	pub total_sources_active: u64,
	pub total_errors: u64,
	pub events_per_minute: f64,
	pub duplicate_events_filtered: u64,
	pub last_activity_timestamp: Option<Timestamp>,
}

/// Event deduplication cache
#[derive(Debug)]
struct EventDeduplicator {
	seen_events: Arc<RwLock<HashMap<String, Timestamp>>>,
	window_seconds: u64,
}

impl EventDeduplicator {
	fn new(window_seconds: u64) -> Self {
		Self {
			seen_events: Arc::new(RwLock::new(HashMap::new())),
			window_seconds,
		}
	}

	async fn is_duplicate(&self, event: &DiscoveryEvent) -> bool {
		let event_key = self.create_event_key(event);
		let current_time = std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.unwrap()
			.as_secs();

		let mut seen = self.seen_events.write().await;

		// Clean up old entries
		seen.retain(|_, timestamp| current_time - *timestamp < self.window_seconds);

		// Check if we've seen this event recently
		match seen.entry(event_key) {
			std::collections::hash_map::Entry::Occupied(_) => true,
			std::collections::hash_map::Entry::Vacant(e) => {
				e.insert(current_time);
				false
			}
		}
	}

	fn create_event_key(&self, event: &DiscoveryEvent) -> String {
		// Use discriminant for EventType to get a unique integer value
		use std::mem::discriminant;
		let event_type_id = format!("{:?}", discriminant(&event.event_type));
		format!(
			"{}:{}:{}:{}",
			event.chain_id,
			event.id,
			event
				.transaction_hash
				.as_ref()
				.unwrap_or(&"none".to_string()),
			event_type_id
		)
	}
}

impl DiscoveryService {
	pub fn new(event_sink: EventSink<Event>) -> Self {
		let default_config = DiscoveryConfig {
			historical_sync: false,
			realtime_monitoring: true,
			dedupe_events: true,
			max_event_age_seconds: 300,
			max_events_per_second: 1000,
			event_buffer_size: 10000,
			deduplication_window_seconds: 300,
			max_concurrent_sources: 10,
		};
		Self::with_config(event_sink, default_config)
	}

	pub fn with_config(event_sink: EventSink<Event>, config: DiscoveryConfig) -> Self {
		Self {
			plugins: Arc::new(RwLock::new(HashMap::new())),
			active_sources: Arc::new(RwLock::new(HashMap::new())),
			event_sink,
			discovery_stats: Arc::new(RwLock::new(DiscoveryStats::default())),
			config,
		}
	}

	/// Register a discovery plugin
	pub async fn register_plugin(&self, name: String, plugin: Box<dyn DiscoveryPlugin>) {
		info!("Registering discovery plugin: {}", name);
		self.plugins
			.write()
			.await
			.insert(name, Arc::new(Mutex::new(plugin)));
	}

	/// Start monitoring with a specific plugin
	pub async fn start_source(&self, plugin_name: &str) -> PluginResult<()> {
		let plugin = {
			let plugins = self.plugins.read().await;
			plugins.get(plugin_name).cloned().ok_or_else(|| {
				PluginError::NotFound(format!("Plugin not found: {}", plugin_name))
			})?
		};

		info!("Starting discovery source: {}", plugin_name);

		// Check if we're at max concurrent sources
		let active_count = self.active_sources.read().await.len();
		if active_count >= self.config.max_concurrent_sources {
			return Err(PluginError::ExecutionFailed(
				"Maximum concurrent sources reached".to_string(),
			));
		}

		let (chain_id, source_type) = {
			let locked_plugin = plugin.lock().await;
			let chain_id = locked_plugin.chain_id();
			let plugin_type = locked_plugin.plugin_type().to_string();
			info!(
				"🔍 DEBUG: Plugin metadata - chain_id: {}, type: {}",
				chain_id, plugin_type
			);
			(chain_id, plugin_type)
		};

		// Create source entry
		let source = DiscoverySource {
			plugin_name: plugin_name.to_string(),
			chain_id,
			source_type,
			status: SourceStatus::Starting,
			stats: SourceStats::default(),
		};

		self.active_sources
			.write()
			.await
			.insert(plugin_name.to_string(), source.clone());

		// Create filtered event sink for this plugin
		let filtered_sink = self.create_filtered_sink(plugin_name.to_string()).await;

		// Lock the plugin for mutable access
		let mut plugin = plugin.lock().await;
		plugin.start_monitoring(filtered_sink).await?;

		let mut sources = self.active_sources.write().await;
		if let Some(source) = sources.get_mut(plugin_name) {
			source.status = SourceStatus::Running;
			source.stats.started_at = Some(
				std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap()
					.as_secs(),
			);
		}
		info!("Discovery source started successfully: {}", plugin_name);
		Ok(())
	}

	/// Stop monitoring for a specific plugin
	pub async fn stop_source(&self, plugin_name: &str) -> PluginResult<()> {
		let plugin = {
			let plugins = self.plugins.read().await;
			plugins.get(plugin_name).cloned().ok_or_else(|| {
				PluginError::NotFound(format!("Plugin not found: {}", plugin_name))
			})?
		};

		info!("Stopping discovery source: {}", plugin_name);

		{
			let mut sources = self.active_sources.write().await;
			if let Some(source) = sources.get_mut(plugin_name) {
				source.status = SourceStatus::Stopping;
			}
		}

		// Lock the plugin for mutable access
		let mut plugin = plugin.lock().await;
		plugin.stop_monitoring().await?;

		self.active_sources.write().await.remove(plugin_name);
		info!("Discovery source stopped successfully: {}", plugin_name);
		Ok(())
	}

	/// Start all registered plugins
	pub async fn start_all(&self) -> PluginResult<()> {
		info!("Starting all discovery sources");

		let plugin_names: Vec<String> = { self.plugins.read().await.keys().cloned().collect() };

		let mut errors = Vec::new();
		for plugin_name in plugin_names {
			if let Err(e) = self.start_source(&plugin_name).await {
				errors.push(format!("{}: {}", plugin_name, e));
			}
		}

		if errors.is_empty() {
			info!("All discovery sources started successfully");
			Ok(())
		} else {
			let error_msg = format!("Some sources failed to start: {}", errors.join(", "));
			warn!("{}", error_msg);
			Err(PluginError::ExecutionFailed(error_msg))
		}
	}

	/// Stop all active sources
	pub async fn stop_all(&self) -> PluginResult<()> {
		info!("Stopping all discovery sources");

		let source_names: Vec<String> =
			{ self.active_sources.read().await.keys().cloned().collect() };

		let mut errors = Vec::new();
		for source_name in source_names {
			if let Err(e) = self.stop_source(&source_name).await {
				errors.push(format!("{}: {}", source_name, e));
			}
		}

		if errors.is_empty() {
			info!("All discovery sources stopped successfully");
			Ok(())
		} else {
			let error_msg = format!("Some sources failed to stop: {}", errors.join(", "));
			warn!("{}", error_msg);
			Err(PluginError::ExecutionFailed(error_msg))
		}
	}

	/// Get status of all active sources
	pub async fn get_status(&self) -> HashMap<String, DiscoverySource> {
		self.active_sources.read().await.clone()
	}

	/// Get status of a specific source
	pub async fn get_source_status(&self, plugin_name: &str) -> Option<DiscoverySource> {
		self.active_sources.read().await.get(plugin_name).cloned()
	}

	/// Get overall discovery statistics
	pub async fn get_stats(&self) -> DiscoveryStats {
		let mut stats = self.discovery_stats.read().await.clone();

		// Update active sources count
		stats.total_sources_active = self.active_sources.read().await.len() as u64;

		// Calculate events per minute from active sources
		let sources = self.active_sources.read().await;
		let total_events: u64 = sources.values().map(|s| s.stats.events_discovered).sum();
		let total_errors: u64 = sources.values().map(|s| s.stats.errors_count).sum();

		stats.total_events_discovered = total_events;
		stats.total_errors = total_errors;

		// Calculate events per minute (simplified calculation)
		if let Some(oldest_start) = sources.values().filter_map(|s| s.stats.started_at).min() {
			let current_time = std::time::SystemTime::now()
				.duration_since(std::time::UNIX_EPOCH)
				.unwrap()
				.as_secs();
			let minutes_running = (current_time - oldest_start) as f64 / 60.0;
			if minutes_running > 0.0 {
				stats.events_per_minute = total_events as f64 / minutes_running;
			}
		}

		stats
	}

	/// Health check all discovery plugins
	pub async fn health_check(
		&self,
	) -> PluginResult<HashMap<String, solver_types::plugins::PluginHealth>> {
		let all_plugins = self.plugins.read().await;
		let mut health_status = HashMap::new();

		for (plugin_name, plugin) in all_plugins.iter() {
			match plugin.lock().await.health_check().await {
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

	/// Create a filtered event sink that handles deduplication and statistics
	async fn create_filtered_sink(&self, plugin_name: String) -> EventSink<Event> {
		let (tx, mut rx) = mpsc::unbounded_channel();
		let main_sink = self.event_sink.clone();
		let active_sources = self.active_sources.clone();
		let config = self.config.clone();

		// Set up deduplicator if enabled
		let deduplicator = if config.dedupe_events {
			Some(EventDeduplicator::new(config.deduplication_window_seconds))
		} else {
			None
		};

		// Spawn task to process events from this plugin
		tokio::spawn(async move {
			while let Some(event) = rx.recv().await {
				// Extract the discovery event
				let discovery_event = match &event {
					Event::Discovery(de) => de,
					_ => continue, // Skip non-discovery events
				};

				// Check for duplicates if enabled
				if let Some(ref dedup) = deduplicator {
					if dedup.is_duplicate(discovery_event).await {
						debug!("Filtered duplicate event: {}", discovery_event.id);
						continue;
					}
				}

				// Update source statistics
				{
					let mut sources = active_sources.write().await;
					if let Some(source) = sources.get_mut(&plugin_name) {
						source.stats.events_discovered += 1;
						source.stats.last_event_timestamp = Some(discovery_event.timestamp);
						if let Some(block_number) = discovery_event.block_number {
							source.stats.current_block = Some(block_number);
						}
					}
				}

				// Forward to main sink
				if let Err(e) = main_sink.send(event) {
					error!("Failed to forward event to main sink: {}", e);
					// Update error count
					let mut sources = active_sources.write().await;
					if let Some(source) = sources.get_mut(&plugin_name) {
						source.stats.errors_count += 1;
					}
				}
			}
		});

		EventSink::new(tx)
	}

	/// Get plugins that support a specific chain
	pub async fn get_plugins_for_chain(&self, chain_id: ChainId) -> Vec<String> {
		let plugins = self.plugins.read().await;
		let mut result = Vec::new();
		for (name, plugin) in plugins.iter() {
			if plugin.lock().await.chain_id() == chain_id {
				result.push(name.clone());
			}
		}
		result
	}

	/// Get supported chains across all plugins
	pub async fn get_supported_chains(&self) -> Vec<ChainId> {
		let plugins = self.plugins.read().await;
		let mut chains = Vec::new();
		for plugin in plugins.values() {
			chains.push(plugin.lock().await.chain_id());
		}
		chains.sort_unstable();
		chains.dedup();
		chains
	}

	/// Get supported event types for a chain
	pub async fn get_supported_event_types(&self, chain_id: ChainId) -> Vec<EventType> {
		let plugins = self.plugins.read().await;
		let mut event_types = Vec::new();
		for (_, plugin) in plugins.iter() {
			let plugin = plugin.lock().await;
			if plugin.chain_id() == chain_id {
				event_types.extend(plugin.supported_event_types());
			}
		}
		use std::mem::discriminant;
		event_types.sort_by_key(|et| format!("{:?}", discriminant(et)));
		event_types.dedup_by_key(|et| format!("{:?}", discriminant(et)));
		event_types
	}
}

/// Builder for DiscoveryService
pub struct DiscoveryServiceBuilder {
	plugins: Vec<(String, Box<dyn DiscoveryPlugin>, PluginConfig)>,
	config: DiscoveryConfig,
}

impl DiscoveryServiceBuilder {
	pub fn new() -> Self {
		Self {
			plugins: Vec::new(),
			config: DiscoveryConfig {
				historical_sync: false,
				realtime_monitoring: true,
				dedupe_events: true,
				max_event_age_seconds: 300,
				max_events_per_second: 1000,
				event_buffer_size: 10000,
				deduplication_window_seconds: 300,
				max_concurrent_sources: 10,
			},
		}
	}

	pub fn with_plugin(
		mut self,
		name: String,
		plugin: Box<dyn DiscoveryPlugin>,
		config: PluginConfig,
	) -> Self {
		// Store plugin with its config for later initialization
		self.plugins.push((name, plugin, config));
		self
	}

	pub fn with_config(mut self, config: DiscoveryConfig) -> Self {
		self.config = config;
		self
	}

	pub fn with_max_events_per_second(mut self, max: u64) -> Self {
		self.config.max_events_per_second = max;
		self
	}

	pub fn with_deduplication(mut self, enabled: bool, window_seconds: u64) -> Self {
		self.config.dedupe_events = enabled;
		self.config.deduplication_window_seconds = window_seconds;
		self
	}

	pub async fn build(self, event_sink: EventSink<Event>) -> DiscoveryService {
		let service = DiscoveryService::with_config(event_sink, self.config);

		// Initialize and register all plugins
		for (name, mut plugin, plugin_config) in self.plugins {
			// Initialize the plugin before registering
			match plugin.initialize(plugin_config).await {
				Ok(_) => {
					info!("Successfully initialized discovery plugin: {}", name);
					service.register_plugin(name, plugin).await;
				}
				Err(e) => {
					error!("Failed to initialize discovery plugin {}: {}", name, e);
					// Skip registration if initialization fails
				}
			}
		}

		service
	}
}

impl Default for DiscoveryServiceBuilder {
	fn default() -> Self {
		Self::new()
	}
}
