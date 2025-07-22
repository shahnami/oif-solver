//! # EIP-7683 On-chain Discovery Plugin
//!
//! Provides on-chain discovery for EIP-7683 cross-chain order events.
//!
//! This plugin monitors Ethereum-compatible blockchains for EIP-7683 order
//! events including order creation (Open), completion (Finalised), and
//! updates (OrderPurchased) from specified settler contracts.

use async_trait::async_trait;
use bytes::Bytes;
use ethers::providers::{Http, Middleware, Provider};
use ethers::types::{Address as EthAddress, Filter, Log, H256};
use hex;
use serde::{Deserialize, Serialize};
use solver_types::plugins::*;
use solver_types::Event;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// State tracking for blockchain monitoring.
///
/// Maintains shared state between the monitoring task and the plugin
/// instance for tracking progress and metrics.
#[derive(Debug, Default)]
struct MonitorState {
	/// Current block being processed
	current_block: Arc<RwLock<Option<u64>>>,
	/// Target block to sync to
	target_block: Arc<RwLock<Option<u64>>>,
	/// Total events discovered
	events_discovered: Arc<RwLock<u64>>,
	/// Total errors encountered
	errors_count: Arc<RwLock<u64>>,
	/// Timestamp of last discovered event
	last_event_timestamp: Arc<RwLock<Option<Timestamp>>>,
}

/// EIP-7683 on-chain discovery plugin implementation.
///
/// Monitors blockchain events from EIP-7683 settler contracts using
/// ethers-rs for Ethereum RPC communication.
#[derive(Debug)]
pub struct Eip7683OnchainDiscoveryPlugin {
	/// Plugin configuration
	config: Eip7683OnchainConfig,
	/// Ethereum RPC provider
	provider: Option<Arc<Provider<Http>>>,
	/// Plugin performance metrics
	metrics: PluginMetrics,
	/// Whether plugin is initialized
	is_initialized: bool,
	/// Whether monitoring is active
	is_monitoring: bool,

	/// Discovery state tracking
	state: MonitorState,

	/// Channel to signal monitoring task to stop
	stop_tx: Option<mpsc::UnboundedSender<()>>,

	/// Active event filters
	active_filters: Arc<RwLock<Vec<EventFilter>>>,
}

/// Configuration for EIP-7683 on-chain discovery.
///
/// Defines the blockchain connection parameters, contract addresses,
/// and monitoring behavior for the discovery plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip7683OnchainConfig {
	/// Chain ID to monitor
	pub chain_id: ChainId,
	/// RPC endpoint URL
	pub rpc_url: String,
	/// Request timeout in milliseconds
	pub timeout_ms: u64,
	/// Maximum retry attempts for failed requests
	pub max_retries: u32,

	/// Input settler contract addresses to monitor
	pub input_settler_addresses: Vec<String>,
	/// Output settler contract addresses to monitor
	pub output_settler_addresses: Vec<String>,

	/// Whether to monitor Open events (order creation)
	pub monitor_open: bool,
	/// Whether to monitor Finalised events (order completion)
	pub monitor_finalised: bool,
	/// Whether to monitor OrderPurchased events (order updates)
	pub monitor_order_purchased: bool,

	/// Number of events to process in batch
	pub batch_size: u32,
	/// Block polling interval in milliseconds
	pub poll_interval_ms: u64,
	/// Maximum blocks to query per request
	pub max_blocks_per_request: u64,

	/// Enable historical event synchronization
	pub enable_historical_sync: bool,
	/// Starting block for historical sync
	pub historical_start_block: Option<u64>,
}

impl Default for Eip7683OnchainConfig {
	fn default() -> Self {
		Self {
			chain_id: 1,
			rpc_url: "https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY".to_string(),
			timeout_ms: 30000,
			max_retries: 3,
			input_settler_addresses: Vec::new(),
			output_settler_addresses: Vec::new(),
			monitor_open: true,
			monitor_finalised: true,
			monitor_order_purchased: true,
			batch_size: 100,
			poll_interval_ms: 12000, // 12 seconds (Ethereum block time)
			max_blocks_per_request: 1000,
			enable_historical_sync: false,
			historical_start_block: None,
		}
	}
}

// EIP-7683 specific event signatures
const OPEN_EVENT_SIGNATURE: &str = "Open(bytes32,(address,uint256,uint32,uint32,bytes32,(bytes32,uint256,bytes32,uint256)[],(bytes32,uint256,bytes32,uint256)[],(uint64,bytes32,bytes)[]))";
const FINALISED_EVENT_SIGNATURE: &str = "Finalised(bytes32,bytes32,bytes32)";
const ORDER_PURCHASED_EVENT_SIGNATURE: &str = "OrderPurchased(bytes32,bytes32,bytes32)";

