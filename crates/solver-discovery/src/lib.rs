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
use tracing::{error, info, warn};

use solver_types::configs::DiscoveryConfig;
use solver_types::plugins::{ChainId, DiscoveryPlugin, EventSink, PluginError, PluginResult};
use solver_types::{Event, PluginConfig};
use std::collections::HashMap;
use std::sync::Arc;

/// Discovery service that orchestrates multiple discovery plugins
#[derive(Debug)]
pub struct DiscoveryService {
	plugins: Arc<RwLock<HashMap<String, Arc<Mutex<Box<dyn DiscoveryPlugin>>>>>>,
	active_sources: Arc<RwLock<HashMap<String, DiscoverySource>>>,
	event_sink: EventSink<Event>,
	config: DiscoveryConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoverySource {
	pub plugin_name: String,
	pub chain_id: ChainId,
	pub source_type: String,
	pub status: SourceStatus,
}

#[derive(Debug, Clone, Serialize)]
pub enum SourceStatus {
	Stopped,
	Starting,
	Running,
	Error(String),
	Stopping,
}

impl DiscoveryService {
	pub fn new(event_sink: EventSink<Event>) -> Self {
		let default_config = DiscoveryConfig {
			realtime_monitoring: true,
			max_events_per_second: 1000,
			max_concurrent_sources: 10,
		};
		Self::with_config(event_sink, default_config)
	}

	pub fn with_config(event_sink: EventSink<Event>, config: DiscoveryConfig) -> Self {
		Self {
			plugins: Arc::new(RwLock::new(HashMap::new())),
			active_sources: Arc::new(RwLock::new(HashMap::new())),
			event_sink,
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

	/// Create a filtered event sink for this plugin
	async fn create_filtered_sink(&self, _plugin_name: String) -> EventSink<Event> {
		let (tx, mut rx) = mpsc::unbounded_channel();
		let main_sink = self.event_sink.clone();

		// Spawn task to process events from this plugin
		tokio::spawn(async move {
			while let Some(event) = rx.recv().await {
				// Forward to main sink
				if let Err(e) = main_sink.send(event) {
					error!("Failed to forward event to main sink: {}", e);
				}
			}
		});

		EventSink::new(tx)
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
				realtime_monitoring: true,
				max_events_per_second: 1000,
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
