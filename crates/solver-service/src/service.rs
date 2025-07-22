//! # Solver Service
//!
//! High-level service wrapper that provides an API interface to the solver orchestrator.
//!
//! This module contains the main service struct that wraps the orchestrator and
//! provides convenient methods for accessing solver functionality through HTTP
//! endpoints and other interfaces.

use solver_core::{HealthReport, Orchestrator};
use solver_types::SolverConfig;
use std::sync::Arc;

/// Main service wrapper that provides access to solver functionality.
///
/// Acts as a facade over the orchestrator, providing a clean API interface
/// for external consumers while maintaining access to configuration and
/// orchestrator methods.
#[derive(Clone)]
pub struct SolverService {
	/// The core orchestrator managing all solver operations
	orchestrator: Arc<Orchestrator>,
	/// Service configuration
	config: SolverConfig,
}

impl SolverService {
	/// Create a new solver service wrapper.
	///
	/// # Arguments
	/// * `orchestrator` - The orchestrator instance to wrap
	/// * `config` - The service configuration
	pub fn new(orchestrator: Arc<Orchestrator>, config: SolverConfig) -> Self {
		Self {
			orchestrator,
			config,
		}
	}

	/// Get a reference to the service configuration.
	pub fn config(&self) -> &SolverConfig {
		&self.config
	}

	/// Get the current health status of all system components.
	pub async fn health(&self) -> HealthReport {
		self.orchestrator.get_health().await
	}

	/// Retrieve information about a specific order by ID.
	///
	/// # Arguments
	/// * `order_id` - The unique identifier of the order
	///
	/// # Returns
	/// Order information if found, None if not found
	pub async fn get_order(
		&self,
		order_id: &str,
	) -> Result<Option<solver_core::OrderInfo>, solver_core::CoreError> {
		self.orchestrator.get_order(order_id).await
	}

	/// Retrieve information about a specific settlement by ID.
	///
	/// # Arguments
	/// * `settlement_id` - The unique identifier of the settlement
	///
	/// # Returns
	/// Settlement information if found, None if not found
	pub async fn get_settlement(
		&self,
		settlement_id: &str,
	) -> Result<Option<solver_core::SettlementEvent>, solver_core::CoreError> {
		self.orchestrator.get_settlement(settlement_id).await
	}
}
