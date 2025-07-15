//! Main solver service implementation with integrated monitoring.

use anyhow::Result;
use solver_config::types::SolverConfig;
use solver_core::SolverCoordinator;
use solver_monitoring::{
	health::{HealthChecker, HealthStatus, MemoryHealthCheck, PingHealthCheck},
	metrics::{MetricsRegistry, SolverMetricsCollector, SystemMetricsCollector},
	tracing::{init_tracing, SolverTracing, TracingConfig},
};
use solver_types::chains::ChainId;
use std::{sync::Arc, time::Duration};
use tokio::signal;
use tracing::{error, info, instrument};

/// Main solver service with integrated monitoring
pub struct SolverService {
	coordinator: Arc<SolverCoordinator>,
	api_server: Option<crate::api::ApiServer>,

	// Monitoring components
	health_checker: Arc<HealthChecker>,
	metrics_registry: Arc<MetricsRegistry>,
	solver_tracing: Arc<SolverTracing>,
	#[allow(dead_code)]
	solver_metrics: Arc<SolverMetricsCollector>,
}

impl SolverService {
	/// Create new solver service with monitoring
	#[instrument(skip(config))]
	pub async fn new(config: SolverConfig) -> Result<Self> {
		info!("Initializing solver service with monitoring");

		// Initialize monitoring first
		let (health_checker, metrics_registry, solver_tracing, solver_metrics) =
			Self::init_monitoring(&config).await?;

		// Initialize solver coordinator with tracing
		let coordinator = solver_tracing
			.trace_solver_operation("init_coordinator", async {
				SolverCoordinator::new(config.clone()).await
			})
			.await?;
		let coordinator = Arc::new(coordinator);

		// Initialize API server with monitoring endpoints
		let api_server = if config.monitoring.enabled {
			let _coordinator_stats = coordinator.stats().await;
			Some(crate::api::ApiServer::new(
				config.monitoring.health_port,
				coordinator.clone(),
				health_checker.clone(),
				metrics_registry.clone(),
			))
		} else {
			None
		};

		info!("Solver service initialized successfully");

		Ok(Self {
			coordinator,
			api_server,
			health_checker,
			metrics_registry,
			solver_tracing,
			solver_metrics,
		})
	}

	/// Initialize monitoring system
	async fn init_monitoring(
		config: &SolverConfig,
	) -> Result<(
		Arc<HealthChecker>,
		Arc<MetricsRegistry>,
		Arc<SolverTracing>,
		Arc<SolverMetricsCollector>,
	)> {
		info!("Initializing monitoring system");

		// Initialize tracing first (only if not already initialized by main)
		// Use debug mode for now, can be configured via environment variable
		let tracing_config = TracingConfig::debug();

		// Ignore error if tracing is already initialized
		let _ = init_tracing(tracing_config);

		// Create monitoring components
		let health_checker = Arc::new(HealthChecker::new(Duration::from_secs(30)));
		let metrics_registry = Arc::new(MetricsRegistry::new(Duration::from_secs(10)));
		let solver_tracing = Arc::new(SolverTracing::new(&config.solver.name));
		let solver_metrics = Arc::new(SolverMetricsCollector::new(&config.solver.name));

		// Register health checks
		health_checker
			.register_check(Box::new(PingHealthCheck::new("solver-ping")))
			.await;
		health_checker
			.register_check(Box::new(MemoryHealthCheck::new(
				"solver-memory",
				1024, // 1GB warning threshold
				2048, // 2GB critical threshold
			)))
			.await;

		// Register metrics collectors
		metrics_registry
			.register_collector(Box::new(SystemMetricsCollector::new("system")))
			.await;
		metrics_registry
			.register_collector(Box::new(SolverMetricsCollector::new(&config.solver.name)))
			.await;

		// Start periodic monitoring
		health_checker.start_periodic_checks().await;
		metrics_registry.start_periodic_collection().await;

		info!("Monitoring system initialized");

		Ok((
			health_checker,
			metrics_registry,
			solver_tracing,
			solver_metrics,
		))
	}

	/// Run the solver service with monitoring
	#[instrument(skip(self))]
	pub async fn run(self) -> Result<()> {
		info!("Starting solver service with monitoring");

		// Start coordinator with tracing
		self.solver_tracing
			.trace_solver_operation("coordinator_start", async {
				self.coordinator.start().await
			})
			.await?;

		// Start API server
		if let Some(api_server) = self.api_server {
			tokio::spawn(async move {
				if let Err(e) = api_server.run().await {
					error!("API server error: {}", e);
				}
			});
		}

		// Wait for shutdown signal
		signal::ctrl_c().await?;
		info!("Received shutdown signal");

		// Graceful shutdown with monitoring
		self.solver_tracing
			.trace_solver_operation("shutdown", async { self.coordinator.stop().await })
			.await?;

		info!("Solver service stopped");
		Ok(())
	}

