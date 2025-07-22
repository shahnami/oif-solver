//! # Settlement Service
//!
//! Manages cross-chain settlement operations and dispute resolution.
//!
//! This crate provides the settlement service that monitors confirmed fills for
//! settlement readiness conditions, manages oracle attestations, handles disputes,
//! and coordinates the settlement process through plugin-based strategies.
//! It tracks claim windows, monitors attestation status, and emits settlement
//! ready events when conditions are met.
//!
//! ## Key Features
//!
//! - **Fill Monitoring**: Tracks confirmed fills for settlement conditions
//! - **Oracle Integration**: Monitors oracle attestations and dispute status
//! - **Claim Window Management**: Enforces timing constraints for settlements
//! - **Dispute Handling**: Manages dispute detection and resolution processes
//! - **Settlement Strategies**: Supports multiple settlement plugins with fallback

use solver_types::configs::SettlementConfig;
use solver_types::events::{Event, SettlementReadyEvent};
use solver_types::plugins::{
	AttestationStatus, ClaimWindow, DisputeData, DisputeResolution, EventSink, FillData,
	PluginError, PluginResult, SettlementPlugin, SettlementReadiness,
};
use solver_types::{FillEvent, FillStatus, PluginConfig};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Utility function to truncate hashes for display purposes.
fn truncate_hash(hash: &str) -> String {
	if hash.len() <= 12 {
		hash.to_string()
	} else {
		format!("{}...{}", &hash[..6], &hash[hash.len() - 4..])
	}
}

/// Represents a fill being monitored for settlement conditions.
///
/// Tracks all information needed to monitor a confirmed fill through the
/// settlement process, including oracle attestations, claim windows, and
/// readiness status.
#[derive(Debug, Clone)]
pub struct MonitoredFill {
	/// The original fill event being monitored
	pub fill_event: FillEvent,
	/// Extracted fill data for settlement processing
	pub fill_data: FillData,
	/// Type of order being settled
	pub order_type: String,
	/// Name of the settlement plugin handling this fill
	pub plugin_name: String,
	/// Timestamp of last settlement condition check
	pub last_check: u64,
	/// Current oracle attestation status
	pub attestation_status: Option<AttestationStatus>,
	/// Settlement claim window information
	pub claim_window: Option<ClaimWindow>,
	/// Current settlement readiness status
	pub readiness: Option<SettlementReadiness>,
}

/// Tracks dispute information and resolution status.
///
/// Maintains the state of disputes that occur during the settlement process,
/// including resolution outcomes and timing information.
#[derive(Debug, Clone)]
pub struct DisputeTracker {
	/// Fill event that is being disputed
	pub fill_event: FillEvent,
	/// Details of the dispute
	pub dispute_data: DisputeData,
	/// Settlement plugin handling the dispute
	pub plugin_name: String,
	/// Resolution outcome if dispute is resolved
	pub resolution: Option<DisputeResolution>,
	/// Timestamp when dispute was first detected
	pub created_at: u64,
}

/// Settlement orchestration service that monitors fills and coordinates settlement.
///
/// The settlement service manages the complex process of cross-chain settlement
/// by monitoring confirmed fills, checking oracle attestations, managing claim
/// windows, handling disputes, and emitting settlement ready events when
/// all conditions are satisfied.
pub struct SettlementService {
	/// Registry of available settlement plugins by name
	settlement_plugins: Arc<RwLock<HashMap<String, Arc<dyn SettlementPlugin>>>>,
	/// Service configuration including strategies and timing
	config: SettlementConfig,
	/// Currently monitored fills awaiting settlement
	monitored_fills: Arc<RwLock<HashMap<String, MonitoredFill>>>,
	/// Active disputes being tracked
	active_disputes: Arc<RwLock<HashMap<String, DisputeTracker>>>,
	/// Event sink for emitting settlement ready events
	event_sink: Option<EventSink<Event>>,
	/// Shutdown flag for stopping monitoring loop
	shutdown: Arc<RwLock<bool>>,
}

