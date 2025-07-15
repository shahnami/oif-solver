//! Oracle-related types and traits.

use crate::{chains::ChainId, common::*, errors::Result, settlement::Fill};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Attestation from an oracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attestation {
	pub fill: Fill,
	pub oracle_signature: Vec<u8>,
	pub attested_at: Timestamp,
	pub oracle_address: Address,
}

/// Oracle trait for attestation services
#[async_trait]
pub trait Oracle: Send + Sync {
	/// Create an attestation for a fill
	async fn attest(&self, fill: &Fill) -> Result<Attestation>;

	/// Verify an attestation
	async fn verify_attestation(&self, attestation: &Attestation) -> Result<bool>;

	/// Check if oracle supports a specific chain pair
	fn supports_route(&self, from: ChainId, to: ChainId) -> bool;
}
