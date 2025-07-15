//! HTTP API server with monitoring endpoints.

use axum::{extract::State, http::StatusCode, response::Json, routing::get, Router};
use solver_core::SolverCoordinator;
use solver_monitoring::{
	health::{HealthChecker, HealthStatus},
	metrics::{Metric, MetricsRegistry},
};
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{info, instrument};

/// API server with monitoring endpoints
pub struct ApiServer {
	port: u16,
	coordinator: Arc<SolverCoordinator>,
	health_checker: Arc<HealthChecker>,
	metrics_registry: Arc<MetricsRegistry>,
}

impl ApiServer {
	pub fn new(
		port: u16,
		coordinator: Arc<SolverCoordinator>,
		health_checker: Arc<HealthChecker>,
		metrics_registry: Arc<MetricsRegistry>,
	) -> Self {
		Self {
			port,
			coordinator,
			health_checker,
			metrics_registry,
		}
	}

	#[instrument(skip(self))]
	pub async fn run(self) -> anyhow::Result<()> {
		let shared_state = AppState {
			coordinator: self.coordinator,
			health_checker: self.health_checker,
			metrics_registry: self.metrics_registry,
		};

		let app = Router::new()
            // Basic endpoints
            .route("/health", get(health_check))
            .route("/status", get(get_status))
            .route("/metrics", get(get_metrics))
            // Detailed monitoring endpoints
            .route("/health/detailed", get(get_detailed_health))
            .route("/metrics/prometheus", get(get_prometheus_metrics))
            .route("/metrics/solver", get(get_solver_metrics))
            .route("/metrics/system", get(get_system_metrics))
            // Debug endpoints
            .route("/debug/config", get(get_config_debug))
            .route("/debug/tracing", get(get_tracing_debug))
            .with_state(shared_state)
            .layer(TraceLayer::new_for_http())
            .layer(CorsLayer::permissive());

		let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", self.port)).await?;

		info!("API server listening on port {}", self.port);

		axum::serve(listener, app).await?;

		Ok(())
	}
}

#[derive(Clone)]
struct AppState {
	coordinator: Arc<SolverCoordinator>,
	health_checker: Arc<HealthChecker>,
	metrics_registry: Arc<MetricsRegistry>,
}

/// Basic health check - returns 200 if service is running
async fn health_check(State(state): State<AppState>) -> StatusCode {
	let health = state.health_checker.get_overall_health().await;
	match health {
		HealthStatus::Healthy => StatusCode::OK,
		HealthStatus::Degraded => StatusCode::OK, // Still consider it healthy for basic check
		HealthStatus::Unhealthy => StatusCode::SERVICE_UNAVAILABLE,
	}
}

/// Detailed health check with individual check results
async fn get_detailed_health(State(state): State<AppState>) -> Json<serde_json::Value> {
	let overall_health = state.health_checker.get_overall_health().await;
	let health_results = state.health_checker.get_last_results().await;

	let checks: Vec<serde_json::Value> = health_results
		.into_iter()
		.map(|(name, result)| {
			serde_json::json!({
				"name": name,
				"status": format!("{:?}", result.status),
				"message": result.message,
				"duration_ms": result.duration.as_millis(),
				"timestamp": result.timestamp.elapsed().as_secs(),
				"details": result.details
			})
		})
		.collect();

	Json(serde_json::json!({
		"overall_status": format!("{:?}", overall_health),
		"checks": checks,
		"timestamp": chrono::Utc::now().timestamp()
	}))
}

/// Basic status with engine information
async fn get_status(State(state): State<AppState>) -> Json<serde_json::Value> {
	let stats = state.coordinator.stats().await;
	let health = state.health_checker.get_overall_health().await;

	Json(serde_json::json!({
		"status": "running",
		"health": format!("{:?}", health),
		"engine": {
			"is_running": stats.engine_stats.is_running,
			"processed_orders": stats.engine_stats.processed_orders,
			"failed_orders": stats.engine_stats.failed_orders,
		},
		"timestamp": chrono::Utc::now().timestamp()
	}))
}

/// All metrics in JSON format
async fn get_metrics(State(state): State<AppState>) -> Json<serde_json::Value> {
	let metrics = state.metrics_registry.collect_all_metrics().await;
	let stats = state.coordinator.stats().await;

	let metrics_json: Vec<serde_json::Value> = metrics
		.into_iter()
		.map(|metric| {
			serde_json::json!({
				"name": metric.name,
				"value": format_metric_value(&metric),
				"tags": metric.tags,
				"description": metric.description,
				"timestamp": metric.timestamp.elapsed().as_secs()
			})
		})
		.collect();

	Json(serde_json::json!({
		"metrics": metrics_json,
		"engine": {
			"processed_orders_total": stats.engine_stats.processed_orders,
			"failed_orders_total": stats.engine_stats.failed_orders,
			"success_rate": calculate_success_rate(stats.engine_stats.processed_orders, stats.engine_stats.failed_orders),
			"engine_running": stats.engine_stats.is_running,
		},
		"timestamp": chrono::Utc::now().timestamp()
	}))
}

