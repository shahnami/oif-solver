use alloy_primitives::U256;
use async_trait::async_trait;
use solver_types::{
	ExecutionContext, ExecutionDecision, ExecutionParams, FillProof, Intent, Order, Transaction,
};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OrderError {
	#[error("Validation failed: {0}")]
	ValidationFailed(String),
	#[error("Insufficient balance")]
	InsufficientBalance,
	#[error("Cannot satisfy order")]
	CannotSatisfyOrder,
}

// Standard-specific implementation
#[async_trait]
pub trait OrderInterface: Send + Sync {
	async fn validate_intent(&self, intent: &Intent) -> Result<Order, OrderError>;
	async fn generate_fill_transaction(
		&self,
		order: &Order,
		params: &ExecutionParams,
	) -> Result<Transaction, OrderError>;
	async fn generate_claim_transaction(
		&self,
		order: &Order,
		fill_proof: &FillProof,
	) -> Result<Transaction, OrderError>;
}

// Solver's execution strategy
#[async_trait]
pub trait ExecutionStrategy: Send + Sync {
	async fn should_execute(&self, order: &Order, context: &ExecutionContext) -> ExecutionDecision;
}

pub struct OrderService {
	implementations: HashMap<String, Box<dyn OrderInterface>>,
	strategy: Box<dyn ExecutionStrategy>,
}

impl OrderService {
	pub fn new(
		implementations: HashMap<String, Box<dyn OrderInterface>>,
		strategy: Box<dyn ExecutionStrategy>,
	) -> Self {
		Self {
			implementations,
			strategy,
		}
	}

	pub async fn validate_intent(&self, intent: &Intent) -> Result<Order, OrderError> {
		let implementation = self
			.implementations
			.get(&intent.standard)
			.ok_or_else(|| OrderError::ValidationFailed("Unknown standard".into()))?;

		implementation.validate_intent(intent).await
	}

	pub async fn should_execute(
		&self,
		order: &Order,
		context: &ExecutionContext,
	) -> ExecutionDecision {
		self.strategy.should_execute(order, context).await
	}

	pub async fn generate_fill_transaction(
		&self,
		order: &Order,
		params: &ExecutionParams,
	) -> Result<Transaction, OrderError> {
		let implementation = self
			.implementations
			.get(&order.standard)
			.ok_or_else(|| OrderError::ValidationFailed("Unknown standard".into()))?;

		implementation
			.generate_fill_transaction(order, params)
			.await
	}

	pub async fn generate_claim_transaction(
		&self,
		order: &Order,
		proof: &FillProof,
	) -> Result<Transaction, OrderError> {
		let implementation = self
			.implementations
			.get(&order.standard)
			.ok_or_else(|| OrderError::ValidationFailed("Unknown standard".into()))?;

		implementation
			.generate_claim_transaction(order, proof)
			.await
	}
}

// Example strategies
pub struct AlwaysExecuteStrategy;

#[async_trait]
impl ExecutionStrategy for AlwaysExecuteStrategy {
	async fn should_execute(
		&self,
		_order: &Order,
		context: &ExecutionContext,
	) -> ExecutionDecision {
		ExecutionDecision::Execute(ExecutionParams {
			gas_price: context.gas_price,
			priority_fee: None,
		})
	}
}

pub struct LimitOrderStrategy {
	pub min_profit_bps: u32,
	pub max_gas_price: U256,
}

#[async_trait]
impl ExecutionStrategy for LimitOrderStrategy {
	async fn should_execute(
		&self,
		_order: &Order,
		context: &ExecutionContext,
	) -> ExecutionDecision {
		// Check gas price limit
		if context.gas_price > self.max_gas_price {
			return ExecutionDecision::Defer(std::time::Duration::from_secs(60));
		}

		// In reality, would calculate actual profit
		// For now, just execute
		ExecutionDecision::Execute(ExecutionParams {
			gas_price: context.gas_price,
			priority_fee: None,
		})
	}
}
