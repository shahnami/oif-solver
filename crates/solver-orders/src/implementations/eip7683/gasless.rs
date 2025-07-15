//! Gasless order implementation.

use async_trait::async_trait;
use solver_types::{
	chains::ChainId,
	common::*,
	errors::Result,
	orders::{
		FillData, FillInstruction, Input, Order, OrderId, OrderSemantics, OrderStandard,
		Output as OrderOutput,
	},
	standards::eip7683::{GaslessCrossChainOrder, Output, ResolvedCrossChainOrder},
};
use std::any::Any;
use tracing::debug;

use super::types::{compute_domain_separator, compute_order_id, parse_order_data_subtype};

/// Wrapper for gasless orders with additional functionality
pub struct GaslessOrder {
	inner: GaslessCrossChainOrder,
	// Cached computed values
	order_id: Option<OrderId>,
}

impl GaslessOrder {
	pub fn new(inner: GaslessCrossChainOrder) -> Self {
		Self {
			inner,
			order_id: None,
		}
	}

	/// Get the inner order
	pub fn inner(&self) -> &GaslessCrossChainOrder {
		&self.inner
	}

	/// Resolve the order to standard format
	pub async fn resolve(&self, _filler_data: Option<&[u8]>) -> Result<ResolvedCrossChainOrder> {
		Err(solver_types::errors::SolverError::NotImplemented(
			"Order resolution not yet implemented".to_string(),
		))
	}

	/// Compute and cache order ID
	fn compute_order_id(&mut self) -> OrderId {
		if let Some(id) = self.order_id {
			return id;
		}

		// Compute EIP-712 hash
		let domain_separator =
			compute_domain_separator(self.inner.origin_chain_id, self.inner.origin_settler);
		let order_struct_hash = Bytes32::zero(); // Would compute actual struct hash

		let id = compute_order_id(order_struct_hash, domain_separator);
		self.order_id = Some(id);
		id
	}
}

#[async_trait]
impl Order for GaslessOrder {
	fn id(&self) -> OrderId {
		// Need mutable self to cache, so clone for now
		let mut order = self.clone();
		order.compute_order_id()
	}

	fn standard(&self) -> OrderStandard {
		OrderStandard::EIP7683
	}

	fn origin_chain(&self) -> ChainId {
		ChainId(self.inner.origin_chain_id.as_u64())
	}

	fn destination_chains(&self) -> Vec<ChainId> {
		// Would parse from orderData
		vec![ChainId(1)] // Placeholder
	}

	fn created_at(&self) -> Timestamp {
		// Use open deadline as proxy for creation time
		self.inner.open_deadline as u64
	}

	fn expires_at(&self) -> Timestamp {
		self.inner.fill_deadline as u64
	}

	async fn validate(&self) -> Result<()> {
		let now = chrono::Utc::now().timestamp() as u32;

		// Check deadlines
		if now > self.inner.open_deadline {
			return Err(solver_types::errors::SolverError::Order(
				"Order open deadline has passed".to_string(),
			));
		}

		if now > self.inner.fill_deadline {
			return Err(solver_types::errors::SolverError::OrderExpired {
				expired_at: self.inner.fill_deadline as u64,
			});
		}

		// Validate nonce is not zero
		if self.inner.nonce == U256::zero() {
			return Err(solver_types::errors::SolverError::Order(
				"Invalid nonce: must be non-zero".to_string(),
			));
		}

		Ok(())
	}

	fn semantics(&self) -> OrderSemantics {
		// Parse orderData to determine semantics
		match parse_order_data_subtype(self.inner.order_data_type, &self.inner.order_data) {
			Ok(_subtype) => {
				// Analyze subtype to determine semantics
				OrderSemantics::Custom("EIP-7683".to_string())
			}
			Err(_) => OrderSemantics::Custom("Unknown".to_string()),
		}
	}

