//! Settlement encoders for different order standards

pub mod eip7683;

use async_trait::async_trait;
use solver_types::{chains::Transaction, common::Address, errors::Result, orders::Order};

/// Trait for encoding settlement transactions for different order standards
#[async_trait]
pub trait SettlementEncoder: Send + Sync {
	/// Get the name of this encoder
	fn name(&self) -> &str;

	/// Check if this encoder supports the given order
	fn supports(&self, order: &dyn Order) -> bool;

	/// Build a settlement claim transaction for the given order
	async fn encode_claim_transaction(
		&self,
		order: &dyn Order,
		settler_address: Address,
		attestation: &crate::types::Attestation,
	) -> Result<Transaction>;

	/// Get the expected gas limit for this type of settlement
	fn estimated_gas_limit(&self) -> u64 {
		300_000 // Default
	}
}

#[cfg(test)]
mod tests {
	use ethers::abi::{Function, Param, ParamType};
	use ethers::utils::keccak256;

	#[test]
	fn test_finalize_self_selector() {
		#[allow(deprecated)]
		let function = Function {
			name: "finaliseSelf".to_string(),
			inputs: vec![
				Param {
					name: "order".to_string(),
					kind: ParamType::Tuple(vec![
						ParamType::Address,   // user
						ParamType::Uint(256), // nonce
						ParamType::Uint(256), // originChainId
						ParamType::Uint(32),  // expires
						ParamType::Uint(32),  // fillDeadline
						ParamType::Address,   // localOracle
						ParamType::Array(Box::new(
							// inputs: uint256[2][]
							ParamType::FixedArray(Box::new(ParamType::Uint(256)), 2),
						)),
						ParamType::Array(Box::new(
							// outputs: MandateOutput[]
							ParamType::Tuple(vec![
								ParamType::FixedBytes(32), // oracle
								ParamType::FixedBytes(32), // settler
								ParamType::Uint(256),      // chainId
								ParamType::FixedBytes(32), // token
								ParamType::Uint(256),      // amount
								ParamType::FixedBytes(32), // recipient
								ParamType::Bytes,          // call
								ParamType::Bytes,          // context
							]),
						)),
					]),
					internal_type: None,
				},
				Param {
					name: "timestamps".to_string(),
					kind: ParamType::Array(Box::new(ParamType::Uint(32))),
					internal_type: None,
				},
				Param {
					name: "solver".to_string(),
					kind: ParamType::FixedBytes(32),
					internal_type: None,
				},
			],
			outputs: vec![],
			constant: Some(false),
			state_mutability: ethers::abi::StateMutability::NonPayable,
		};

		// Calculate the function selector
		let signature = function.signature();
		let selector = &keccak256(signature.as_bytes())[..4];
		let selector_hex = hex::encode(selector);

		println!("Function signature: {}", signature);
		println!("Function selector: 0x{}", selector_hex);

		assert_eq!(selector_hex, "747e0ec5", "Function selector mismatch!");
	}

	#[test]
	fn test_incorrect_finalize_self_selector() {
		#[allow(deprecated)]
		let function = Function {
			name: "finaliseSelf".to_string(),
			inputs: vec![
				Param {
					name: "order".to_string(),
					kind: ParamType::Tuple(vec![
						ParamType::Address,   // user
						ParamType::Uint(256), // nonce
						ParamType::Uint(256), // originChainId
						ParamType::Uint(32),  // expires
						ParamType::Uint(32),  // fillDeadline
						ParamType::Address,   // localOracle
						ParamType::Array(Box::new(
							// inputs (WRONG: should be uint256[2][])
							ParamType::Tuple(vec![
								ParamType::Uint(256), // token
								ParamType::Uint(256), // amount
							]),
						)),
						ParamType::Array(Box::new(
							// outputs
							ParamType::Tuple(vec![
								ParamType::FixedBytes(32), // oracle
								ParamType::FixedBytes(32), // settler
								ParamType::Uint(256),      // chainId
								ParamType::FixedBytes(32), // token
								ParamType::Uint(256),      // amount
								ParamType::FixedBytes(32), // recipient
								ParamType::Bytes,          // call
								ParamType::Bytes,          // context
							]),
						)),
					]),
					internal_type: None,
				},
				Param {
					name: "timestamps".to_string(),
					kind: ParamType::Array(Box::new(ParamType::Uint(32))),
					internal_type: None,
				},
				Param {
					name: "solver".to_string(),
					kind: ParamType::FixedBytes(32),
					internal_type: None,
				},
			],
			outputs: vec![],
			constant: Some(false),
			state_mutability: ethers::abi::StateMutability::NonPayable,
		};

		// Calculate the function selector
		let signature = function.signature();
		let selector = &keccak256(signature.as_bytes())[..4];
		let selector_hex = hex::encode(selector);

		println!("Incorrect function signature: {}", signature);
		println!("Incorrect function selector: 0x{}", selector_hex);

		assert_eq!(
			selector_hex, "a40d3f35",
			"This is the incorrect selector we're currently generating"
		);
	}

	#[test]
	fn test_standard_order_inputs_type() {
		let correct_inputs_type = ParamType::Array(Box::new(ParamType::FixedArray(
			Box::new(ParamType::Uint(256)),
			2,
		)));

		let incorrect_inputs_type = ParamType::Array(Box::new(ParamType::Tuple(vec![
			ParamType::Uint(256),
			ParamType::Uint(256),
		])));

		println!("Correct inputs type: {:?}", correct_inputs_type);
		println!("Incorrect inputs type: {:?}", incorrect_inputs_type);

		assert_ne!(
			format!("{:?}", correct_inputs_type),
			format!("{:?}", incorrect_inputs_type)
		);
	}
}
