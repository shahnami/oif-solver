//! Common types used throughout the solver system.

use serde::{Deserialize, Serialize};
use std::fmt;

// Re-export commonly used ethereum types
pub use ethers_core::types::{Address, Bytes as EthBytes, H256 as Bytes32, U256};

/// Transaction hash
pub type TxHash = Bytes32;

/// Block number
pub type BlockNumber = u64;

/// Timestamp (Unix seconds)
pub type Timestamp = u64;

/// Unique identifier for various entities
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Id<T> {
	value: uuid::Uuid,
	_phantom: std::marker::PhantomData<T>,
}

impl<T> Default for Id<T> {
	fn default() -> Self {
		Self::new()
	}
}

impl<T> Id<T> {
	pub fn new() -> Self {
		Self {
			value: uuid::Uuid::new_v4(),
			_phantom: std::marker::PhantomData,
		}
	}

	pub fn from_bytes(bytes: [u8; 16]) -> Self {
		Self {
			value: uuid::Uuid::from_bytes(bytes),
			_phantom: std::marker::PhantomData,
		}
	}
}

impl<T> fmt::Display for Id<T> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}", self.value)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_id_generation() {
		#[derive(Debug, PartialEq)]
		struct TestType;
		let id1 = Id::<TestType>::new();
		let id2 = Id::<TestType>::new();

		// IDs should be unique
		assert_ne!(id1, id2);

		// ID from bytes should be deterministic
		let bytes = [1u8; 16];
		let id3 = Id::<TestType>::from_bytes(bytes);
		let id4 = Id::<TestType>::from_bytes(bytes);
		assert_eq!(id3, id4);
	}
}
