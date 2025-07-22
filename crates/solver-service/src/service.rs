use solver_core::{HealthReport, Orchestrator};
use solver_types::SolverConfig;
use std::sync::Arc;

/// Main service wrapper that holds the orchestrator and configuration
#[derive(Clone)]
pub struct SolverService {
	orchestrator: Arc<Orchestrator>,
	config: SolverConfig,
}

impl SolverService {
	pub fn new(orchestrator: Arc<Orchestrator>, config: SolverConfig) -> Self {
		Self {
			orchestrator,
			config,
		}
	}

	pub fn config(&self) -> &SolverConfig {
		&self.config
	}

	pub async fn health(&self) -> HealthReport {
		self.orchestrator.get_health().await
	}
}
