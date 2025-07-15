//! Event signatures for various order standards

use crate::common::Bytes32;
use sha3::{Digest, Keccak256};

/// EIP-7683 event signatures
pub mod eip7683 {
	use super::*;

	/// Get the topic hash for the Open event
	/// Open(bytes32 indexed orderId, ResolvedCrossChainOrder resolvedOrder)
	/// Where ResolvedCrossChainOrder is:
	/// (address user, uint256 originChainId, uint32 openDeadline, uint32 fillDeadline, bytes32 orderId, Output[] maxSpent, Output[] minReceived, FillInstruction[] fillInstructions)
	/// Where Output is: (bytes32 token, uint256 amount, bytes32 recipient, uint256 chainId)
	/// Where FillInstruction is: (uint64 destinationChainId, bytes32 destinationSettler, bytes originData)
	pub fn open_event_topic() -> Bytes32 {
		let mut hasher = Keccak256::new();
		hasher.update(b"Open(bytes32,(address,uint256,uint32,uint32,bytes32,(bytes32,uint256,bytes32,uint256)[],(bytes32,uint256,bytes32,uint256)[],(uint64,bytes32,bytes)[]))");
		let hash = hasher.finalize();
		Bytes32::from_slice(&hash)
	}

	/// Get the topic hash for the Finalised event
	/// Finalised(bytes32 indexed orderId, bytes32 fillerData, bytes32 attestation)
	pub fn finalised_event_topic() -> Bytes32 {
		let mut hasher = Keccak256::new();
		hasher.update(b"Finalised(bytes32,bytes32,bytes32)");
		let hash = hasher.finalize();
		Bytes32::from_slice(&hash)
	}

	/// Get the topic hash for the OrderPurchased event
	/// OrderPurchased(bytes32 indexed orderId, bytes32 fulfillerPubKey, bytes32 orderMetadata)
	pub fn order_purchased_event_topic() -> Bytes32 {
		let mut hasher = Keccak256::new();
		hasher.update(b"OrderPurchased(bytes32,bytes32,bytes32)");
		let hash = hasher.finalize();
		Bytes32::from_slice(&hash)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_eip7683_open_event_topic() {
		let topic = eip7683::open_event_topic();

		// Should be deterministic
		let topic2 = eip7683::open_event_topic();
		assert_eq!(topic, topic2);

		// Should not be zero
		assert_ne!(topic, Bytes32::zero());

		let hex_topic = hex::encode(topic.as_ref());

		// The expected hash from the actual contract
		let expected = "a576d0af275d0c6207ef43ceee8c498a5d7a26b8157a32d3fdf361e64371628c";
		assert_eq!(hex_topic, expected, "Open event signature mismatch");
	}

	#[test]
	fn test_eip7683_finalised_event_topic() {
		let topic = eip7683::finalised_event_topic();

		// Should be deterministic
		let topic2 = eip7683::finalised_event_topic();
		assert_eq!(topic, topic2);

		// Should not be zero
		assert_ne!(topic, Bytes32::zero());

		let hex_topic = hex::encode(topic.as_ref());

		// The expected hash for Finalised(bytes32,bytes32,bytes32)
		let expected = "6d1ab3c99edb0b034244c4a410afdfc12e0fef57313ad9bc936138f2b080025b";
		assert_eq!(hex_topic, expected, "Finalised event signature mismatch");
	}

	#[test]
	fn test_eip7683_order_purchased_event_topic() {
		let topic = eip7683::order_purchased_event_topic();

		// Should be deterministic
		let topic2 = eip7683::order_purchased_event_topic();
		assert_eq!(topic, topic2);

		// Should not be zero
		assert_ne!(topic, Bytes32::zero());

		let hex_topic = hex::encode(topic.as_ref());

		// The expected hash for OrderPurchased(bytes32,bytes32,bytes32)
		let expected = "4cdacd323f9cdecf06f8a27bbdd8d66110c7a2d97ea8430a8c3c67d6eb2f6cc0";
		assert_eq!(
			hex_topic, expected,
			"OrderPurchased event signature mismatch"
		);
	}
}
