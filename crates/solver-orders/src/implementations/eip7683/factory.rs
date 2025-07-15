//! Factory for EIP-7683 orders.

use async_trait::async_trait;
use solver_types::{
	common::{Address, Bytes32, U256},
	errors::{Result, SolverError},
	orders::Order,
	standards::eip7683::{GaslessCrossChainOrder, OnchainCrossChainOrder},
};
use tracing::{debug, warn};

use super::{gasless::GaslessOrder, onchain::OnchainOrder};
use crate::factory::OrderFactory;

/// Marker byte for gasless orders
const GASLESS_ORDER_MARKER: u8 = 0x01;

/// Marker byte for onchain orders
const ONCHAIN_ORDER_MARKER: u8 = 0x02;

#[derive(Clone)]
pub struct EIP7683OrderFactory;

impl Default for EIP7683OrderFactory {
	fn default() -> Self {
		Self::new()
	}
}

impl EIP7683OrderFactory {
	pub fn new() -> Self {
		Self
	}

	/// Try to parse as gasless order
	async fn try_parse_gasless(&self, data: &[u8]) -> Result<GaslessCrossChainOrder> {
		// Gasless orders should have marker 0x01
		if data.is_empty() || data[0] != GASLESS_ORDER_MARKER {
			return Err(SolverError::Order(
				"Not a gasless order (invalid marker)".to_string(),
			));
		}

		Err(SolverError::NotImplemented(
			"Gasless order parsing".to_string(),
		))
	}

	/// Try to parse as onchain order (from event data)
	async fn try_parse_onchain(
		&self,
		data: &[u8],
	) -> Result<(OnchainCrossChainOrder, Bytes32, Address, U256, u64)> {
		// Format: [marker(1)] [order_id(32)] [user(20)] [origin_chain_id(32)] [timestamp(8)] [fill_deadline(4)] [order_data_type(32)] [order_data_len(4)] [order_data(...)]

		if data.is_empty() {
			return Err(SolverError::Order("Empty order data".to_string()));
		}

		if data[0] != ONCHAIN_ORDER_MARKER {
			return Err(SolverError::Order(format!(
				"Not an onchain order (marker: 0x{:02x}, expected: 0x{:02x})",
				data[0], ONCHAIN_ORDER_MARKER
			)));
		}

		let mut offset = 1;

		// Extract order ID
		if data.len() < offset + 32 {
			return Err(SolverError::Order(
				"Insufficient data for order ID".to_string(),
			));
		}
		let order_id = Bytes32::from_slice(&data[offset..offset + 32]);
		offset += 32;

		// Extract user address
		if data.len() < offset + 20 {
			return Err(SolverError::Order(
				"Insufficient data for user address".to_string(),
			));
		}
		let user = Address::from_slice(&data[offset..offset + 20]);
		offset += 20;

		// Extract origin chain ID
		if data.len() < offset + 32 {
			return Err(SolverError::Order(
				"Insufficient data for origin chain ID".to_string(),
			));
		}
		let origin_chain_id = U256::from_big_endian(&data[offset..offset + 32]);
		offset += 32;

		// Extract timestamp
		if data.len() < offset + 8 {
			return Err(SolverError::Order(
				"Insufficient data for timestamp".to_string(),
			));
		}
		let timestamp = u64::from_be_bytes(data[offset..offset + 8].try_into().unwrap());
		offset += 8;

		// Extract fill deadline
		if data.len() < offset + 4 {
			return Err(SolverError::Order(
				"Insufficient data for fill deadline".to_string(),
			));
		}
		let fill_deadline = u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap());
		offset += 4;

		// Extract order data type
		if data.len() < offset + 32 {
			return Err(SolverError::Order(
				"Insufficient data for order data type".to_string(),
			));
		}
		let order_data_type = Bytes32::from_slice(&data[offset..offset + 32]);
		offset += 32;

		// Extract order data length
		if data.len() < offset + 4 {
			return Err(SolverError::Order(
				"Insufficient data for order data length".to_string(),
			));
		}
		let order_data_len =
			u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
		offset += 4;

		// Extract order data
		if data.len() < offset + order_data_len {
			return Err(SolverError::Order(
				"Insufficient data for order data".to_string(),
			));
		}
		let order_data = data[offset..offset + order_data_len].to_vec();

		let onchain_order = OnchainCrossChainOrder {
			fill_deadline,
			order_data_type,
			order_data,
		};

		Ok((onchain_order, order_id, user, origin_chain_id, timestamp))
	}
}

#[async_trait]
impl OrderFactory for EIP7683OrderFactory {
	fn event_signatures(&self) -> Vec<Bytes32> {
		// Use the shared event signature from solver-types
		vec![solver_types::events::eip7683::open_event_topic()]
	}

	async fn parse_order(&self, data: &[u8]) -> Result<Box<dyn Order>> {
		debug!("Attempting to parse EIP-7683 order ({} bytes)", data.len());

		if data.is_empty() {
			return Err(SolverError::Order("Empty order data".to_string()));
		}

		// Log first few bytes for debugging
		let marker = data[0];
		debug!(
			"Order data preview: marker=0x{:02x}, data_len={}",
			marker,
			data.len()
		);

		// Use marker byte to determine order type
		match marker {
			GASLESS_ORDER_MARKER => {
				// Gasless order
				match self.try_parse_gasless(data).await {
					Ok(order) => {
						debug!("Successfully parsed as gasless order");
						Ok(Box::new(GaslessOrder::new(order)))
					}
					Err(e) => {
						debug!("Failed to parse gasless order: {}", e);
						Err(e)
					}
				}
			}
			ONCHAIN_ORDER_MARKER => {
				// Onchain order
				match self.try_parse_onchain(data).await {
					Ok((order, order_id, user, origin_chain_id, timestamp)) => {
						debug!("Successfully parsed as onchain order");
						Ok(Box::new(OnchainOrder::from_event(
							order,
							order_id,
							user,
							origin_chain_id,
							timestamp,
						)))
					}
					Err(e) => {
						debug!("Failed to parse onchain order: {}", e);
						Err(e)
					}
				}
			}
			_ => {
				warn!("Unknown order marker: 0x{:02x}", marker);
				Err(SolverError::Order(format!(
					"Unknown EIP-7683 order marker: 0x{:02x}",
					marker
				)))
			}
		}
	}

	async fn validate_format(&self, data: &[u8]) -> Result<()> {
		if data.is_empty() {
			return Err(SolverError::Order("Empty order data".to_string()));
		}

		// Check marker byte
		let marker = data[0];
		match marker {
			GASLESS_ORDER_MARKER => {
				// Gasless order - check minimum size for signature + order data
				if data.len() < 200 {
					return Err(SolverError::Order(
						"Gasless order data too short".to_string(),
					));
				}
			}
			ONCHAIN_ORDER_MARKER => {
				// Onchain order - check minimum size for required fields
				// marker(1) + order_id(32) + user(20) + origin_chain_id(32) + timestamp(8) + fill_deadline(4) + order_data_type(32) + order_data_len(4) = 133 bytes minimum
				if data.len() < 133 {
					return Err(SolverError::Order(
						"Onchain order data too short".to_string(),
					));
				}
			}
			_ => {
				return Err(SolverError::Order(format!(
					"Invalid order marker: 0x{:02x}",
					marker
				)));
			}
		}

		Ok(())
	}
}
