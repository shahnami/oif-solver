//! Error types for the solver system.

use thiserror::Error;

pub type Result<T> = std::result::Result<T, SolverError>;

#[derive(Error, Debug)]
pub enum SolverError {
	#[error("Chain error: {0}")]
	Chain(String),

	#[error("Order error: {0}")]
	Order(String),

	#[error("Settlement error: {0}")]
	Settlement(String),

	#[error("Validation error: {0}")]
	Validation(String),

	#[error("Delivery error: {0}")]
	Delivery(String),

	#[error("Configuration error: {0}")]
	Config(String),

	#[error("Network error: {0}")]
	Network(String),

	#[error("Insufficient liquidity: need {needed}, have {available}")]
	InsufficientLiquidity {
		needed: crate::common::U256,
		available: crate::common::U256,
	},

	#[error("Order expired at {expired_at}")]
	OrderExpired {
		expired_at: crate::common::Timestamp,
	},

	#[error("Not implemented: {0}")]
	NotImplemented(String),

	#[error("State error: {0}")]
	State(String),

	#[error(transparent)]
	Other(#[from] anyhow::Error),
}
