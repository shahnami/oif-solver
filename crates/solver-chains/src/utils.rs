//! Utility functions and types for chain adapters.
//!
//! This module provides reusable components for chain adapter implementations,
//! including retry logic for network requests and configuration structures.

use backoff::{backoff::Backoff, ExponentialBackoff};
use ethers::providers::{Http, JsonRpcClient, ProviderError};
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use tracing::warn;

/// Retry client wrapper for automatic retries.
///
/// Wraps any JSON-RPC client to add automatic retry functionality with exponential
/// backoff. This helps handle transient network failures and temporary node
/// unavailability. The retry logic will attempt requests multiple times with
/// increasing delays between attempts.
///
/// The maximum retry duration is configured to 30 seconds by default.
#[derive(Debug, Clone)]
pub struct RetryClient<T> {
	inner: T,
	backoff: ExponentialBackoff,
	max_retries: u32,
}

impl<T> RetryClient<T> {
	/// Creates a new retry client wrapping the provided inner client.
	///
	/// Configures exponential backoff with a maximum elapsed time of 30 seconds.
	/// After this duration, retries will stop and the error will be returned.
	pub fn new(inner: T) -> Self {
		let backoff = ExponentialBackoff {
			max_elapsed_time: Some(Duration::from_secs(30)),
			..Default::default()
		};

		Self {
			inner,
			backoff,
			max_retries: 3, // Default to 3 retries
		}
	}

	/// Sets the maximum number of retry attempts.
	pub fn with_max_retries(mut self, max_retries: u32) -> Self {
		self.max_retries = max_retries;
		self
	}
}

#[async_trait::async_trait]
impl JsonRpcClient for RetryClient<Http> {
	type Error = ProviderError;

	async fn request<T, R>(&self, method: &str, params: T) -> Result<R, Self::Error>
	where
		T: Serialize + Send + Sync + std::fmt::Debug,
		R: DeserializeOwned + Send,
	{
		let mut backoff = self.backoff.clone();
		let mut attempts = 0;

		loop {
			match self.inner.request(method, &params).await {
				Ok(result) => return Ok(result),
				Err(e) => {
					attempts += 1;

					// Check if we've exceeded the maximum number of retries
					if attempts > self.max_retries {
						warn!(
							"RPC request failed after {} attempts, giving up: {}",
							self.max_retries, e
						);
						return Err(e.into());
					}

					// Check if we should retry based on backoff
					if let Some(delay) = backoff.next_backoff() {
						warn!(
							"RPC request failed, attempt {}/{}, retrying in {:?}: {}",
							attempts, self.max_retries, delay, e
						);
						tokio::time::sleep(delay).await;
					} else {
						warn!(
							"RPC request failed, backoff exhausted after {} attempts: {}",
							attempts, e
						);
						return Err(e.into());
					}
				}
			}
		}
	}
}

/// Chain configuration parameters.
///
/// Contains all necessary configuration for connecting to and interacting with
/// a blockchain network. This structure can be deserialized from configuration
/// files to set up chain adapters.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChainConfig {
	/// Unique identifier for the blockchain network.
	pub chain_id: u64,
	/// Human-readable name of the chain.
	pub name: String,
	/// HTTP/HTTPS JSON-RPC endpoint URL.
	pub rpc_endpoint: String,
	/// Optional WebSocket endpoint URL for real-time updates.
	pub ws_endpoint: Option<String>,
	/// Average block time in seconds.
	pub block_time: u64,
	/// Number of block confirmations required for finality.
	pub confirmations: u64,
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_chain_config_construction() {
		let config = ChainConfig {
			chain_id: 1,
			name: "Ethereum".to_string(),
			rpc_endpoint: "https://eth.example.com".to_string(),
			ws_endpoint: Some("wss://eth.example.com".to_string()),
			block_time: 12,
			confirmations: 12,
		};

		assert_eq!(config.chain_id, 1);
		assert_eq!(config.name, "Ethereum");
		assert_eq!(config.rpc_endpoint, "https://eth.example.com");
		assert_eq!(
			config.ws_endpoint,
			Some("wss://eth.example.com".to_string())
		);
		assert_eq!(config.block_time, 12);
		assert_eq!(config.confirmations, 12);
	}

	#[test]
	fn test_retry_client_creation() {
		// Test just the retry client configuration without actual HTTP
		let backoff = ExponentialBackoff {
			max_elapsed_time: Some(Duration::from_secs(30)),
			..Default::default()
		};
		let _max_elapsed = backoff.max_elapsed_time;

		// Create a mock inner type (we just test the backoff config)
		struct MockInner;
		let client = RetryClient::new(MockInner);

		// Check that backoff is configured
		assert!(client.backoff.max_elapsed_time.is_some());
		assert_eq!(
			client.backoff.max_elapsed_time,
			Some(Duration::from_secs(30))
		);
		assert_eq!(client.max_retries, 3);
	}
}