	async fn to_fill_instructions(&self) -> Result<Vec<FillInstruction>> {
		debug!("Converting order {} to fill instructions", self.id());

		let resolved = self.resolve(None).await?;

		resolved
			.fill_instructions
			.into_iter()
			.map(|inst| {
				Ok(FillInstruction {
					destination_chain: ChainId(inst.destination_chain_id.as_u64()),
					destination_contract: Output::bytes32_to_address(inst.destination_settler)?,
					fill_data: FillData::EIP7683 {
						order_id: resolved.order_id,
						origin_data: inst.origin_data,
					},
				})
			})
			.collect()
	}

	fn as_any(&self) -> &dyn Any {
		self
	}

	fn user(&self) -> Address {
		self.inner.user
	}

	fn inputs(&self) -> Result<Vec<Input>> {
		// For gasless orders, we need to parse the order_data to get inputs
		// This is typically encoded in a standard format based on order_data_type
		// For now, return an error indicating this needs implementation
		Err(solver_types::errors::SolverError::NotImplemented(
			"Input extraction for gasless orders not yet implemented".to_string(),
		))
	}

	fn outputs(&self) -> Result<Vec<OrderOutput>> {
		// For gasless orders, we need to parse the order_data to get outputs
		// This is typically encoded in a standard format based on order_data_type
		// For now, return an error indicating this needs implementation
		Err(solver_types::errors::SolverError::NotImplemented(
			"Output extraction for gasless orders not yet implemented".to_string(),
		))
	}
}

impl Clone for GaslessOrder {
	fn clone(&self) -> Self {
		Self {
			inner: self.inner.clone(),
			order_id: self.order_id,
		}
	}
}

impl std::fmt::Debug for GaslessOrder {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("GaslessOrder")
			.field("inner", &self.inner)
			.field("order_id", &self.order_id)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_gasless_order_creation() {
		let inner = GaslessCrossChainOrder {
			origin_settler: Address::zero(),
			user: Address::from([1u8; 20]),
			nonce: U256::from(123),
			origin_chain_id: U256::from(1),
			open_deadline: 1000,
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![],
		};

		let order = GaslessOrder::new(inner.clone());
		assert_eq!(order.inner().user, inner.user);
		assert_eq!(order.inner().nonce, inner.nonce);
	}

	#[tokio::test]
	async fn test_gasless_order_validation() {
		let mut inner = GaslessCrossChainOrder {
			origin_settler: Address::zero(),
			user: Address::zero(),
			nonce: U256::from(1),
			origin_chain_id: U256::from(1),
			open_deadline: 1000,
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![],
		};

		// Test expired open deadline
		inner.open_deadline = 0;
		let order = GaslessOrder::new(inner.clone());
		let result = order.validate().await;
		assert!(result.is_err());

		// Test zero nonce
		inner.open_deadline = u32::MAX;
		inner.fill_deadline = u32::MAX;
		inner.nonce = U256::zero();
		let order = GaslessOrder::new(inner.clone());
		let result = order.validate().await;
		assert!(result.is_err());
		if let Err(e) = result {
			let err_string = e.to_string();
			assert!(
				err_string.contains("nonce") || err_string.contains("Invalid nonce"),
				"Expected error to contain 'nonce', got: {}",
				err_string
			);
		}
	}

	#[test]
	fn test_order_trait_implementation() {
		let inner = GaslessCrossChainOrder {
			origin_settler: Address::zero(),
			user: Address::zero(),
			nonce: U256::from(1),
			origin_chain_id: U256::from(42161),
			open_deadline: 1000,
			fill_deadline: 2000,
			order_data_type: Bytes32::zero(),
			order_data: vec![],
		};

		let order = GaslessOrder::new(inner);
		assert_eq!(order.standard(), OrderStandard::EIP7683);
		assert_eq!(order.origin_chain(), ChainId(42161));
		assert_eq!(order.expires_at(), 2000);
	}
}
