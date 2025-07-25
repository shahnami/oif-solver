use alloy::{
	primitives::{keccak256, Address as AlloyAddress},
	providers::Provider,
	rpc::types::{Filter, Log},
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use solver_discovery::{DiscoveryError, DiscoveryInterface};
use solver_types::{Intent, IntentMetadata};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// EIP-7683 event signatures
const OPEN_EVENT_SIGNATURE: &str = "Open(bytes32,(address,uint256,uint32,uint32,bytes32,(bytes32,uint256,bytes32,uint256)[],(bytes32,uint256,bytes32,uint256)[],(uint64,bytes32,bytes)[]))";

/// EIP-7683 on-chain discovery implementation
pub struct Eip7683Discovery {
	provider: alloy::providers::RootProvider<alloy::transports::http::Http<reqwest::Client>>,
	settler_addresses: Vec<AlloyAddress>,
	last_block: Arc<Mutex<u64>>,
	is_monitoring: Arc<AtomicBool>,
	stop_signal: Arc<Mutex<Option<mpsc::Sender<()>>>>,
}

/// Parsed EIP-7683 order data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip7683Order {
	pub order_id: [u8; 32],
	pub user: String,
	pub origin_chain_id: u64,
	pub open_deadline: u32,
	pub fill_deadline: u32,
}

impl Eip7683Discovery {
	pub async fn new(
		rpc_url: &str,
		settler_addresses: Vec<String>,
	) -> Result<Self, DiscoveryError> {
		// Create provider
		let provider = alloy::providers::RootProvider::new_http(
			rpc_url
				.parse()
				.map_err(|e| DiscoveryError::Connection(format!("Invalid RPC URL: {}", e)))?,
		);

		// Parse settler addresses
		let addresses: Vec<AlloyAddress> = settler_addresses
			.iter()
			.filter_map(|addr| addr.parse().ok())
			.collect();

		if addresses.is_empty() {
			return Err(DiscoveryError::Connection(
				"No valid settler addresses".to_string(),
			));
		}

		// Get current block
		let current_block = provider.get_block_number().await.map_err(|e| {
			DiscoveryError::Connection(format!("Failed to get block number: {}", e))
		})?;

		Ok(Self {
			provider,
			settler_addresses: addresses,
			last_block: Arc::new(Mutex::new(current_block)),
			is_monitoring: Arc::new(AtomicBool::new(false)),
			stop_signal: Arc::new(Mutex::new(None)),
		})
	}

	async fn parse_open_event(&self, log: &Log) -> Result<Intent, DiscoveryError> {
		// Extract order ID from first topic
		if log.topics().len() < 2 {
			return Err(DiscoveryError::Connection(
				"Missing order ID topic".to_string(),
			));
		}

		let order_id = log.topics()[1].0;

		// Parse order data from log data
		// Simplified parsing - in production would use proper ABI decoding
		let data = &log.data().data;
		if data.len() < 160 {
			return Err(DiscoveryError::Connection("Insufficient data".to_string()));
		}

		// Skip offset (32 bytes) and read user address
		let user_bytes = &data[44..64]; // 32 + 12 = 44, address is last 20 bytes
		let user = AlloyAddress::from_slice(user_bytes);

		// Read chain ID
		let origin_chain_id = u64::from_be_bytes(data[88..96].try_into().unwrap_or([0u8; 8]));

		// Read deadlines
		let open_deadline = u32::from_be_bytes(data[92..96].try_into().unwrap_or([0u8; 4]));

		let fill_deadline = u32::from_be_bytes(data[96..100].try_into().unwrap_or([0u8; 4]));

		let eip_order = Eip7683Order {
			order_id,
			user: user.to_string(),
			origin_chain_id,
			open_deadline,
			fill_deadline,
		};

		// Convert to intent
		Ok(Intent {
			id: hex::encode(order_id),
			source: "eip7683".to_string(),
			standard: "eip7683".to_string(),
			metadata: IntentMetadata {
				requires_auction: false,
				exclusive_until: None,
				discovered_at: std::time::SystemTime::now()
					.duration_since(std::time::UNIX_EPOCH)
					.unwrap()
					.as_secs(),
			},
			data: serde_json::to_value(&eip_order)
				.map_err(|e| DiscoveryError::Connection(format!("Failed to serialize: {}", e)))?,
		})
	}

