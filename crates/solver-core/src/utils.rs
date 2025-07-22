//! # Core Utilities
//!
//! Utility functions used throughout the solver-core crate.
//!
//! This module provides common helper functions for formatting,
//! string manipulation, and other utilities that are used across
//! multiple modules within the core system.

/// Truncate a hash or long identifier for display purposes.
///
/// Creates a human-readable shortened version of long identifiers such as
/// transaction hashes, order IDs, or other lengthy strings by showing
/// the first 6 and last 4 characters with ellipsis in between.
///
/// # Arguments
/// * `hash` - The string to truncate
///
/// # Returns
/// A truncated string in the format "prefix...suffix" for long strings,
/// or the original string if it's 12 characters or shorter
///
/// # Examples
/// - `"0xa096c418fd1192ba7f5b506beea682a633f9ab82911fa3d7a249b8d80889a0b4"` becomes `"0xa096...a0b4"`
/// - `"0x12345"` remains `"0x12345"`
pub fn truncate_hash(hash: &str) -> String {
	if hash.len() <= 12 {
		hash.to_string()
	} else {
		format!("{}...{}", &hash[..6], &hash[hash.len() - 4..])
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_truncate_hash() {
		// Long hash
		let hash = "0xa096c418fd1192ba7f5b506beea682a633f9ab82911fa3d7a249b8d80889a0b4";
		assert_eq!(truncate_hash(hash), "0xa096...a0b4");

		// Short hash (shouldn't truncate)
		let short = "0x12345";
		assert_eq!(truncate_hash(short), "0x12345");

		// Exactly 12 chars
		let exact = "0x1234567890";
		assert_eq!(truncate_hash(exact), "0x1234567890");
	}
}
