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
//! - Event filtering
//! - Multi-chain monitoring

use serde::Serialize;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use solver_types::configs::DiscoveryConfig;
use solver_types::plugins::{ChainId, DiscoveryPlugin, EventSink, PluginError, PluginResult};
use solver_types::{Event, PluginConfig};
use std::collections::HashMap;
use std::sync::Arc;

type DiscoveryPluginType = Arc<RwLock<HashMap<String, Arc<Mutex<Box<dyn DiscoveryPlugin>>>>>>;

/// Discovery service that orchestrates multiple discovery plugins.
///
/// The discovery service manages a collection of discovery plugins that monitor
/// various sources for order events. It provides lifecycle management, event
/// routing, and coordination between plugins while enforcing rate limits and
/// concurrency constraints.
#[derive(Debug)]
pub struct DiscoveryService {
	/// Registry of registered discovery plugins
	plugins: DiscoveryPluginType,
	/// Currently active discovery sources with their status
	active_sources: Arc<RwLock<HashMap<String, DiscoverySource>>>,
	/// Event sink for forwarding discovered events
	event_sink: EventSink<Event>,
	/// Service configuration and limits
	config: DiscoveryConfig,
}

/// Represents an active discovery source with its current status.
///
/// Tracks the operational state of a discovery plugin instance, including
/// the chain it monitors and its current operational status.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoverySource {
	/// Name of the plugin providing this source
	pub plugin_name: String,
	/// Blockchain network being monitored
	pub chain_id: ChainId,
	/// Type of discovery source (e.g., "eip7683_onchain")
	pub source_type: String,
	/// Current operational status of the source
	pub status: SourceStatus,
}

/// Operational status of a discovery source.
///
/// Represents the current state of a discovery source throughout its lifecycle
/// from initialization through active monitoring to shutdown.
#[derive(Debug, Clone, Serialize)]
pub enum SourceStatus {
	/// Source is not currently active
	Stopped,
	/// Source is in the process of starting up
	Starting,
	/// Source is actively monitoring for events
	Running,
	/// Source encountered an error and is not operational
	Error(String),
	/// Source is in the process of shutting down
	Stopping,
}

impl DiscoveryService {
	/// Create a new discovery service with default configuration.
	///
	/// # Arguments
	/// * `event_sink` - Event sink for forwarding discovered events
	pub fn new(event_sink: EventSink<Event>) -> Self {
		let default_config = DiscoveryConfig {
			realtime_monitoring: true,
			max_events_per_second: 1000,
			max_concurrent_sources: 10,
		};
		Self::with_config(event_sink, default_config)
	}

	/// Create a discovery service with the specified configuration.
	///
	/// # Arguments
	/// * `event_sink` - Event sink for forwarding discovered events
	/// * `config` - Discovery service configuration and limits
	pub fn with_config(event_sink: EventSink<Event>, config: DiscoveryConfig) -> Self {
		Self {
			plugins: Arc::new(RwLock::new(HashMap::new())),
			active_sources: Arc::new(RwLock::new(HashMap::new())),
			event_sink,
			config,
		}
	}

	/// Register a discovery plugin with the service.
	///
	/// Adds a discovery plugin to the service registry, making it available
	/// for starting and monitoring operations.
	///
	/// # Arguments
	/// * `name` - Unique name for the plugin
	/// * `plugin` - Discovery plugin implementation
	pub async fn register_plugin(&self, name: String, plugin: Box<dyn DiscoveryPlugin>) {
		info!("Registering discovery plugin: {}", name);
		self.plugins
			.write()
			.await
			.insert(name, Arc::new(Mutex::new(plugin)));
	}

	/// Start monitoring with a specific discovery plugin.
	///
	/// Initializes and starts monitoring for the specified plugin, creating
	/// a filtered event sink and tracking the source status. Enforces
	/// concurrency limits and handles plugin lifecycle.
	///
	/// # Arguments
	/// * `plugin_name` - Name of the plugin to start
	///
	/// # Returns
	/// Success if the plugin starts monitoring, error otherwise
	///
	/// # Errors
	/// Returns error if plugin not found, concurrency limit reached, or start fails
	pub async fn start_source(&self, plugin_name: &str) -> PluginResult<()> {
		let plugin = {
			let plugins = self.plugins.read().await;
			plugins.get(plugin_name).cloned().ok_or_else(|| {
				PluginError::NotFound(format!("Plugin not found: {}", plugin_name))
			})?
		};

		info!("Starting {}", plugin_name);

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
		info!("{} started successfully", plugin_name);
		Ok(())
	}

	/// Stop monitoring for a specific discovery plugin.
	///
	/// Gracefully stops the specified plugin's monitoring operations and
	/// removes it from the active sources registry.
	///
	/// # Arguments
	/// * `plugin_name` - Name of the plugin to stop
	///
	/// # Returns
	/// Success if the plugin stops successfully, error otherwise
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

	/// Start monitoring for all registered discovery plugins.
	///
	/// Attempts to start all registered plugins concurrently, collecting
	/// any errors that occur during startup. Partial failures are reported
	/// but do not prevent other plugins from starting.
	///
	/// # Returns
	/// Success if all plugins start successfully, error with details of failures
	pub async fn start_all(&self) -> PluginResult<()> {
		let plugin_names: Vec<String> = { self.plugins.read().await.keys().cloned().collect() };

		let mut errors = Vec::new();
		for plugin_name in plugin_names {
			if let Err(e) = self.start_source(&plugin_name).await {
				errors.push(format!("{}: {}", plugin_name, e));
			}
		}

		if errors.is_empty() {
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

/// Builder for constructing DiscoveryService instances.
///
/// Provides a fluent interface for configuring discovery services with
/// plugins and settings. Handles plugin initialization during the build process.
pub struct DiscoveryServiceBuilder {
	/// Plugins to register with their configurations
	plugins: Vec<(String, Box<dyn DiscoveryPlugin>, PluginConfig)>,
	/// Service configuration
	config: DiscoveryConfig,
}

impl DiscoveryServiceBuilder {
	/// Create a new discovery service builder with default configuration.
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

	/// Add a discovery plugin to be registered with the service.
	///
	/// # Arguments
	/// * `name` - Unique name for the plugin
	/// * `plugin` - Discovery plugin implementation
	/// * `config` - Plugin-specific configuration
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

	/// Set the discovery service configuration.
	///
	/// # Arguments
	/// * `config` - Service configuration including limits and behavior
	pub fn with_config(mut self, config: DiscoveryConfig) -> Self {
		self.config = config;
		self
	}

	/// Build the discovery service with all configured plugins.
	///
	/// Initializes all plugins and registers them with the service.
	/// Plugin initialization failures are logged but do not prevent
	/// service creation.
	///
	/// # Arguments
	/// * `event_sink` - Event sink for forwarding discovered events
	///
	/// # Returns
	/// Configured discovery service ready for use
	pub async fn build(self, event_sink: EventSink<Event>) -> DiscoveryService {
		let service = DiscoveryService::with_config(event_sink, self.config);

		// Initialize and register all plugins
		for (name, mut plugin, plugin_config) in self.plugins {
			// Initialize the plugin before registering
			match plugin.initialize(plugin_config).await {
				Ok(_) => {
					debug!("Successfully initialized discovery plugin: {}", name);
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