impl Default for Eip7683OnchainDiscoveryPlugin {
	fn default() -> Self {
		Self::new()
	}
}

impl Eip7683OnchainDiscoveryPlugin {
	/// Create a new plugin instance with default configuration.
	pub fn new() -> Self {
		Self {
			config: Eip7683OnchainConfig::default(),
			provider: None,
			metrics: PluginMetrics::new(),
			is_initialized: false,
			is_monitoring: false,
			state: MonitorState::default(),
			stop_tx: None,
			active_filters: Arc::new(RwLock::new(Vec::new())),
		}
	}

	/// Create a new plugin instance with specified configuration.
	pub fn with_config(config: Eip7683OnchainConfig) -> Self {
		Self {
			config,
			provider: None,
			metrics: PluginMetrics::new(),
			is_initialized: false,
			is_monitoring: false,
			state: MonitorState::default(),
			stop_tx: None,
			active_filters: Arc::new(RwLock::new(Vec::new())),
		}
	}

	/// Setup the Ethereum RPC provider.
	///
	/// Establishes connection to the blockchain and verifies chain ID.
	async fn setup_provider(&mut self) -> PluginResult<()> {
		debug!(
			"Setting up ethers provider for EIP-7683 discovery on chain {}",
			self.config.chain_id
		);

		let provider = Provider::<Http>::try_from(&self.config.rpc_url)
			.map_err(|e| PluginError::InitializationFailed(format!("Invalid RPC URL: {}", e)))?
			.interval(Duration::from_millis(self.config.poll_interval_ms));

		// Verify chain ID
		let chain_id = provider.get_chainid().await.map_err(|e| {
			PluginError::InitializationFailed(format!("Failed to get chain ID: {}", e))
		})?;

		if chain_id.as_u64() != self.config.chain_id {
			return Err(PluginError::InitializationFailed(format!(
				"Chain ID mismatch: expected {}, got {}",
				self.config.chain_id,
				chain_id.as_u64()
			)));
		}

		self.provider = Some(Arc::new(provider));
		Ok(())
	}

	/// Extract Ethereum addresses from configuration.
	///
	/// Parses and validates contract addresses for monitoring.
	fn get_contract_addresses(config: &Eip7683OnchainConfig) -> Vec<EthAddress> {
		let mut addresses = Vec::new();

		// Add input settler addresses
		for addr_str in &config.input_settler_addresses {
			if let Ok(addr) = addr_str.parse::<EthAddress>() {
				addresses.push(addr);
			} else {
				warn!("Invalid input settler address: {}", addr_str);
			}
		}

		// Add output settler addresses
		for addr_str in &config.output_settler_addresses {
			if let Ok(addr) = addr_str.parse::<EthAddress>() {
				addresses.push(addr);
			} else {
				warn!("Invalid output settler address: {}", addr_str);
			}
		}

		addresses
	}

	/// Generate event signatures for filtering.
	///
	/// Creates Keccak256 hashes of event signatures based on
	/// which event types are enabled in configuration.
	fn get_event_signatures(config: &Eip7683OnchainConfig) -> Vec<H256> {
		let mut signatures = Vec::new();

		if config.monitor_open {
			signatures.push(H256::from_slice(&ethers::utils::keccak256(
				OPEN_EVENT_SIGNATURE.as_bytes(),
			)));
		}

		if config.monitor_finalised {
			signatures.push(H256::from_slice(&ethers::utils::keccak256(
				FINALISED_EVENT_SIGNATURE.as_bytes(),
			)));
		}

		if config.monitor_order_purchased {
			signatures.push(H256::from_slice(&ethers::utils::keccak256(
				ORDER_PURCHASED_EVENT_SIGNATURE.as_bytes(),
			)));
		}

		signatures
	}

	/// Create an Ethereum log filter for event discovery.
	///
	/// Constructs a filter with contract addresses, event signatures,
	/// and block range for querying blockchain logs.
	async fn create_filter(
		config: &Eip7683OnchainConfig,
		from_block: Option<u64>,
		to_block: Option<u64>,
	) -> Filter {
		let addresses = Self::get_contract_addresses(config);
		let signatures = Self::get_event_signatures(config);

		let mut filter = Filter::new();

		if !addresses.is_empty() {
			filter = filter.address(addresses);
		}

		if !signatures.is_empty() {
			// Set topic0 to any of our event signatures
			filter = filter.topic0(signatures);
		}

		if let Some(from) = from_block {
			filter = filter.from_block(from);
		}

		if let Some(to) = to_block {
			filter = filter.to_block(to);
		}

		filter
	}