	/// Get comprehensive service status including monitoring data
	pub async fn status(&self) -> ServiceStatus {
		let coordinator_stats = self.coordinator.stats().await;
		let health_status = self.health_checker.get_overall_health().await;
		let health_results = self.health_checker.get_last_results().await;
		let metrics = self.metrics_registry.get_last_collection().await;
		let solver_stats = self.solver_tracing.get_solver_stats().await;

		ServiceStatus {
			is_running: true,
			engine_stats: coordinator_stats.engine_stats,
			queue_stats: coordinator_stats.queue_stats,
			config_summary: ConfigSummary {
				solver_name: coordinator_stats.config_summary.solver_name,
				monitored_chains: coordinator_stats.config_summary.monitored_chains,
				storage_backend: coordinator_stats.config_summary.storage_backend,
				settlement_type: coordinator_stats.config_summary.settlement_type,
			},
			health: HealthSummary {
				overall_status: health_status,
				checks: health_results
					.into_iter()
					.map(|(name, result)| HealthCheckSummary {
						name,
						status: result.status,
						message: result.message,
						duration_ms: result.duration.as_millis() as u64,
					})
					.collect(),
			},
			metrics_summary: MetricsSummary {
				total_metrics: metrics.len(),
				solver_metrics: self.extract_solver_metrics(&metrics),
				system_metrics: self.extract_system_metrics(&metrics),
			},
			tracing_stats: solver_stats,
		}
	}

	fn extract_solver_metrics(
		&self,
		metrics: &[solver_monitoring::metrics::Metric],
	) -> SolverMetricsSummary {
		let mut summary = SolverMetricsSummary::default();

		for metric in metrics {
			match metric.name.as_str() {
				"solver.solutions.total" => {
					if let Some(count) = metric.value.as_counter() {
						summary.total_solutions = count;
					}
				}
				"solver.errors.total" => {
					if let Some(count) = metric.value.as_counter() {
						summary.total_errors = count;
					}
				}
				"solver.solution_time.avg_ms" => {
					if let Some(avg) = metric.value.as_gauge() {
						summary.avg_solution_time_ms = avg;
					}
				}
				_ => {}
			}
		}

		summary
	}

	fn extract_system_metrics(
		&self,
		metrics: &[solver_monitoring::metrics::Metric],
	) -> SystemMetricsSummary {
		let mut summary = SystemMetricsSummary::default();

		for metric in metrics {
			match metric.name.as_str() {
				"system.memory.usage_mb" => {
					if let Some(usage) = metric.value.as_gauge() {
						summary.memory_usage_mb = usage;
					}
				}
				"system.cpu.usage_percent" => {
					if let Some(usage) = metric.value.as_gauge() {
						summary.cpu_usage_percent = usage;
					}
				}
				_ => {}
			}
		}

		summary
	}
}

#[derive(Debug, serde::Serialize)]
pub struct ServiceStatus {
	pub is_running: bool,
	pub engine_stats: solver_core::EngineStats,
	pub queue_stats: solver_state::manager::StateStats,
	pub config_summary: ConfigSummary,
	pub health: HealthSummary,
	pub metrics_summary: MetricsSummary,
	pub tracing_stats: solver_monitoring::tracing::SpanStats,
}

#[derive(Debug, serde::Serialize)]
pub struct ConfigSummary {
	pub solver_name: String,
	pub monitored_chains: Vec<ChainId>,
	pub storage_backend: String,
	pub settlement_type: String,
}

#[derive(Debug, serde::Serialize)]
pub struct HealthSummary {
	pub overall_status: HealthStatus,
	pub checks: Vec<HealthCheckSummary>,
}

#[derive(Debug, serde::Serialize)]
pub struct HealthCheckSummary {
	pub name: String,
	pub status: HealthStatus,
	pub message: String,
	pub duration_ms: u64,
}

#[derive(Debug, serde::Serialize)]
pub struct MetricsSummary {
	pub total_metrics: usize,
	pub solver_metrics: SolverMetricsSummary,
	pub system_metrics: SystemMetricsSummary,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SolverMetricsSummary {
	pub total_solutions: u64,
	pub total_errors: u64,
	pub avg_solution_time_ms: f64,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SystemMetricsSummary {
	pub memory_usage_mb: f64,
	pub cpu_usage_percent: f64,
}
