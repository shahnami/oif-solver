use async_trait::async_trait;
use solver_account::AccountService;
use solver_types::{Signature, Transaction, TransactionHash, TransactionReceipt};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DeliveryError {
	#[error("Network error: {0}")]
	Network(String),
	#[error("Transaction failed: {0}")]
	TransactionFailed(String),
	#[error("No provider available")]
	NoProviderAvailable,
}

#[async_trait]
pub trait DeliveryInterface: Send + Sync {
	async fn submit(
		&self,
		tx: Transaction,
		signature: &Signature,
	) -> Result<TransactionHash, DeliveryError>;
	async fn wait_for_confirmation(
		&self,
		hash: &TransactionHash,
		confirmations: u64,
	) -> Result<TransactionReceipt, DeliveryError>;
	async fn get_receipt(
		&self,
		hash: &TransactionHash,
	) -> Result<TransactionReceipt, DeliveryError>;
}

pub struct DeliveryService {
	providers: Vec<Box<dyn DeliveryInterface>>,
	account: Arc<AccountService>,
	confirmations: u64,
}

impl DeliveryService {
	pub fn new(
		providers: Vec<Box<dyn DeliveryInterface>>,
		account: Arc<AccountService>,
		confirmations: u64,
	) -> Self {
		Self {
			providers,
			account,
			confirmations,
		}
	}

	pub async fn deliver(&self, tx: Transaction) -> Result<TransactionHash, DeliveryError> {
		// Sign transaction
		let signature = self
			.account
			.sign(&tx)
			.await
			.map_err(|e| DeliveryError::Network(e.to_string()))?;

		// Try providers in order
		for provider in &self.providers {
			match provider.submit(tx.clone(), &signature).await {
				Ok(hash) => return Ok(hash),
				Err(e) => log::warn!("Provider failed: {}", e),
			}
		}

		Err(DeliveryError::NoProviderAvailable)
	}

	pub async fn confirm(
		&self,
		hash: &TransactionHash,
		confirmations: u64,
	) -> Result<TransactionReceipt, DeliveryError> {
		// Use first available provider
		self.providers
			.first()
			.ok_or(DeliveryError::NoProviderAvailable)?
			.wait_for_confirmation(hash, confirmations)
			.await
	}

	pub async fn confirm_with_default(
		&self,
		hash: &TransactionHash,
	) -> Result<TransactionReceipt, DeliveryError> {
		// Use configured confirmations
		self.confirm(hash, self.confirmations).await
	}

	pub async fn get_status(&self, hash: &TransactionHash) -> Result<bool, DeliveryError> {
		// Use first available provider
		let receipt = self
			.providers
			.first()
			.ok_or(DeliveryError::NoProviderAvailable)?
			.get_receipt(hash)
			.await?;

		// Return true only if transaction is confirmed and successful
		Ok(receipt.success)
	}
}