	/// Parse blockchain log into a discovery event.
	///
	/// Converts raw Ethereum log data into a structured discovery
	/// event with decoded parameters and metadata.
	async fn parse_log_to_discovery_event(
		config: &Eip7683OnchainConfig,
		log: &Log,
	) -> PluginResult<Option<DiscoveryEvent>> {
		// Determine event type from topic0
		let event_type = if !log.topics.is_empty() {
			let topic0 = log.topics[0];
			let open_hash =
				H256::from_slice(&ethers::utils::keccak256(OPEN_EVENT_SIGNATURE.as_bytes()));
			let finalised_hash = H256::from_slice(&ethers::utils::keccak256(
				FINALISED_EVENT_SIGNATURE.as_bytes(),
			));
			let order_purchased_hash = H256::from_slice(&ethers::utils::keccak256(
				ORDER_PURCHASED_EVENT_SIGNATURE.as_bytes(),
			));

			if topic0 == open_hash {
				EventType::OrderCreated
			} else if topic0 == finalised_hash {
				EventType::OrderFilled // Finalised means the order was completed
			} else if topic0 == order_purchased_hash {
				EventType::OrderUpdated // OrderPurchased is an intermediate state
			} else {
				return Ok(None); // Unknown event
			}
		} else {
			return Ok(None);
		};

		// Extract order ID from topics[1] (orderId is indexed in all events)
		let order_id = if log.topics.len() > 1 {
			format!("0x{}", hex::encode(log.topics[1].as_bytes()))
		} else {
			format!(
				"{:?}_{}",
				log.transaction_hash.unwrap_or_default(),
				log.log_index.unwrap_or_default()
			)
		};

		// For EIP-7683, user address is in the ResolvedCrossChainOrder struct for Open events
		// For other events, we need to decode the data or use transaction context
		let user = if matches!(event_type, EventType::OrderCreated) {
			// User address is the first field in ResolvedCrossChainOrder struct
			if log.data.len() >= 32 {
				// Skip the first 32 bytes (offset to struct) and get the user address
				let user_offset = 32; // Offset to the user field in the struct
				if log.data.len() >= user_offset + 32 {
					let user_bytes = &log.data[user_offset..user_offset + 32];
					// Address is in the last 20 bytes of the 32-byte word
					let addr_bytes = &user_bytes[12..32];
					Some(format!("0x{}", hex::encode(addr_bytes)))
				} else {
					None
				}
			} else {
				None
			}
		} else {
			None
		};

		// Parse event data based on type
		let parsed_data = ParsedEventData {
			order_id: Some(order_id.clone()),
			user,
			contract_address: Some(format!("{:?}", log.address)),
			method_signature: Some(match event_type {
				EventType::OrderCreated => OPEN_EVENT_SIGNATURE.to_string(),
				EventType::OrderFilled => FINALISED_EVENT_SIGNATURE.to_string(),
				EventType::OrderUpdated => ORDER_PURCHASED_EVENT_SIGNATURE.to_string(),
				_ => "unknown".to_string(),
			}),
			decoded_params: Self::decode_event_params(&Bytes::from(log.data.to_vec()), &event_type),
		};

		// Calculate processing delay
		let now = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs();
		let processing_delay = log.block_number.map(|bn| {
			// Estimate block timestamp (rough calculation)
			let estimated_block_time = bn.as_u64() * 12; // 12 seconds per block
			(now - estimated_block_time) * 1000 // Convert to milliseconds
		});

		let discovery_event = DiscoveryEvent {
			id: order_id,
			event_type,
			source: "eip7683_onchain".to_string(),
			chain_id: config.chain_id,
			block_number: log.block_number.map(|bn| bn.as_u64()),
			transaction_hash: log.transaction_hash.map(|tx| format!("{:?}", tx)),
			timestamp: now,
			raw_data: Bytes::from(log.data.to_vec()),
			parsed_data: Some(parsed_data),
			metadata: EventMetadata {
				source_specific: {
					let mut meta = HashMap::new();
					meta.insert("contract_address".to_string(), format!("{:?}", log.address));
					meta.insert(
						"log_index".to_string(),
						log.log_index.unwrap_or_default().to_string(),
					);
					if let Some(removed) = log.removed {
						meta.insert("removed".to_string(), removed.to_string());
					}
					meta
				},
				confidence_score: 0.95, // High confidence for on-chain events
				processing_delay,
				retry_count: 0,
			},
		};

		Ok(Some(discovery_event))
	}

