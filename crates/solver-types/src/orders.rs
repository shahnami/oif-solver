//! Order-related types and traits - extensible for multiple standards.

use crate::{chains::ChainId, common::*, errors::Result};
use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::any::Any;

/// Unique order identifier
pub type OrderId = Bytes32;

/// Base trait that ALL orders must implement
#[async_trait]
pub trait Order: Send + Sync + std::fmt::Debug + Any {
	/// Unique identifier for this order
	fn id(&self) -> OrderId;

	/// Order standard/type identifier
	fn standard(&self) -> OrderStandard;

	/// Chain where the order was created
	fn origin_chain(&self) -> ChainId;

	/// Chains where the order should be filled
	fn destination_chains(&self) -> Vec<ChainId>;

	/// Order creation timestamp
	fn created_at(&self) -> Timestamp;

	/// Order expiration timestamp
	fn expires_at(&self) -> Timestamp;

	/// Validate order consistency
	async fn validate(&self) -> Result<()>;

	/// Get semantic classification (for solver decision making)
	fn semantics(&self) -> OrderSemantics {
		// Default implementation can be overridden
		OrderSemantics::Custom(self.standard().to_string())
	}

	/// Convert to standardized fill instructions
	async fn to_fill_instructions(&self) -> Result<Vec<FillInstruction>>;

	/// Downcast to specific order type
	fn as_any(&self) -> &dyn Any;

	/// Get the user who created this order
	fn user(&self) -> Address;

	/// Get input tokens/amounts (what user deposits on origin chain)
	fn inputs(&self) -> Result<Vec<Input>>;

	/// Get expected outputs (what recipient receives on destination chain)
	fn outputs(&self) -> Result<Vec<Output>>;
}

/// Supported order standards
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStandard {
	/// EIP-7683 Cross Chain Intents
	EIP7683,
	/// Custom order type
	Custom(String),
}

impl std::fmt::Display for OrderStandard {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::EIP7683 => write!(f, "EIP-7683"),
			Self::Custom(s) => write!(f, "Custom({})", s),
		}
	}
}

/// Generic fill instruction that works across standards
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillInstruction {
	/// The chain to fill on
	pub destination_chain: ChainId,
	/// The contract to interact with
	pub destination_contract: Address,
	/// Standard-specific fill data
	pub fill_data: FillData,
}

/// Fill data variants for different standards
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FillData {
	/// EIP-7683 specific fill data
	EIP7683 {
		order_id: Bytes32,
		origin_data: Vec<u8>,
	},
	/// Generic fill data
	Generic(Vec<u8>),
}

/// Input specification (what user deposits)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Input {
	pub token: Address,
	pub amount: U256,
}

/// Output specification (what recipient receives)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Output {
	pub token: Address,
	pub amount: U256,
	pub recipient: Address,
	pub chain_id: ChainId,
}

// Keep semantic analysis types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OrderSemantics {
	Swap {
		price_sensitivity: PriceSensitivity,
	},
	Bridge {
		slippage_tolerance: rust_decimal::Decimal,
	},
	CrossChainSwap {
		price_sensitivity: PriceSensitivity,
		bridge_params: BridgeParams,
	},
	Custom(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PriceSensitivity {
	pub is_limit_order: bool,
	pub price_threshold: Option<Decimal>,
	pub reference_price: Option<Decimal>,
	pub max_slippage: rust_decimal::Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeParams {
	pub max_bridge_fee: U256,
	pub slippage_tolerance: rust_decimal::Decimal,
}

/// Status of an order in its lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStatus {
	/// Order has been discovered
	Discovered,
	/// Order is being validated
	Validating,
	/// Order validation failed
	Invalid,
	/// Order is ready to be filled
	Ready,
	/// Order fill is in progress
	Filling,
	/// Order has been filled
	Filled,
	/// Order settlement is in progress
	Settling,
	/// Order has been settled
	Settled,
	/// Order was abandoned
	Abandoned,
}