impl Default for SettlementService {
	fn default() -> Self {
		Self {
			settlement_plugins: Arc::new(RwLock::new(HashMap::new())),
			config: SettlementConfig {
				default_strategy: "direct_settlement".to_string(),
				fallback_strategies: vec![],
				profit_threshold_wei: "0".to_string(),
				monitor_interval_seconds: 10,
			},
			monitored_fills: Arc::new(RwLock::new(HashMap::new())),
			active_disputes: Arc::new(RwLock::new(HashMap::new())),
			event_sink: None,
			shutdown: Arc::new(RwLock::new(false)),
		}
	}
}

impl SettlementService {
	/// Create a new settlement service with default configuration.
	pub fn new() -> Self {
		Self::default()
	}

	/// Create a settlement service with the specified configuration.
	///
	/// # Arguments
	/// * `config` - Settlement service configuration including strategies and timing
	pub fn with_config(config: SettlementConfig) -> Self {
		Self {
			settlement_plugins: Arc::new(RwLock::new(HashMap::new())),
			config,
			monitored_fills: Arc::new(RwLock::new(HashMap::new())),
			active_disputes: Arc::new(RwLock::new(HashMap::new())),
			event_sink: None,
			shutdown: Arc::new(RwLock::new(false)),
		}
	}

	/// Set the event sink for emitting settlement ready events.
	///
	/// # Arguments
	/// * `sink` - Event sink for forwarding settlement ready events
	pub fn set_event_sink(&mut self, sink: EventSink<Event>) {
		self.event_sink = Some(sink);
	}

	/// Begin monitoring a confirmed fill for settlement conditions.
	///
	/// Adds the fill to the monitoring queue and begins tracking oracle
	/// attestations, claim windows, and dispute status. Only confirmed fills
	/// are monitored for settlement.
	///
	/// # Arguments
	/// * `fill_event` - The confirmed fill event to monitor
	///
	/// # Returns
	/// Success if monitoring starts, error otherwise
	pub async fn monitor_fill(&self, fill_event: FillEvent) -> PluginResult<()> {
		info!(
			"Starting to monitor fill {} for order {}",
			truncate_hash(&fill_event.fill_id),
			truncate_hash(&fill_event.order_id)
		);

		// Only monitor confirmed fills
		if fill_event.status != FillStatus::Confirmed {
			return Ok(());
		}

		// Extract order type from source
		let order_type = fill_event.source.clone();

		// Create fill data from the event
		let fill_data = FillData {
			order_id: fill_event.order_id.clone(),
			fill_tx_hash: fill_event.tx_hash.clone(),
			fill_timestamp: fill_event.timestamp,
			chain_id: fill_event.chain_id,
			order_data: fill_event.order_data.clone(),
		};

		// Select appropriate plugin
		let plugin_name = self
			.select_plugin_for_fill(&fill_event, &order_type)
			.await?;

		// Create monitored fill entry
		let monitored_fill = MonitoredFill {
			fill_event: fill_event.clone(),
			fill_data,
			order_type,
			plugin_name,
			last_check: chrono::Utc::now().timestamp() as u64,
			attestation_status: None,
			claim_window: None,
			readiness: None,
		};

		self.monitored_fills
			.write()
			.await
			.insert(fill_event.fill_id.clone(), monitored_fill);

		Ok(())
	}

	/// Start the background monitoring loop for all tracked fills.
	///
	/// Spawns a background task that periodically checks all monitored fills
	/// for settlement readiness conditions. The loop runs at the configured
	/// monitoring interval until shutdown is requested.
	pub async fn start_monitoring(&self) {
		let interval_secs = self.config.monitor_interval_seconds;
		let service = self.clone();

		tokio::spawn(async move {
			let mut ticker = interval(Duration::from_secs(interval_secs));

			loop {
				ticker.tick().await;

				// Check shutdown flag
				if *service.shutdown.read().await {
					info!("Settlement monitoring loop shutting down");
					break;
				}

				if let Err(e) = service.check_monitored_fills().await {
					error!("Error checking monitored fills: {}", e);
				}
			}
		});
	}