/// Prometheus-format metrics
async fn get_prometheus_metrics(State(state): State<AppState>) -> String {
	let metrics = state.metrics_registry.collect_all_metrics().await;
	let stats = state.coordinator.stats().await;

	let mut prometheus_output = String::new();

	// Add engine metrics
	prometheus_output
		.push_str("# HELP solver_processed_orders_total Total number of processed orders\n");
	prometheus_output.push_str("# TYPE solver_processed_orders_total counter\n");
	prometheus_output.push_str(&format!(
		"solver_processed_orders_total {}\n",
		stats.engine_stats.processed_orders
	));

	prometheus_output.push_str("# HELP solver_failed_orders_total Total number of failed orders\n");
	prometheus_output.push_str("# TYPE solver_failed_orders_total counter\n");
	prometheus_output.push_str(&format!(
		"solver_failed_orders_total {}\n",
		stats.engine_stats.failed_orders
	));

	prometheus_output.push_str("# HELP solver_success_rate Success rate percentage\n");
	prometheus_output.push_str("# TYPE solver_success_rate gauge\n");
	prometheus_output.push_str(&format!(
		"solver_success_rate {}\n",
		calculate_success_rate(
			stats.engine_stats.processed_orders,
			stats.engine_stats.failed_orders
		)
	));

	// Add collected metrics
	for metric in metrics {
		let metric_name = metric.name.replace(".", "_");
		let value = match &metric.value {
			solver_monitoring::metrics::MetricValue::Counter(v) => v.to_string(),
			solver_monitoring::metrics::MetricValue::Gauge(v) => v.to_string(),
			solver_monitoring::metrics::MetricValue::Timer(v) => v.as_millis().to_string(),
			solver_monitoring::metrics::MetricValue::Histogram(v) => v.len().to_string(),
		};

		let metric_type = match &metric.value {
			solver_monitoring::metrics::MetricValue::Counter(_) => "counter",
			solver_monitoring::metrics::MetricValue::Gauge(_) => "gauge",
			solver_monitoring::metrics::MetricValue::Timer(_) => "gauge",
			solver_monitoring::metrics::MetricValue::Histogram(_) => "histogram",
		};

		if !metric.description.is_empty() {
			prometheus_output.push_str(&format!("# HELP {} {}\n", metric_name, metric.description));
		}
		prometheus_output.push_str(&format!("# TYPE {} {}\n", metric_name, metric_type));

		if metric.tags.is_empty() {
			prometheus_output.push_str(&format!("{} {}\n", metric_name, value));
		} else {
			let tags: Vec<String> = metric
				.tags
				.iter()
				.map(|(k, v)| format!("{}=\"{}\"", k, v))
				.collect();
			prometheus_output.push_str(&format!(
				"{}{{{}}} {}\n",
				metric_name,
				tags.join(","),
				value
			));
		}
	}

	prometheus_output
}

/// Solver-specific metrics
async fn get_solver_metrics(State(state): State<AppState>) -> Json<serde_json::Value> {
	let metrics = state.metrics_registry.collect_all_metrics().await;

	let solver_metrics: Vec<&Metric> = metrics
		.iter()
		.filter(|m| m.name.starts_with("solver."))
		.collect();

	let metrics_json: Vec<serde_json::Value> = solver_metrics
		.into_iter()
		.map(|metric| {
			serde_json::json!({
				"name": metric.name,
				"value": format_metric_value(metric),
				"tags": metric.tags,
				"description": metric.description
			})
		})
		.collect();

	Json(serde_json::json!({
		"solver_metrics": metrics_json,
		"timestamp": chrono::Utc::now().timestamp()
	}))
}

/// System metrics
async fn get_system_metrics(State(state): State<AppState>) -> Json<serde_json::Value> {
	let metrics = state.metrics_registry.collect_all_metrics().await;

	let system_metrics: Vec<&Metric> = metrics
		.iter()
		.filter(|m| m.name.starts_with("system."))
		.collect();

	let metrics_json: Vec<serde_json::Value> = system_metrics
		.into_iter()
		.map(|metric| {
			serde_json::json!({
				"name": metric.name,
				"value": format_metric_value(metric),
				"tags": metric.tags,
				"description": metric.description
			})
		})
		.collect();

	Json(serde_json::json!({
		"system_metrics": metrics_json,
		"timestamp": chrono::Utc::now().timestamp()
	}))
}

/// Debug endpoint for configuration
async fn get_config_debug(State(_state): State<AppState>) -> Json<serde_json::Value> {
	// In a real implementation, you'd want to expose non-sensitive config
	Json(serde_json::json!({
		"message": "Config debug endpoint - implement based on your config structure",
		"timestamp": chrono::Utc::now().timestamp()
	}))
}

/// Debug endpoint for tracing information
async fn get_tracing_debug(State(_state): State<AppState>) -> Json<serde_json::Value> {
	Json(serde_json::json!({
		"message": "Tracing debug endpoint - implement based on your tracing needs",
		"timestamp": chrono::Utc::now().timestamp()
	}))
}

// Helper functions

fn format_metric_value(metric: &Metric) -> serde_json::Value {
	match &metric.value {
		solver_monitoring::metrics::MetricValue::Counter(v) => serde_json::json!(v),
		solver_monitoring::metrics::MetricValue::Gauge(v) => serde_json::json!(v),
		solver_monitoring::metrics::MetricValue::Timer(v) => serde_json::json!({
			"milliseconds": v.as_millis(),
			"seconds": v.as_secs_f64()
		}),
		solver_monitoring::metrics::MetricValue::Histogram(ref v) => serde_json::json!({
			"values": v,
			"count": v.len(),
			"min": v.iter().fold(f64::INFINITY, |a, &b| a.min(b)),
			"max": v.iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b)),
			"avg": if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 }
		}),
	}
}

fn calculate_success_rate(processed: u64, failed: u64) -> f64 {
	let total = processed + failed;
	if total > 0 {
		(processed as f64 / total as f64) * 100.0
	} else {
		0.0
	}
}
