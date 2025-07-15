//! EIP-7683 specific types and helpers.

use serde::{Deserialize, Serialize};
use solver_types::{
	common::{Address, Bytes32, U256},
	errors::Result,
};

/// Order data subtypes that can be parsed from the arbitrary orderData field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderDataSubtype {
	/// Simple swap order
	SimpleSwap {
		input_token: Address,
		input_amount: U256,
		output_token: Address,
		min_output_amount: U256,
		recipient: Address,
	},
	/// Bridge order
	Bridge {
		token: Address,
		amount: U256,
		destination_chain: U256,
		recipient: Bytes32, // bytes32 for cross-chain compatibility
	},
	/// Generic order data
	Generic(Vec<u8>),
}

/// EIP-712 domain separator computation
pub fn compute_domain_separator(chain_id: U256, settler_address: Address) -> Bytes32 {
	use sha3::{Digest, Keccak256};

	// EIP-712 domain type hash
	let domain_typehash = Keccak256::digest(
		b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
	);

	let mut hasher = Keccak256::new();
	hasher.update(domain_typehash);
	hasher.update(Keccak256::digest(b"OIF Settler"));
	hasher.update(Keccak256::digest(b"1.0.0"));
	let mut chain_id_bytes = [0u8; 32];
	chain_id.to_big_endian(&mut chain_id_bytes);
	hasher.update(chain_id_bytes);
	hasher.update(settler_address.to_fixed_bytes());

	let result = hasher.finalize();
	Bytes32::from_slice(&result)
}

/// Compute order ID using EIP-712 hashing
pub fn compute_order_id(order_struct_hash: Bytes32, domain_separator: Bytes32) -> Bytes32 {
	use sha3::{Digest, Keccak256};

	let mut hasher = Keccak256::new();
	hasher.update(b"\x19\x01");
	hasher.update(domain_separator.as_bytes());
	hasher.update(order_struct_hash.as_bytes());

	let result = hasher.finalize();
	Bytes32::from_slice(&result)
}

/// Parse order data based on type hash
pub fn parse_order_data_subtype(
	_order_data_type: Bytes32,
	order_data: &[u8],
) -> Result<OrderDataSubtype> {
	// TODO: This is a simplified example - real implementation would check actual type hashes
	if order_data.len() < 32 {
		return Ok(OrderDataSubtype::Generic(order_data.to_vec()));
	}

	// Try to decode as SimpleSwap
	if order_data.len() >= 160 {
		// 5 * 32 bytes
		// This would use proper ABI decoding in production
		return Ok(OrderDataSubtype::Generic(order_data.to_vec()));
	}

	Ok(OrderDataSubtype::Generic(order_data.to_vec()))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_compute_domain_separator() {
		let chain_id = U256::from(1);
		let settler = Address::zero();
		let domain1 = compute_domain_separator(chain_id, settler);

		// Should produce deterministic output
		let domain2 = compute_domain_separator(chain_id, settler);
		assert_eq!(domain1, domain2);

		// Different inputs should produce different outputs
		let different_domain = compute_domain_separator(U256::from(2), settler);
		assert_ne!(domain1, different_domain);
	}

	#[test]
	fn test_compute_order_id() {
		let struct_hash = Bytes32::from([1u8; 32]);
		let domain_sep = Bytes32::from([2u8; 32]);
		let order_id = compute_order_id(struct_hash, domain_sep);

		// Should produce deterministic output
		let order_id2 = compute_order_id(struct_hash, domain_sep);
		assert_eq!(order_id, order_id2);

		// Different inputs should produce different outputs
		let different_id = compute_order_id(Bytes32::zero(), domain_sep);
		assert_ne!(order_id, different_id);
	}

	#[test]
	fn test_parse_order_data_subtype() {
		// Test with empty data
		let result = parse_order_data_subtype(Bytes32::zero(), &[]);
		assert!(matches!(result, Ok(OrderDataSubtype::Generic(_))));

		// Test with some data
		let data = vec![1, 2, 3, 4, 5];
		let result = parse_order_data_subtype(Bytes32::zero(), &data);
		if let Ok(OrderDataSubtype::Generic(parsed)) = result {
			assert_eq!(parsed, data);
		} else {
			panic!("Expected Generic subtype");
		}
	}
}