	/// Check all monitored fills for settlement readiness
	async fn check_monitored_fills(&self) -> PluginResult<()> {
		let fills = self.monitored_fills.read().await.clone();

		if !fills.is_empty() {
			debug!("Checking {} monitored fills", fills.len());
		}

		for (fill_id, mut monitored_fill) in fills {
			if let Err(e) = self.check_single_fill(&fill_id, &mut monitored_fill).await {
				warn!("Error checking fill {}: {}", fill_id, e);
			}
		}

		Ok(())
	}

	/// Check a single fill for settlement readiness
	async fn check_single_fill(
		&self,
		fill_id: &str,
		monitored_fill: &mut MonitoredFill,
	) -> PluginResult<()> {
		// Check if this fill has expired (24 hours since last check)
		let now = chrono::Utc::now().timestamp() as u64;
		if now - monitored_fill.last_check > 86400 {
			info!(
				"Fill {} has expired after 24 hours, removing from monitoring",
				fill_id
			);
			self.monitored_fills.write().await.remove(fill_id);
			return Ok(());
		}

		let plugins = self.settlement_plugins.read().await;
		let plugin = plugins.get(&monitored_fill.plugin_name).ok_or_else(|| {
			PluginError::NotFound(format!(
				"Settlement plugin '{}' not found",
				monitored_fill.plugin_name
			))
		})?;

		// Verify settlement conditions (this will check attestation and claim window internally)
		let readiness = plugin
			.verify_settlement_conditions(&monitored_fill.fill_data)
			.await?;

		// Extract attestation status and claim window from readiness
		monitored_fill.attestation_status = Some(readiness.oracle_status.clone());
		monitored_fill.claim_window = Some(readiness.claim_window.clone());
		monitored_fill.readiness = Some(readiness.clone());

		// Update last check time
		monitored_fill.last_check = chrono::Utc::now().timestamp() as u64;

		// Check if this fill is newly disputed
		let is_newly_disputed = readiness.oracle_status.is_disputed
			&& monitored_fill
				.attestation_status
				.as_ref()
				.map(|s| !s.is_disputed)
				.unwrap_or(true);

		// Handle dispute if newly detected
		if is_newly_disputed {
			info!("Dispute detected for fill {}", truncate_hash(fill_id));

			// Create dispute data from oracle information
			let dispute_data = DisputeData {
				disputer: readiness
					.oracle_status
					.oracle_address
					.clone()
					.unwrap_or_else(|| "unknown".to_string()),
				dispute_reason: "Oracle reported dispute".to_string(),
				dispute_time: chrono::Utc::now().timestamp() as u64,
				evidence: None,
			};

			// Handle the dispute
			match self.handle_dispute(fill_id, dispute_data).await {
				Ok(resolution) => {
					info!(
						"Dispute handled for fill {}: {:?}",
						truncate_hash(fill_id),
						resolution
					);
				}
				Err(e) => {
					error!("Failed to handle dispute for fill {}: {}", fill_id, e);
				}
			}
		}

		// If ready, emit settlement ready event
		if readiness.is_ready {
			info!("Fill {} is ready for settlement", truncate_hash(fill_id));
			self.emit_settlement_ready_event(monitored_fill).await?;

			// Remove from monitoring
			self.monitored_fills.write().await.remove(fill_id);
		} else {
			debug!(
				"Fill {} not ready for settlement: {:?}",
				fill_id, readiness.reasons
			);
			debug!(
				"Attestation status: is_attested={}, is_disputed={}, attestation_time={:?}",
				readiness.oracle_status.is_attested,
				readiness.oracle_status.is_disputed,
				readiness.oracle_status.attestation_time
			);
			debug!(
				"Claim window: start={}, end={}, is_active={}",
				readiness.claim_window.start,
				readiness.claim_window.end,
				readiness.claim_window.is_active
			);

			// Update the monitored fill
			self.monitored_fills
				.write()
				.await
				.insert(fill_id.to_string(), monitored_fill.clone());
		}

		Ok(())
	}