	/// Decode event parameters based on event type.
	///
	/// Extracts and decodes specific parameters from the event data
	/// based on the EIP-7683 event ABI definitions.
	fn decode_event_params(data: &Bytes, event_type: &EventType) -> HashMap<String, EventParam> {
		let mut params = HashMap::new();

		match event_type {
			EventType::OrderCreated => {
				// Open(bytes32 indexed orderId, ResolvedCrossChainOrder resolvedOrder)
				// The ResolvedCrossChainOrder struct contains:
				// - address user
				// - uint256 originChainId
				// - uint32 openDeadline
				// - uint32 fillDeadline
				// - bytes32 orderId
				// - Output[] maxSpent
				// - Output[] minReceived
				// - FillInstruction[] fillInstructions

				if data.len() >= 32 {
					// Store the full resolved order data for later parsing
					params.insert(
						"resolved_order_data".to_string(),
						EventParam::Bytes(data.clone()),
					);

					// Extract some key fields (simplified decoding)
					if data.len() >= 96 {
						// Origin chain ID is at offset 32 (after user address)
						let origin_chain_bytes = &data[32..64];
						let origin_chain = ethers::types::U256::from_big_endian(origin_chain_bytes);
						params.insert(
							"origin_chain_id".to_string(),
							EventParam::Uint256(origin_chain.to_string()),
						);

						// Open deadline at offset 64
						let open_deadline_bytes = &data[64..68];
						let open_deadline = u32::from_be_bytes([
							open_deadline_bytes[0],
							open_deadline_bytes[1],
							open_deadline_bytes[2],
							open_deadline_bytes[3],
						]);
						params.insert(
							"open_deadline".to_string(),
							EventParam::Uint256(open_deadline.to_string()),
						);

						// Fill deadline at offset 68
						let fill_deadline_bytes = &data[68..72];
						let fill_deadline = u32::from_be_bytes([
							fill_deadline_bytes[0],
							fill_deadline_bytes[1],
							fill_deadline_bytes[2],
							fill_deadline_bytes[3],
						]);
						params.insert(
							"fill_deadline".to_string(),
							EventParam::Uint256(fill_deadline.to_string()),
						);
					}
				}
			}
			EventType::OrderFilled => {
				// Finalised(bytes32 indexed orderId, bytes32 fillerData, bytes32 attestation)
				if data.len() >= 64 {
					// Filler data
					let filler_data = &data[0..32];
					params.insert(
						"filler_data".to_string(),
						EventParam::Bytes(Bytes::from(filler_data.to_vec())),
					);

					// Attestation
					let attestation = &data[32..64];
					params.insert(
						"attestation".to_string(),
						EventParam::Bytes(Bytes::from(attestation.to_vec())),
					);
				}
			}
			EventType::OrderUpdated => {
				// OrderPurchased(bytes32 indexed orderId, bytes32 fulfillerPubKey, bytes32 orderMetadata)
				if data.len() >= 64 {
					// Fulfiller public key
					let fulfiller_pub_key = &data[0..32];
					params.insert(
						"fulfiller_pub_key".to_string(),
						EventParam::Bytes(Bytes::from(fulfiller_pub_key.to_vec())),
					);

					// Order metadata
					let order_metadata = &data[32..64];
					params.insert(
						"order_metadata".to_string(),
						EventParam::Bytes(Bytes::from(order_metadata.to_vec())),
					);
				}
			}
			_ => {}
		}

		params
	}

	/// Background task for monitoring blockchain blocks.
	///
	/// Polls for new blocks and processes events, handling errors
	/// and maintaining synchronization state.
	async fn monitor_blocks_task(
		provider: Arc<Provider<Http>>,
		config: Eip7683OnchainConfig,
		sink: EventSink<Event>,
		mut stop_rx: mpsc::UnboundedReceiver<()>,
		state: MonitorState,
	) -> PluginResult<()> {
		let mut poll_interval = interval(Duration::from_millis(config.poll_interval_ms));

		// Get starting block
		let mut last_processed_block = if let Some(start) = config.historical_start_block {
			start
		} else {
			provider
				.get_block_number()
				.await
				.map_err(|e| {
					PluginError::ExecutionFailed(format!("Failed to get latest block: {}", e))
				})?
				.as_u64()
		};

		debug!(
			"Starting block monitoring from block {}",
			last_processed_block
		);

		loop {
			tokio::select! {
				_ = poll_interval.tick() => {
					match Self::process_new_blocks(
						&provider,
						&config,
						&sink,
						&mut last_processed_block,
						&state,
					).await {
						Ok(processed_count) => {
							if processed_count > 0 {
								debug!("Processed {} blocks", processed_count);
							}
						}
						Err(e) => {
							error!("Error processing blocks: {}", e);
							let mut errors = state.errors_count.write().await;
							*errors += 1;
						}
					}
				}
				_ = stop_rx.recv() => {
					info!("Received stop signal, stopping block monitoring");
					break;
				}
			}
		}

		Ok(())
	}

