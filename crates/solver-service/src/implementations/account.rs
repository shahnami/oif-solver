use alloy::{
	consensus::TxLegacy,
	network::TxSigner,
	primitives::{Address as AlloyAddress, Bytes, TxKind},
	signers::{local::PrivateKeySigner, Signer},
};
use async_trait::async_trait;
use solver_account::{AccountError, AccountInterface};
use solver_types::{Address, Signature, Transaction};

/// Local wallet implementation using Alloy's signer
pub struct LocalWallet {
	signer: PrivateKeySigner,
}

impl LocalWallet {
	pub fn new(private_key_hex: &str) -> Result<Self, AccountError> {
		// Parse the private key using Alloy's signer
		let signer = private_key_hex
			.parse::<PrivateKeySigner>()
			.map_err(|e| AccountError::InvalidKey(format!("Invalid private key: {}", e)))?;

		Ok(Self { signer })
	}
}

#[async_trait]
impl AccountInterface for LocalWallet {
	async fn address(&self) -> Result<Address, AccountError> {
		let alloy_address = self.signer.address();
		Ok(Address(alloy_address.as_slice().to_vec()))
	}

	async fn sign_transaction(&self, tx: &Transaction) -> Result<Signature, AccountError> {
		let to = if let Some(to_addr) = &tx.to {
			if to_addr.0.len() != 20 {
				return Err(AccountError::SigningFailed(
					"Invalid address length".to_string(),
				));
			}
			let mut addr_bytes = [0u8; 20];
			addr_bytes.copy_from_slice(&to_addr.0);
			TxKind::Call(AlloyAddress::from(addr_bytes))
		} else {
			TxKind::Create
		};

		let value = tx.value;

		let mut legacy_tx = TxLegacy {
			chain_id: Some(tx.chain_id),
			nonce: tx.nonce.unwrap_or(0),
			gas_price: tx.gas_price.unwrap_or(0),
			gas_limit: tx.gas_limit.unwrap_or(0),
			to,
			value,
			input: Bytes::from(tx.data.clone()),
		};

		let signature = self
			.signer
			.sign_transaction(&mut legacy_tx)
			.await
			.map_err(|e| {
				AccountError::SigningFailed(format!("Failed to sign transaction: {}", e))
			})?;

		Ok(signature.into())
	}

	async fn sign_message(&self, message: &[u8]) -> Result<Signature, AccountError> {
		// Use Alloy's signer to sign the message (handles EIP-191 internally)
		let signature =
			self.signer.sign_message(message).await.map_err(|e| {
				AccountError::SigningFailed(format!("Failed to sign message: {}", e))
			})?;

		Ok(signature.into())
	}
}

pub fn create_account(config: &toml::Value) -> Box<dyn AccountInterface> {
	let private_key = config
		.get("private_key")
		.and_then(|v| v.as_str())
		.expect("private_key is required for local wallet");

	Box::new(LocalWallet::new(private_key).expect("Failed to create wallet"))
}
