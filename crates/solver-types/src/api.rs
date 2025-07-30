//! API types for the OIF Solver HTTP API.
//!
//! This module defines the request and response types for the OIF Solver API
//! endpoints, following the ERC-7683 Cross-Chain Intents Standard.

use alloy_primitives::U256;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Asset amount representation using ERC-7930 interoperable address format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetAmount {
    /// Asset address in ERC-7930 interoperable format
    pub asset: String,
    /// Amount as a big integer
    #[serde(with = "u256_serde")]
    pub amount: U256,
}

/// Available input with optional priority weighting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableInput {
    /// The input asset and amount
    pub input: AssetAmount,
    /// Optional priority weighting (0-100)
    pub priority: Option<u8>,
}

/// Request for getting price quotes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetQuoteRequest {
    /// Available inputs with optional priority
    #[serde(rename = "availableInputs")]
    pub available_inputs: Vec<AvailableInput>,
    /// Requested minimum outputs
    #[serde(rename = "requestedMinOutputs")]
    pub requested_min_outputs: Vec<AssetAmount>,
    /// Minimum quote validity duration in seconds
    #[serde(rename = "minValidUntil")]
    pub min_valid_until: Option<u64>,
    /// User preference for optimization
    pub preference: Option<QuotePreference>,
}

/// Quote optimization preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QuotePreference {
    Price,
    Speed,
    InputPriority,
}

/// Settlement order data for quotes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementOrder {
    /// Settlement contract address
    pub settler: String,
    /// Settlement-specific data to be signed
    pub data: serde_json::Value,
}

/// A quote option with all necessary execution details.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteOption {
    /// Settlement orders
    pub orders: SettlementOrder,
    /// Required token allowances
    #[serde(rename = "requiredAllowances")]
    pub required_allowances: Vec<AssetAmount>,
    /// Quote validity timestamp
    #[serde(rename = "validUntil")]
    pub valid_until: u64,
    /// Estimated time to completion in seconds
    pub eta: u64,
    /// Total cost in USD
    #[serde(rename = "totalFeeUsd")]
    pub total_fee_usd: f64,
    /// Unique quote identifier
    #[serde(rename = "quoteId")]
    pub quote_id: String,
    /// Settlement mechanism type
    #[serde(rename = "settlementType")]
    pub settlement_type: SettlementType,
}

/// Settlement mechanism types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SettlementType {
    Escrow,
    ResourceLock,
}

/// Response containing quote options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetQuoteResponse {
    /// Available quote options
    pub quotes: Vec<QuoteOption>,
}

/// Cross-chain order for intent submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainOrder {
    /// Settlement contract address
    #[serde(rename = "settlementContract")]
    pub settlement_contract: String,
    /// User's wallet address
    pub swapper: String,
    /// Unique order identifier
    #[serde(with = "u256_serde")]
    pub nonce: U256,
    /// Maximum execution time (Unix timestamp)
    #[serde(rename = "fillDeadline")]
    pub fill_deadline: u64,
    /// Settlement mechanism type
    #[serde(rename = "settlementType")]
    pub settlement_type: SettlementType,
    /// Settlement-specific order data
    #[serde(rename = "orderData")]
    pub order_data: serde_json::Value,
    /// User authorization signature
    pub signature: String,
}

/// Response for intent submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitIntentResponse {
    /// Unique tracking identifier
    #[serde(rename = "intentId")]
    pub intent_id: String,
    /// Acceptance status
    pub status: IntentStatus,
    /// Error details if rejected
    pub message: Option<String>,
}

/// Intent processing status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntentStatus {
    Accepted,
    Rejected,
}

/// Detailed intent status for tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DetailedIntentStatus {
    Pending,
    Registered,
    Filling,
    Filled,
    Claiming,
    Completed,
    Failed,
}

/// Intent status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentStatusResponse {
    /// Intent identifier
    #[serde(rename = "intentId")]
    pub intent_id: String,
    /// Current processing status
    pub status: DetailedIntentStatus,
    /// Status message or error details
    pub message: Option<String>,
    /// Transaction hashes for tracking
    pub transactions: Option<HashMap<String, String>>,
    /// Estimated completion time
    pub eta: Option<u64>,
    /// Last update timestamp
    #[serde(rename = "lastUpdated")]
    pub last_updated: u64,
}

/// API error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error type/code
    pub error: String,
    /// Human-readable description
    pub message: String,
    /// Additional error context
    pub details: Option<serde_json::Value>,
    /// Suggested retry delay in seconds
    #[serde(rename = "retryAfter")]
    pub retry_after: Option<u64>,
}

/// Order data for escrow settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowOrderData {
    #[serde(rename = "settlementType")]
    pub settlement_type: String, // Should be "escrow"
    #[serde(rename = "inputToken")]
    pub input_token: String,
    #[serde(rename = "inputAmount", with = "u256_serde")]
    pub input_amount: U256,
    #[serde(rename = "outputToken")]
    pub output_token: String,
    #[serde(rename = "outputAmount", with = "u256_serde")]
    pub output_amount: U256,
    pub recipient: String,
    #[serde(rename = "additionalData")]
    pub additional_data: Option<String>,
}

/// Order data for ResourceLock settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLockOrderData {
    #[serde(rename = "settlementType")]
    pub settlement_type: String, // Should be "resourceLock"
    #[serde(rename = "lockContract")]
    pub lock_contract: String,
    #[serde(rename = "lockSignature")]
    pub lock_signature: String,
    #[serde(rename = "inputToken")]
    pub input_token: String,
    #[serde(rename = "inputAmount", with = "u256_serde")]
    pub input_amount: U256,
    #[serde(rename = "outputToken")]
    pub output_token: String,
    #[serde(rename = "outputAmount", with = "u256_serde")]
    pub output_amount: U256,
    pub recipient: String,
}

/// Serde module for U256 serialization/deserialization.
pub mod u256_serde {
    use alloy_primitives::U256;
    use serde::{de::Error, Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        value.to_string().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        U256::from_str_radix(&s, 10).map_err(D::Error::custom)
    }
} 