	/// Process new blocks for events.
	///
	/// Queries blockchain for logs in the specified block range and
	/// converts discovered events into the solver's event format.
	async fn process_new_blocks(
		provider: &Provider<Http>,
		config: &Eip7683OnchainConfig,
		sink: &EventSink<Event>,
		last_processed_block: &mut u64,
		state: &MonitorState,
	) -> PluginResult<u64> {
		// Get latest block
		let latest_block = provider
			.get_block_number()
			.await
			.map_err(|e| {
				PluginError::ExecutionFailed(format!("Failed to get latest block: {}", e))
			})?
			.as_u64();

		// Update target block
		{
			let mut target = state.target_block.write().await;
			*target = Some(latest_block);
		}

		if latest_block <= *last_processed_block {
			return Ok(0); // No new blocks
		}

		let from_block = *last_processed_block + 1;
		let to_block = std::cmp::min(latest_block, from_block + config.max_blocks_per_request - 1);

		debug!("Processing blocks {} to {}", from_block, to_block);

		// Create filter for this block range
		let filter = Self::create_filter(config, Some(from_block), Some(to_block)).await;

		// Get logs
		let logs = provider
			.get_logs(&filter)
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Failed to get logs: {}", e)))?;

		debug!(
			"Found {} logs in blocks {} to {}",
			logs.len(),
			from_block,
			to_block
		);

		// Process each log
		for log in logs {
			if let Some(event) = Self::parse_log_to_discovery_event(config, &log).await? {
				sink.send_discovery(event).map_err(|e| {
					PluginError::ExecutionFailed(format!("Failed to send event: {}", e))
				})?;

				// Update metrics
				let mut count = state.events_discovered.write().await;
				*count += 1;

				let mut last_timestamp = state.last_event_timestamp.write().await;
				*last_timestamp = Some(
					SystemTime::now()
						.duration_since(UNIX_EPOCH)
						.unwrap()
						.as_secs(),
				);
			}
		}

		// Update current block
		*last_processed_block = to_block;
		{
			let mut current = state.current_block.write().await;
			*current = Some(to_block);
		}

		Ok(to_block - from_block + 1)
	}
}

