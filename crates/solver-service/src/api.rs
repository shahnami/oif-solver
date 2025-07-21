use crate::service::SolverService;
use anyhow::Result;
use axum::{
	extract::{Path, State},
	http::StatusCode,
	response::{IntoResponse, Json},
	routing::get,
	Router,
};
use serde::Serialize;
use solver_types::ServiceStatus;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Health check response
#[derive(Serialize)]
struct HealthResponse {
	status: String,
	services: ServiceHealthStatus,
}

#[derive(Serialize)]
struct ServiceHealthStatus {
	discovery: bool,
	delivery: bool,
	state: bool,
	event_processor: bool,
}

/// API error response
#[derive(Serialize)]
struct ErrorResponse {
	error: String,
}

/// Order response
#[derive(Serialize)]
struct OrderResponse {
	order_id: String,
	status: String,
}

pub async fn start_http_server(service: SolverService, port: u16) -> Result<()> {
	let app = create_app(service);

	let addr = SocketAddr::from(([0, 0, 0, 0], port));
	info!("HTTP server listening on {}", addr);

	axum::serve(
		tokio::net::TcpListener::bind(addr).await?,
		app.into_make_service(),
	)
	.await?;

	Ok(())
}

pub async fn start_metrics_server(port: u16) -> Result<()> {
	let app = Router::new().route("/metrics", get(metrics_handler));

	let addr = SocketAddr::from(([0, 0, 0, 0], port));
	info!("Metrics server listening on {}", addr);

	axum::serve(
		tokio::net::TcpListener::bind(addr).await?,
		app.into_make_service(),
	)
	.await?;

	Ok(())
}

fn create_app(service: SolverService) -> Router {
	Router::new()
		// Health endpoints
		.route("/health", get(health_handler))
		.route("/health/live", get(liveness_handler))
		.route("/health/ready", get(readiness_handler))
		// Order endpoints
		.route("/api/v1/orders/{order_id}", get(get_order_handler))
		// Admin endpoints
		.route("/api/v1/admin/config", get(get_config_handler))
		// Add state
		.with_state(service)
		// Add middleware
		.layer(CorsLayer::permissive())
		.layer(TraceLayer::new_for_http())
}

/// Health check handler
async fn health_handler(State(service): State<SolverService>) -> impl IntoResponse {
	let health = service.health().await;

	let response = HealthResponse {
		status: match health.overall_status {
			ServiceStatus::Healthy => "healthy".to_string(),
			ServiceStatus::Degraded => "degraded".to_string(),
			ServiceStatus::Unhealthy => "unhealthy".to_string(),
			ServiceStatus::Starting => "starting".to_string(),
			ServiceStatus::Stopping => "stopping".to_string(),
		},
		services: ServiceHealthStatus {
			discovery: health.discovery_healthy,
			delivery: health.delivery_healthy,
			state: health.state_healthy,
			event_processor: health.event_processor_healthy,
		},
	};

	let status_code = match health.overall_status {
		ServiceStatus::Healthy => StatusCode::OK,
		ServiceStatus::Degraded => StatusCode::OK,
		ServiceStatus::Unhealthy => StatusCode::SERVICE_UNAVAILABLE,
		ServiceStatus::Starting => StatusCode::SERVICE_UNAVAILABLE,
		ServiceStatus::Stopping => StatusCode::SERVICE_UNAVAILABLE,
	};

	(status_code, Json(response))
}

/// Kubernetes liveness probe
async fn liveness_handler() -> impl IntoResponse {
	StatusCode::OK
}

/// Kubernetes readiness probe
async fn readiness_handler(State(service): State<SolverService>) -> impl IntoResponse {
	let health = service.health().await;

	match health.overall_status {
		ServiceStatus::Healthy => StatusCode::OK,
		_ => StatusCode::SERVICE_UNAVAILABLE,
	}
}

/// Get order status
async fn get_order_handler(
	State(_service): State<SolverService>,
	Path(order_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ErrorResponse>)> {
	// TODO: Implement order status retrieval from state service
	Ok((
		StatusCode::OK,
		Json(OrderResponse {
			order_id,
			status: "pending".to_string(),
		}),
	))
}

/// Get current configuration (admin endpoint)
async fn get_config_handler(State(service): State<SolverService>) -> impl IntoResponse {
	Json(service.config().clone())
}

/// Metrics handler (placeholder for now)
async fn metrics_handler() -> impl IntoResponse {
	// TODO: Implement proper metrics collection
	// For now, return a simple response
	"# HELP solver_health Solver service health status\n\
	 # TYPE solver_health gauge\n\
	 solver_health 1\n"
}