	async fn monitoring_loop(
		provider: alloy::providers::RootProvider<alloy::transports::http::Http<reqwest::Client>>,
		settler_addresses: Vec<AlloyAddress>,
		last_block: Arc<Mutex<u64>>,
		sender: mpsc::UnboundedSender<Intent>,
		mut stop_rx: mpsc::Receiver<()>,
	) {
		let mut interval = tokio::time::interval(std::time::Duration::from_secs(12));

		loop {
			tokio::select! {
				_ = interval.tick() => {
					let mut last_block_num = last_block.lock().await;

					// Get current block
					let current_block = match provider.get_block_number().await {
						Ok(block) => block,
						Err(e) => {
							eprintln!("Failed to get block number: {}", e);
							continue;
						}
					};

					if current_block <= *last_block_num {
						continue; // No new blocks
					}

					// Create filter for Open events
					let open_sig = keccak256(OPEN_EVENT_SIGNATURE.as_bytes());

					let filter = Filter::new()
						.address(settler_addresses.clone())
						.event_signature(vec![open_sig])
						.from_block(*last_block_num + 1)
						.to_block(current_block);

					// Get logs
					let logs = match provider.get_logs(&filter).await {
						Ok(logs) => logs,
						Err(e) => {
							eprintln!("Failed to get logs: {}", e);
							continue;
						}
					};

					// Parse logs into intents
					for log in logs {
						match Self::parse_open_event(&Eip7683Discovery {
							provider: provider.clone(),
							settler_addresses: settler_addresses.clone(),
							last_block: last_block.clone(),
							is_monitoring: Arc::new(AtomicBool::new(true)),
							stop_signal: Arc::new(Mutex::new(None)),
						}, &log).await {
							Ok(intent) => {
								if let Err(e) = sender.send(intent) {
									eprintln!("Failed to send intent: {}", e);
								}
							}
							Err(e) => {
								eprintln!("Failed to parse event: {}", e);
							}
						}
					}

					// Update last block
					*last_block_num = current_block;
				}
				_ = stop_rx.recv() => {
					break;
				}
			}
		}
	}
}

#[async_trait]
impl DiscoveryInterface for Eip7683Discovery {
	async fn start_monitoring(
		&self,
		sender: mpsc::UnboundedSender<Intent>,
	) -> Result<(), DiscoveryError> {
		if self.is_monitoring.load(Ordering::SeqCst) {
			return Err(DiscoveryError::AlreadyMonitoring);
		}

		let (stop_tx, stop_rx) = mpsc::channel(1);
		*self.stop_signal.lock().await = Some(stop_tx);

		// Spawn monitoring task
		let provider = self.provider.clone();
		let settler_addresses = self.settler_addresses.clone();
		let last_block = self.last_block.clone();

		tokio::spawn(async move {
			Self::monitoring_loop(provider, settler_addresses, last_block, sender, stop_rx).await;
		});

		self.is_monitoring.store(true, Ordering::SeqCst);
		Ok(())
	}

	async fn stop_monitoring(&self) -> Result<(), DiscoveryError> {
		if !self.is_monitoring.load(Ordering::SeqCst) {
			return Ok(());
		}

		if let Some(stop_tx) = self.stop_signal.lock().await.take() {
			let _ = stop_tx.send(()).await;
		}

		self.is_monitoring.store(false, Ordering::SeqCst);
		Ok(())
	}
}

pub fn create_discovery(config: &toml::Value) -> Box<dyn DiscoveryInterface> {
	let rpc_url = config
		.get("rpc_url")
		.and_then(|v| v.as_str())
		.expect("rpc_url is required");

	let settler_addresses = config
		.get("settler_addresses")
		.and_then(|v| v.as_array())
		.map(|arr| {
			arr.iter()
				.filter_map(|v| v.as_str().map(String::from))
				.collect()
		})
		.unwrap_or_default();

	// Create discovery service synchronously
	let discovery = tokio::task::block_in_place(|| {
		tokio::runtime::Handle::current()
			.block_on(async { Eip7683Discovery::new(rpc_url, settler_addresses).await })
	});

	Box::new(discovery.expect("Failed to create discovery service"))
}