	/// Emit a settlement ready event
	async fn emit_settlement_ready_event(
		&self,
		monitored_fill: &MonitoredFill,
	) -> PluginResult<()> {
		let event = SettlementReadyEvent {
			fill_event: monitored_fill.fill_event.clone(),
			oracle_attestation_id: monitored_fill
				.attestation_status
				.as_ref()
				.and_then(|s| s.attestation_id.clone()),
			claim_window_start: monitored_fill
				.claim_window
				.as_ref()
				.map(|w| w.start)
				.unwrap_or(0),
			claim_window_end: monitored_fill
				.claim_window
				.as_ref()
				.map(|w| w.end)
				.unwrap_or(0),
		};

		// Emit event
		if let Some(sink) = &self.event_sink {
			sink.send(Event::SettlementReady(event)).map_err(|e| {
				PluginError::ExecutionFailed(format!("Failed to emit event: {}", e))
			})?;
			info!(
				"Emitted SettlementReadyEvent for fill {}",
				truncate_hash(&monitored_fill.fill_event.fill_id)
			);
		} else {
			warn!("No event sink configured, cannot emit SettlementReadyEvent");
		}

		Ok(())
	}

	/// Handle a dispute for a monitored fill.
	///
	/// Processes dispute information through the appropriate settlement plugin
	/// and tracks the dispute resolution outcome. Creates a dispute tracker
	/// to maintain dispute state.
	///
	/// # Arguments
	/// * `fill_id` - ID of the fill being disputed
	/// * `dispute_data` - Details of the dispute
	///
	/// # Returns
	/// The dispute resolution outcome
	pub async fn handle_dispute(
		&self,
		fill_id: &str,
		dispute_data: DisputeData,
	) -> PluginResult<DisputeResolution> {
		let fills = self.monitored_fills.read().await;
		let monitored_fill = fills.get(fill_id).ok_or_else(|| {
			PluginError::NotFound(format!("Fill '{}' not found in monitoring", fill_id))
		})?;

		let plugins = self.settlement_plugins.read().await;
		let plugin = plugins.get(&monitored_fill.plugin_name).ok_or_else(|| {
			PluginError::NotFound(format!(
				"Settlement plugin '{}' not found",
				monitored_fill.plugin_name
			))
		})?;

		// Handle dispute through plugin
		let resolution = plugin
			.handle_dispute(&monitored_fill.fill_data, &dispute_data)
			.await?;

		// Track dispute
		let dispute_tracker = DisputeTracker {
			fill_event: monitored_fill.fill_event.clone(),
			dispute_data,
			plugin_name: monitored_fill.plugin_name.clone(),
			resolution: Some(resolution.clone()),
			created_at: chrono::Utc::now().timestamp() as u64,
		};

		self.active_disputes
			.write()
			.await
			.insert(fill_id.to_string(), dispute_tracker);

		Ok(resolution)
	}

	/// Select the best plugin for a fill
	async fn select_plugin_for_fill(
		&self,
		fill_event: &FillEvent,
		order_type: &str,
	) -> PluginResult<String> {
		let plugins = self.settlement_plugins.read().await;

		// Try default strategy first
		if let Some(plugin) = plugins.get(&self.config.default_strategy) {
			if plugin.can_handle(fill_event.chain_id, order_type).await? {
				return Ok(self.config.default_strategy.clone());
			}
		}

		// Try fallback strategies
		for strategy in &self.config.fallback_strategies {
			if let Some(plugin) = plugins.get(strategy) {
				if plugin.can_handle(fill_event.chain_id, order_type).await? {
					return Ok(strategy.clone());
				}
			}
		}

		Err(PluginError::NotFound(format!(
			"No settlement plugin available for chain {} and order type {}",
			fill_event.chain_id, order_type
		)))
	}

