//! Account provider implementations for the solver service.
//!
//! This module provides concrete implementations of the AccountInterface trait,
//! currently supporting local private key wallets using the Alloy library.

use crate::{AccountError, AccountInterface};
use alloy_consensus::TxLegacy;
use alloy_network::TxSigner;
use alloy_primitives::{Address as AlloyAddress, Bytes, TxKind};
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use async_trait::async_trait;
use solver_types::{Address, ConfigSchema, Field, FieldType, Schema, Signature, Transaction};

/// Local wallet implementation using Alloy's signer.
///
/// This implementation manages a private key locally and uses it to sign
/// transactions and messages. It's suitable for development and testing
/// environments where key management simplicity is preferred.
pub struct LocalWallet {
	/// The underlying Alloy signer that handles cryptographic operations.
	signer: PrivateKeySigner,
}

impl LocalWallet {
	/// Creates a new LocalWallet from a hex-encoded private key.
	///
	/// The private key should be provided as a hex string (with or without 0x prefix).
	pub fn new(private_key_hex: &str) -> Result<Self, AccountError> {
		// Parse the private key using Alloy's signer
		let signer = private_key_hex
			.parse::<PrivateKeySigner>()
			.map_err(|e| AccountError::InvalidKey(format!("Invalid private key: {}", e)))?;

		Ok(Self { signer })
	}
}

/// Configuration schema for LocalWallet.
pub struct LocalWalletSchema;

impl ConfigSchema for LocalWalletSchema {
	fn validate(&self, config: &toml::Value) -> Result<(), solver_types::ValidationError> {
		let schema = Schema::new(
			// Required fields
			vec![
				Field::new("private_key", FieldType::String).with_validator(|value| {
					let key = value.as_str().unwrap();
					let key_without_prefix = key.strip_prefix("0x").unwrap_or(key);

					if key_without_prefix.len() != 64 {
						return Err("Private key must be 64 hex characters (32 bytes)".to_string());
					}

					if hex::decode(key_without_prefix).is_err() {
						return Err("Private key must be valid hexadecimal".to_string());
					}

					Ok(())
				}),
			],
			// Optional fields
			vec![],
		);

		schema.validate(config)
	}
}

#[async_trait]
impl AccountInterface for LocalWallet {
	fn config_schema(&self) -> Box<dyn ConfigSchema> {
		Box::new(LocalWalletSchema)
	}

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

/// Factory function to create an account provider from configuration.
///
/// This function reads the account configuration and creates the appropriate
/// AccountInterface implementation. Currently only supports local wallets
/// with a private_key configuration parameter.
pub fn create_account(config: &toml::Value) -> Box<dyn AccountInterface> {
	let private_key = config
		.get("private_key")
		.and_then(|v| v.as_str())
		.expect("private_key is required for local wallet");

	Box::new(LocalWallet::new(private_key).expect("Failed to create wallet"))
}
