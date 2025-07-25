use async_trait::async_trait;
use solver_types::{Address, Signature, Transaction};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AccountError {
	#[error("Signing failed: {0}")]
	SigningFailed(String),
	#[error("Invalid key: {0}")]
	InvalidKey(String),
	#[error("Provider error: {0}")]
	Provider(String),
}

#[async_trait]
pub trait AccountInterface: Send + Sync {
	async fn address(&self) -> Result<Address, AccountError>;
	async fn sign_transaction(&self, tx: &Transaction) -> Result<Signature, AccountError>;
	async fn sign_message(&self, message: &[u8]) -> Result<Signature, AccountError>;
}

pub struct AccountService {
	provider: Box<dyn AccountInterface>,
}

impl AccountService {
	pub fn new(provider: Box<dyn AccountInterface>) -> Self {
		Self { provider }
	}

	pub async fn get_address(&self) -> Result<Address, AccountError> {
		self.provider.address().await
	}

	pub async fn sign(&self, tx: &Transaction) -> Result<Signature, AccountError> {
		self.provider.sign_transaction(tx).await
	}
}
