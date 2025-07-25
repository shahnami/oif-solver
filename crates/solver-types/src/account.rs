//! Account-related types for the solver system.
//!
//! This module defines types for blockchain addresses, signatures, and transactions
//! that are used throughout the solver for account management and transaction processing.

use alloy_primitives::{Address as AlloyAddress, Bytes, PrimitiveSignature, U256};
use alloy_rpc_types::TransactionRequest;

/// Blockchain address representation.
///
/// Stores addresses as raw bytes to support different blockchain formats.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Address(pub Vec<u8>);

/// Cryptographic signature representation.
///
/// Stores signatures as raw bytes in the standard Ethereum format (r, s, v).
#[derive(Debug, Clone)]
pub struct Signature(pub Vec<u8>);

impl From<PrimitiveSignature> for Signature {
	fn from(sig: PrimitiveSignature) -> Self {
		// Convert to standard Ethereum signature format (r, s, v)
		let mut bytes = Vec::with_capacity(65);
		bytes.extend_from_slice(&sig.r().to_be_bytes::<32>());
		bytes.extend_from_slice(&sig.s().to_be_bytes::<32>());
		// For EIP-155, v = chain_id * 2 + 35 + y_parity
		// For non-EIP-155, v = 27 + y_parity
		let v = if sig.v() { 28 } else { 27 };
		bytes.push(v);
		Signature(bytes)
	}
}

/// Blockchain transaction representation.
///
/// Contains all fields necessary for constructing and submitting transactions
/// to various blockchain networks.
#[derive(Debug, Clone)]
pub struct Transaction {
	/// Recipient address (None for contract creation).
	pub to: Option<Address>,
	/// Transaction data/calldata.
	pub data: Vec<u8>,
	/// Value to transfer in native currency.
	pub value: U256,
	/// Chain ID for replay protection.
	pub chain_id: u64,
	/// Transaction nonce (optional, can be filled by provider).
	pub nonce: Option<u64>,
	/// Gas limit for transaction execution.
	pub gas_limit: Option<u64>,
	/// Legacy gas price (for non-EIP-1559 transactions).
	pub gas_price: Option<u128>,
	/// Maximum fee per gas (EIP-1559).
	pub max_fee_per_gas: Option<u128>,
	/// Maximum priority fee per gas (EIP-1559).
	pub max_priority_fee_per_gas: Option<u128>,
}

/// Conversion from Alloy's TransactionRequest to our Transaction type.
impl From<TransactionRequest> for Transaction {
	fn from(req: TransactionRequest) -> Self {
		Transaction {
			to: req.to.map(|addr| match addr {
				alloy_primitives::TxKind::Call(a) => Address(a.as_slice().to_vec()),
				alloy_primitives::TxKind::Create => panic!("Create transactions not supported"),
			}),
			data: req.input.input.clone().unwrap_or_default().to_vec(),
			value: req.value.unwrap_or(U256::ZERO),
			chain_id: req.chain_id.unwrap_or(1),
			nonce: req.nonce,
			gas_limit: req.gas,
			gas_price: req.gas_price,
			max_fee_per_gas: req.max_fee_per_gas,
			max_priority_fee_per_gas: req.max_priority_fee_per_gas,
		}
	}
}

/// Conversion from our Transaction type to Alloy's TransactionRequest.
impl From<Transaction> for TransactionRequest {
	fn from(tx: Transaction) -> Self {
		let to = tx.to.map(|to| {
			let mut addr_bytes = [0u8; 20];
			addr_bytes.copy_from_slice(&to.0[..20]);
			alloy_primitives::TxKind::Call(AlloyAddress::from(addr_bytes))
		});

		TransactionRequest {
			chain_id: Some(tx.chain_id),
			value: Some(tx.value),
			to,
			nonce: tx.nonce,
			gas: tx.gas_limit,
			gas_price: tx.gas_price,
			max_fee_per_gas: tx.max_fee_per_gas,
			max_priority_fee_per_gas: tx.max_priority_fee_per_gas,
			input: alloy_rpc_types::TransactionInput {
				input: Some(Bytes::from(tx.data)),
				data: None,
			},
			..Default::default()
		}
	}
}
