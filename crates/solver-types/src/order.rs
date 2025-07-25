use alloy_primitives::U256;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{Address, TransactionHash};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
	pub id: String,
	pub standard: String,
	pub created_at: u64,
	pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionParams {
	pub gas_price: U256,
	pub priority_fee: Option<U256>,
}

#[derive(Debug, Clone)]
pub struct ExecutionContext {
	pub gas_price: U256,
	pub timestamp: u64,
	pub solver_balance: HashMap<Address, U256>,
}

#[derive(Debug)]
pub enum ExecutionDecision {
	Execute(ExecutionParams),
	Skip(String),
	Defer(std::time::Duration),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillProof {
	pub tx_hash: TransactionHash,
	pub block_number: u64,
	pub attestation_data: Option<Vec<u8>>,
}
