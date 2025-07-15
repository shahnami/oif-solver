//! EIP-7683 order management and implementation for the OIF solver.

pub mod classification;
pub mod factory;
pub mod implementations;
pub mod registry;

pub use classification::{OrderClassification, OrderClassifier};
pub use factory::OrderFactory;
pub use registry::OrderRegistry;

use async_trait::async_trait;
use solver_types::{
	chains::ChainId,
	common::{Address, Timestamp},
	errors::Result,
	orders::{FillInstruction, Input, Order, OrderId, OrderStandard, Output},
};
use std::any::Any;

/// Enum wrapper for different order implementations
#[derive(Debug, Clone)]
pub enum OrderImpl {
	GaslessOrder(implementations::eip7683::GaslessOrder),
	OnchainOrder(implementations::eip7683::OnchainOrder),
}

#[async_trait]
impl Order for OrderImpl {
	fn id(&self) -> OrderId {
		match self {
			OrderImpl::GaslessOrder(order) => order.id(),
			OrderImpl::OnchainOrder(order) => order.id(),
		}
	}

	fn standard(&self) -> OrderStandard {
		match self {
			OrderImpl::GaslessOrder(order) => order.standard(),
			OrderImpl::OnchainOrder(order) => order.standard(),
		}
	}

	fn origin_chain(&self) -> ChainId {
		match self {
			OrderImpl::GaslessOrder(order) => order.origin_chain(),
			OrderImpl::OnchainOrder(order) => order.origin_chain(),
		}
	}

	fn destination_chains(&self) -> Vec<ChainId> {
		match self {
			OrderImpl::GaslessOrder(order) => order.destination_chains(),
			OrderImpl::OnchainOrder(order) => order.destination_chains(),
		}
	}

	fn created_at(&self) -> Timestamp {
		match self {
			OrderImpl::GaslessOrder(order) => order.created_at(),
			OrderImpl::OnchainOrder(order) => order.created_at(),
		}
	}

	fn expires_at(&self) -> Timestamp {
		match self {
			OrderImpl::GaslessOrder(order) => order.expires_at(),
			OrderImpl::OnchainOrder(order) => order.expires_at(),
		}
	}

	async fn validate(&self) -> Result<()> {
		match self {
			OrderImpl::GaslessOrder(order) => order.validate().await,
			OrderImpl::OnchainOrder(order) => order.validate().await,
		}
	}

	async fn to_fill_instructions(&self) -> Result<Vec<FillInstruction>> {
		match self {
			OrderImpl::GaslessOrder(order) => order.to_fill_instructions().await,
			OrderImpl::OnchainOrder(order) => order.to_fill_instructions().await,
		}
	}

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn user(&self) -> Address {
		match self {
			OrderImpl::GaslessOrder(order) => order.user(),
			OrderImpl::OnchainOrder(order) => order.user(),
		}
	}

	fn inputs(&self) -> Result<Vec<Input>> {
		match self {
			OrderImpl::GaslessOrder(order) => order.inputs(),
			OrderImpl::OnchainOrder(order) => order.inputs(),
		}
	}

	fn outputs(&self) -> Result<Vec<Output>> {
		match self {
			OrderImpl::GaslessOrder(order) => order.outputs(),
			OrderImpl::OnchainOrder(order) => order.outputs(),
		}
	}
}
