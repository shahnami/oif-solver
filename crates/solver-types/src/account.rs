use alloy_primitives::{Address as AlloyAddress, Bytes, PrimitiveSignature, U256};
use alloy_rpc_types::TransactionRequest;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Address(pub Vec<u8>);

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

#[derive(Debug, Clone)]
pub struct Transaction {
	pub to: Option<Address>,
	pub data: Vec<u8>,
	pub value: U256,
	pub chain_id: u64,
	pub nonce: Option<u64>,
	pub gas_limit: Option<u64>,
	pub gas_price: Option<u128>,
	pub max_fee_per_gas: Option<u128>,
	pub max_priority_fee_per_gas: Option<u128>,
}

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