#[async_trait]
impl BasePlugin for Eip7683OnchainDiscoveryPlugin {
	fn plugin_type(&self) -> &'static str {
		"eip7683_onchain_discovery"
	}

	fn name(&self) -> String {
		format!(
			"EIP-7683 On-chain Discovery Plugin (Chain {})",
			self.config.chain_id
		)
	}

	fn version(&self) -> &'static str {
		"1.0.0"
	}

	fn description(&self) -> &'static str {
		"Discovers EIP-7683 order events from on-chain sources using ethers-rs"
	}

	async fn initialize(&mut self, config: PluginConfig) -> PluginResult<()> {
		debug!("Initializing EIP-7683 on-chain discovery plugin");

		// Parse configuration
		if let Some(chain_id) = config.get_number("chain_id") {
			self.config.chain_id = chain_id as ChainId;
		}

		if let Some(rpc_url) = config.get_string("rpc_url") {
			self.config.rpc_url = rpc_url;
		}

		if let Some(timeout) = config.get_number("timeout_ms") {
			self.config.timeout_ms = timeout as u64;
		}

		if let Some(poll_interval) = config.get_number("poll_interval_ms") {
			self.config.poll_interval_ms = poll_interval as u64;
		}

		if let Some(batch_size) = config.get_number("batch_size") {
			self.config.batch_size = batch_size as u32;
		}

		// Parse contract addresses
		if let Some(input_settlers) = config.get_array("input_settler_addresses") {
			self.config.input_settler_addresses = input_settlers;
		}

		if let Some(output_settlers) = config.get_array("output_settler_addresses") {
			self.config.output_settler_addresses = output_settlers;
		}

		// Parse boolean flags
		if let Some(monitor_open) = config.get_bool("monitor_open") {
			self.config.monitor_open = monitor_open;
		}

		if let Some(monitor_finalised) = config.get_bool("monitor_finalised") {
			self.config.monitor_finalised = monitor_finalised;
		}

		if let Some(monitor_order_purchased) = config.get_bool("monitor_order_purchased") {
			self.config.monitor_order_purchased = monitor_order_purchased;
		}

		// Historical sync settings
		if let Some(enable_historical) = config.get_bool("enable_historical_sync") {
			self.config.enable_historical_sync = enable_historical;
		}

		if let Some(start_block) = config.get_number("historical_start_block") {
			self.config.historical_start_block = Some(start_block as u64);
		}

		// Setup provider
		self.setup_provider().await?;

		self.is_initialized = true;
		debug!("EIP-7683 on-chain discovery plugin initialized successfully");
		Ok(())
	}

	fn validate_config(&self, config: &PluginConfig) -> PluginResult<()> {
		if config.get_string("rpc_url").is_none() {
			return Err(PluginError::InvalidConfiguration(
				"rpc_url is required".to_string(),
			));
		}

		if let Some(chain_id) = config.get_number("chain_id") {
			if chain_id <= 0 {
				return Err(PluginError::InvalidConfiguration(
					"chain_id must be positive".to_string(),
				));
			}
		}

		if let Some(poll_interval) = config.get_number("poll_interval_ms") {
			if poll_interval < 1000 {
				return Err(PluginError::InvalidConfiguration(
					"poll_interval_ms must be at least 1000".to_string(),
				));
			}
		}

		// Validate that at least one contract address is provided
		let has_input_settlers = config
			.get_array("input_settler_addresses")
			.map(|arr| !arr.is_empty())
			.unwrap_or(false);
		let has_output_settlers = config
			.get_array("output_settler_addresses")
			.map(|arr| !arr.is_empty())
			.unwrap_or(false);

		if !has_input_settlers && !has_output_settlers {
			return Err(PluginError::InvalidConfiguration(
				"At least one settler address must be provided".to_string(),
			));
		}

		Ok(())
	}

	async fn health_check(&self) -> PluginResult<PluginHealth> {
		if !self.is_initialized {
			return Ok(PluginHealth::unhealthy("Plugin not initialized"));
		}

		let provider = match &self.provider {
			Some(provider) => provider,
			None => return Ok(PluginHealth::unhealthy("Provider not configured")),
		};

		// Test RPC connection
		match provider.get_block_number().await {
			Ok(block_number) => {
				let current_block = *self.state.current_block.read().await;
				let events_discovered = *self.state.events_discovered.read().await;
				let errors_count = *self.state.errors_count.read().await;

				Ok(
					PluginHealth::healthy("EIP-7683 discovery plugin is operational")
						.with_detail("chain_id", self.config.chain_id.to_string())
						.with_detail("latest_block", block_number.to_string())
						.with_detail(
							"current_block",
							current_block
								.map(|b| b.to_string())
								.unwrap_or("none".to_string()),
						)
						.with_detail("events_discovered", events_discovered.to_string())
						.with_detail("errors_count", errors_count.to_string())
						.with_detail("is_monitoring", self.is_monitoring.to_string()),
				)
			}
			Err(e) => Ok(PluginHealth::unhealthy(format!(
				"RPC connection failed: {}",
				e
			))),
		}
	}

	async fn get_metrics(&self) -> PluginResult<PluginMetrics> {
		let mut metrics = self.metrics.clone();

		metrics.set_gauge("chain_id", self.config.chain_id as f64);
		metrics.set_gauge("is_monitoring", if self.is_monitoring { 1.0 } else { 0.0 });
		metrics.set_gauge(
			"events_discovered",
			*self.state.events_discovered.read().await as f64,
		);
		metrics.set_gauge("errors_count", *self.state.errors_count.read().await as f64);

		if let Some(current_block) = *self.state.current_block.read().await {
			metrics.set_gauge("current_block", current_block as f64);
		}

		if let Some(target_block) = *self.state.target_block.read().await {
			metrics.set_gauge("target_block", target_block as f64);
		}

		Ok(metrics)
	}

	async fn shutdown(&mut self) -> PluginResult<()> {
		info!("Shutting down EIP-7683 on-chain discovery plugin");

		if self.is_monitoring {
			self.stop_monitoring().await?;
		}

		self.is_initialized = false;
		self.provider = None;

		info!("EIP-7683 on-chain discovery plugin shutdown complete");
		Ok(())
	}

	fn config_schema(&self) -> PluginConfigSchema {
		PluginConfigSchema::new()
			.required("chain_id", ConfigFieldType::Number, "EVM chain ID")
			.required("rpc_url", ConfigFieldType::String, "RPC endpoint URL")
			.optional(
				"timeout_ms",
				ConfigFieldType::Number,
				"Request timeout in milliseconds",
				Some(ConfigValue::from(30000i64)),
			)
			.optional(
				"poll_interval_ms",
				ConfigFieldType::Number,
				"Block polling interval in milliseconds",
				Some(ConfigValue::from(12000i64)),
			)
			.optional(
				"batch_size",
				ConfigFieldType::Number,
				"Number of events to process in batch",
				Some(ConfigValue::from(100i64)),
			)
			.optional(
				"input_settler_addresses",
				ConfigFieldType::Array(Box::new(ConfigFieldType::String)),
				"Input settler contract addresses to monitor",
				None,
			)
			.optional(
				"output_settler_addresses",
				ConfigFieldType::Array(Box::new(ConfigFieldType::String)),
				"Output settler contract addresses to monitor",
				None,
			)
			.optional(
				"monitor_open",
				ConfigFieldType::Boolean,
				"Monitor Open events (order creation)",
				Some(ConfigValue::from(true)),
			)
			.optional(
				"monitor_finalised",
				ConfigFieldType::Boolean,
				"Monitor Finalised events (order completion)",
				Some(ConfigValue::from(true)),
			)
			.optional(
				"monitor_order_purchased",
				ConfigFieldType::Boolean,
				"Monitor OrderPurchased events (order updates)",
				Some(ConfigValue::from(true)),
			)
			.optional(
				"enable_historical_sync",
				ConfigFieldType::Boolean,
				"Enable historical event synchronization",
				Some(ConfigValue::from(false)),
			)
			.optional(
				"historical_start_block",
				ConfigFieldType::Number,
				"Starting block for historical sync",
				None,
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
impl DiscoveryPlugin for Eip7683OnchainDiscoveryPlugin {
	async fn start_monitoring(&mut self, sink: EventSink<Event>) -> PluginResult<()> {
		if !self.is_initialized {
			return Err(PluginError::ExecutionFailed(
				"Plugin not initialized".to_string(),
			));
		}

		if self.is_monitoring {
			return Err(PluginError::ExecutionFailed(
				"Already monitoring".to_string(),
			));
		}

		debug!("Starting EIP-7683 on-chain monitoring");

		// Create stop channel
		let (stop_tx, stop_rx) = mpsc::unbounded_channel();
		self.stop_tx = Some(stop_tx);

		// Clone necessary data for the monitoring task
		let provider = self.provider.as_ref().unwrap().clone();
		let config = self.config.clone();
		let current_block = self.state.current_block.clone();
		let target_block = self.state.target_block.clone();
		let events_discovered = self.state.events_discovered.clone();
		let errors_count = self.state.errors_count.clone();
		let last_event_timestamp = self.state.last_event_timestamp.clone();

		// Start monitoring task
		tokio::spawn(async move {
			if let Err(e) = Self::monitor_blocks_task(
				provider,
				config,
				sink,
				stop_rx,
				MonitorState {
					current_block,
					target_block,
					events_discovered,
					errors_count,
					last_event_timestamp,
				},
			)
			.await
			{
				error!("Monitoring task failed: {}", e);
			}
		});

		self.is_monitoring = true;
		Ok(())
	}

	async fn stop_monitoring(&mut self) -> PluginResult<()> {
		if !self.is_monitoring {
			return Ok(());
		}

		info!("Stopping EIP-7683 on-chain monitoring");

		if let Some(stop_tx) = self.stop_tx.take() {
			let _ = stop_tx.send(());
		}

		self.is_monitoring = false;
		info!("EIP-7683 on-chain monitoring stopped");
		Ok(())
	}

	async fn get_status(&self) -> PluginResult<DiscoveryStatus> {
		let current_block = *self.state.current_block.read().await;
		let target_block = *self.state.target_block.read().await;
		let events_discovered = *self.state.events_discovered.read().await;
		let errors_count = *self.state.errors_count.read().await;
		let last_event_timestamp = *self.state.last_event_timestamp.read().await;

		Ok(DiscoveryStatus {
			is_running: self.is_monitoring,
			current_block,
			target_block,
			events_discovered,
			last_event_timestamp,
			errors_count,
			average_processing_time_ms: 0.0, // TODO: Calculate actual average
		})
	}

	async fn discover_range(
		&self,
		from_block: u64,
		to_block: u64,
		sink: EventSink<Event>,
	) -> PluginResult<u64> {
		let provider = self
			.provider
			.as_ref()
			.ok_or_else(|| PluginError::ExecutionFailed("Provider not initialized".to_string()))?;

		let filter = Self::create_filter(&self.config, Some(from_block), Some(to_block)).await;
		let logs = provider
			.get_logs(&filter)
			.await
			.map_err(|e| PluginError::ExecutionFailed(format!("Failed to get logs: {}", e)))?;

		let mut event_count = 0;
		for log in logs {
			if let Some(event) = Self::parse_log_to_discovery_event(&self.config, &log).await? {
				sink.send_discovery(event)?;
				event_count += 1;
			}
		}

		Ok(event_count)
	}

	fn supported_event_types(&self) -> Vec<EventType> {
		let mut types = Vec::new();

		if self.config.monitor_open {
			types.push(EventType::OrderCreated);
		}
		if self.config.monitor_finalised {
			types.push(EventType::OrderFilled);
		}
		if self.config.monitor_order_purchased {
			types.push(EventType::OrderUpdated);
		}

		types
	}

	fn chain_id(&self) -> ChainId {
		self.config.chain_id
	}

	async fn can_monitor_contract(&self, contract_address: &String) -> PluginResult<bool> {
		Ok(self
			.config
			.input_settler_addresses
			.contains(&contract_address.to_string())
			|| self
				.config
				.output_settler_addresses
				.contains(&contract_address.to_string()))
	}

	async fn subscribe_to_events(&mut self, filters: Vec<EventFilter>) -> PluginResult<()> {
		let mut active_filters = self.active_filters.write().await;
		active_filters.extend(filters);
		Ok(())
	}

	async fn unsubscribe_from_events(&mut self, _filters: Vec<EventFilter>) -> PluginResult<()> {
		let mut active_filters = self.active_filters.write().await;
		// Simplified: clear all filters since EventFilter doesn't implement PartialEq
		active_filters.clear();
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn create_test_config() -> PluginConfig {
		PluginConfig::new("discovery")
			.with_config("chain_id", 1i64)
			.with_config("rpc_url", "http://localhost:8545")
			.with_config("poll_interval_ms", 5000i64)
			.with_config("batch_size", 50i64)
			.with_config(
				"input_settler_addresses",
				ConfigValue::Array(vec![ConfigValue::String(
					"0x742d35Cc6634C0532925a3b8D6Ac6c001afb7f9c".to_string(),
				)]),
			)
			.with_config("monitor_open", true)
			.with_config("monitor_finalised", true)
			.with_config("monitor_order_purchased", true)
	}

	#[tokio::test]
	async fn test_plugin_initialization() {
		let mut plugin = Eip7683OnchainDiscoveryPlugin::new();
		let config = create_test_config();

		// Validate config first
		assert!(BasePlugin::validate_config(&plugin, &config).is_ok());

		// Initialize might fail due to network, but should not panic
		match plugin.initialize(config).await {
			Ok(_) => {
				assert!(plugin.is_initialized);
				assert_eq!(plugin.chain_id(), 1);
			}
			Err(PluginError::InitializationFailed(_)) => {
				// Expected in test environment without real RPC
				assert!(!plugin.is_initialized);
			}
			Err(e) => panic!("Unexpected error: {:?}", e),
		}
	}

	#[tokio::test]
	async fn test_event_type_support() {
		let plugin = Eip7683OnchainDiscoveryPlugin::new();
		let supported_types = plugin.supported_event_types();

		assert!(supported_types.contains(&EventType::OrderCreated)); // Open event
		assert!(supported_types.contains(&EventType::OrderFilled)); // Finalised event
		assert!(supported_types.contains(&EventType::OrderUpdated)); // OrderPurchased event
	}

	#[test]
	fn test_contract_address_parsing() {
		let mut plugin = Eip7683OnchainDiscoveryPlugin::new();
		plugin.config.input_settler_addresses =
			vec!["0x742d35Cc6634C0532925a3b8D6Ac6c001afb7f9c".to_string()];
		plugin.config.output_settler_addresses =
			vec!["0x123d35Cc6634C0532925a3b8D6Ac6c001afb7f9c".to_string()];

		let addresses = Eip7683OnchainDiscoveryPlugin::get_contract_addresses(&plugin.config);
		assert_eq!(addresses.len(), 2);
	}

	#[test]
	fn test_event_signature_generation() {
		let plugin = Eip7683OnchainDiscoveryPlugin::new();
		let signatures = Eip7683OnchainDiscoveryPlugin::get_event_signatures(&plugin.config);

		assert_eq!(signatures.len(), 3); // All three event types enabled by default

		// Verify the signatures match expected values
		let expected_open =
			H256::from_slice(&ethers::utils::keccak256(OPEN_EVENT_SIGNATURE.as_bytes()));
		let expected_finalised = H256::from_slice(&ethers::utils::keccak256(
			FINALISED_EVENT_SIGNATURE.as_bytes(),
		));
		let expected_purchased = H256::from_slice(&ethers::utils::keccak256(
			ORDER_PURCHASED_EVENT_SIGNATURE.as_bytes(),
		));

		assert!(signatures.contains(&expected_open));
		assert!(signatures.contains(&expected_finalised));
		assert!(signatures.contains(&expected_purchased));
	}
}
