use alloy::{
	primitives::FixedBytes,
	providers::{Provider, ProviderBuilder},
	rpc::types::TransactionRequest,
};
use async_trait::async_trait;
use solver_delivery::{DeliveryError, DeliveryInterface};
use solver_types::{
	Signature, Transaction as SolverTransaction, TransactionHash, TransactionReceipt,
};
use std::sync::Arc;

/// Alloy-based EVM delivery implementation
pub struct AlloyDelivery {
	provider: Arc<dyn Provider<alloy::transports::http::Http<reqwest::Client>> + Send + Sync>,
}

impl AlloyDelivery {
	pub async fn new(
		rpc_url: &str,
		_chain_id: u64,
		signer: alloy::signers::local::PrivateKeySigner,
	) -> Result<Self, DeliveryError> {
		// Create provider with wallet for automatic signing
		let url = rpc_url
			.parse()
			.map_err(|e| DeliveryError::Network(format!("Invalid RPC URL: {}", e)))?;

		let wallet = alloy::network::EthereumWallet::from(signer);

		let provider = ProviderBuilder::new()
			.with_cached_nonce_management()
			.wallet(wallet)
			.on_http(url);

		Ok(Self {
			provider: Arc::new(provider),
		})
	}
}

#[async_trait]
impl DeliveryInterface for AlloyDelivery {
	async fn submit(
		&self,
		tx: SolverTransaction,
		_signature: &Signature,
	) -> Result<TransactionHash, DeliveryError> {
		// Convert solver transaction to alloy transaction request
		let request: TransactionRequest = tx.into();

		// Send transaction - the provider's wallet will handle signing
		let pending_tx =
			self.provider.send_transaction(request).await.map_err(|e| {
				DeliveryError::Network(format!("Failed to send transaction: {}", e))
			})?;

		// Get the transaction hash
		let tx_hash = *pending_tx.tx_hash();

		Ok(TransactionHash(tx_hash.0.to_vec()))
	}

	async fn wait_for_confirmation(
		&self,
		hash: &TransactionHash,
		confirmations: u64,
	) -> Result<TransactionReceipt, DeliveryError> {
		let tx_hash = FixedBytes::<32>::from_slice(&hash.0);

		// Get transaction receipt
		let receipt = self
			.provider
			.get_transaction_receipt(tx_hash)
			.await
			.map_err(|e| DeliveryError::Network(format!("Failed to get receipt: {}", e)))?
			.ok_or_else(|| DeliveryError::Network("Transaction not found".to_string()))?;

		let current_block =
			self.provider.get_block_number().await.map_err(|e| {
				DeliveryError::Network(format!("Failed to get block number: {}", e))
			})?;

		let tx_block = receipt.block_number.unwrap_or(0);
		let current_confirmations = current_block.saturating_sub(tx_block);

		if current_confirmations < confirmations {
			// In production, would poll until enough confirmations
			return Err(DeliveryError::Network(format!(
				"Insufficient confirmations: {} < {}",
				current_confirmations, confirmations
			)));
		}

		Ok(TransactionReceipt {
			hash: TransactionHash(receipt.transaction_hash.0.to_vec()),
			block_number: tx_block,
			success: receipt.status(),
		})
	}

	async fn get_receipt(
		&self,
		hash: &TransactionHash,
	) -> Result<TransactionReceipt, DeliveryError> {
		let tx_hash = FixedBytes::<32>::from_slice(&hash.0);

		let receipt = self
			.provider
			.get_transaction_receipt(tx_hash)
			.await
			.map_err(|e| DeliveryError::Network(format!("Failed to get receipt: {}", e)))?
			.ok_or_else(|| DeliveryError::Network("Transaction not found".to_string()))?;

		Ok(TransactionReceipt {
			hash: TransactionHash(receipt.transaction_hash.0.to_vec()),
			block_number: receipt.block_number.unwrap_or(0),
			success: receipt.status(),
		})
	}
}

pub fn create_http_delivery(config: &toml::Value) -> Box<dyn DeliveryInterface> {
	let rpc_url = config
		.get("rpc_url")
		.and_then(|v| v.as_str())
		.expect("rpc_url is required");

	let chain_id = config
		.get("chain_id")
		.and_then(|v| v.as_integer())
		.expect("chain_id is required") as u64;

	let private_key = config
		.get("private_key")
		.and_then(|v| v.as_str())
		.expect("private_key is required");

	// Parse the private key
	let signer: alloy::signers::local::PrivateKeySigner =
		private_key.parse().expect("Invalid private key");

	// Create delivery service synchronously, but the actual connection happens async
	let delivery = tokio::task::block_in_place(|| {
		tokio::runtime::Handle::current()
			.block_on(async { AlloyDelivery::new(rpc_url, chain_id, signer).await })
	});

	Box::new(delivery.expect("Failed to create delivery service"))
}
