//! Delivery configuration types.
//!
//! This module contains configuration structures for transaction delivery services,
//! including endpoint configuration, authentication, and gas pricing strategies.

use serde::{Deserialize, Serialize};
use solver_types::{chains::ChainId, common::Address};
use std::collections::HashMap;

/// Delivery service configuration.
///
/// Contains all necessary parameters for configuring a transaction delivery service.
/// This structure is designed to be deserialized from configuration files and
/// supports multi-chain setups with different endpoints per chain.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DeliveryConfig {
	/// API endpoints mapped by chain ID.
	///
	/// Each chain can have its own RPC endpoint, allowing for optimal
	/// node selection per network.
	pub endpoints: HashMap<ChainId, String>,
	/// API key for authenticating with RPC endpoints.
	///
	/// Used as Bearer token in Authorization headers. Keep this value
	/// secure and never commit to version control.
	pub api_key: String,
	/// Strategy for calculating transaction gas prices.
	///
	/// Determines how gas prices are set relative to network conditions.
	pub gas_strategy: GasStrategy,
	/// Maximum number of retry attempts for failed transactions.
	///
	/// Only transient failures are retried. Reverted transactions
	/// are not retried.
	pub max_retries: u32,
	/// Number of block confirmations required before considering
	/// a transaction final.
	///
	/// Higher values provide more security against chain reorganizations.
	pub confirmations: u64,
	/// Address that will send transactions.
	///
	/// This address must have sufficient balance for gas costs
	/// and must be accessible to the signing mechanism.
	pub from_address: Address,
}

/// Gas pricing strategy for transaction submission.
///
/// Different strategies allow for various trade-offs between
/// transaction cost and inclusion speed. The appropriate strategy
/// depends on network conditions and urgency requirements.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum GasStrategy {
	/// Use the network's standard gas price recommendation.
	///
	/// Suitable for non-urgent transactions where cost optimization
	/// is more important than fast inclusion.
	Standard,
	/// Use a higher gas price for faster inclusion.
	///
	/// Typically applies a 1.2x multiplier to the standard price.
	/// Suitable for time-sensitive transactions.
	Fast,
	/// Apply a custom multiplier to the base gas price.
	///
	/// Allows fine-tuning gas prices based on specific requirements.
	/// Multiplier of 1.0 equals standard price, higher values increase
	/// priority and cost.
	Custom { multiplier: f64 },
	/// Use EIP-1559 dynamic fee mechanism.
	///
	/// Available on compatible chains (Ethereum post-London, Polygon, etc.).
	/// The max_priority_fee is added to the base fee for miner incentive.
	/// Provides more predictable gas costs and better UX.
	Eip1559 { max_priority_fee: u64 },
}