	/// Register a new settlement plugin
	pub async fn register_plugin(&self, name: String, plugin: Arc<dyn SettlementPlugin>) {
		info!("Registering settlement plugin: {}", name);
		info!("Starting {}", name);
		self.settlement_plugins
			.write()
			.await
			.insert(name.clone(), plugin);
		info!("{} started successfully", name);
	}

	/// Stop monitoring
	pub async fn stop_monitoring(&self) {
		info!("Stopping settlement monitoring");
		*self.shutdown.write().await = true;
	}
}

/// Builder for constructing SettlementService instances.
///
/// Provides a fluent interface for configuring settlement services with
/// plugins, event sinks, and configuration options. Handles plugin
/// initialization during the build process.
pub struct SettlementServiceBuilder {
	/// Settlement plugins to register with their configurations
	plugins: Vec<(String, Box<dyn SettlementPlugin>, PluginConfig)>,
	/// Service configuration
	config: SettlementConfig,
	/// Optional event sink for settlement ready events
	event_sink: Option<EventSink<Event>>,
}

impl SettlementServiceBuilder {
	/// Create a new settlement service builder with default configuration.
	pub fn new() -> Self {
		Self {
			plugins: Vec::new(),
			config: SettlementConfig {
				default_strategy: "direct_settlement".to_string(),
				fallback_strategies: vec![],
				profit_threshold_wei: "0".to_string(),
				monitor_interval_seconds: 10,
			},
			event_sink: None,
		}
	}

	/// Set the event sink for settlement ready events.
	///
	/// # Arguments
	/// * `event_sink` - Event sink for forwarding settlement events
	pub fn with_event_sink(mut self, event_sink: EventSink<Event>) -> Self {
		self.event_sink = Some(event_sink);
		self
	}

	/// Add a settlement plugin to be registered with the service.
	///
	/// # Arguments
	/// * `name` - Unique name for the plugin
	/// * `plugin` - Settlement plugin implementation
	/// * `config` - Plugin-specific configuration
	pub fn with_plugin(
		mut self,
		name: String,
		plugin: Box<dyn SettlementPlugin>,
		config: PluginConfig,
	) -> Self {
		self.plugins.push((name, plugin, config));
		self
	}

	/// Set the settlement service configuration.
	///
	/// # Arguments
	/// * `config` - Service configuration including strategies and timing
	pub fn with_config(mut self, config: SettlementConfig) -> Self {
		self.config = config;
		self
	}

	/// Build the settlement service with all configured plugins.
	///
	/// Initializes all plugins, sets up the event sink, and creates the
	/// service instance. Plugin initialization failures are logged but
	/// do not prevent service creation.
	///
	/// # Returns
	/// Configured settlement service ready for monitoring
	pub async fn build(self) -> SettlementService {
		let mut service = SettlementService::with_config(self.config);

		// Set event sink if provided
		if let Some(event_sink) = self.event_sink {
			service.set_event_sink(event_sink);
		}

		// Initialize and register all plugins
		for (name, mut plugin, plugin_config) in self.plugins {
			// Initialize the plugin before registering
			match plugin.initialize(plugin_config).await {
				Ok(_) => {
					debug!("Successfully initialized settlement plugin: {}", name);
					service.register_plugin(name, Arc::from(plugin)).await;
				}
				Err(e) => {
					error!("Failed to initialize settlement plugin {}: {}", name, e);
					// Skip registration if initialization fails
				}
			}
		}

		service
	}
}

impl Default for SettlementServiceBuilder {
	fn default() -> Self {
		Self::new()
	}
}

impl Clone for SettlementService {
	fn clone(&self) -> Self {
		Self {
			settlement_plugins: self.settlement_plugins.clone(),
			config: self.config.clone(),
			monitored_fills: self.monitored_fills.clone(),
			active_disputes: self.active_disputes.clone(),
			event_sink: self.event_sink.clone(),
			shutdown: self.shutdown.clone(),
		}
	}
}
