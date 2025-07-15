use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Health status of a component
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HealthStatus {
	Healthy,
	Degraded,
	Unhealthy,
}

impl HealthStatus {
	pub fn is_healthy(&self) -> bool {
		matches!(self, HealthStatus::Healthy)
	}

	pub fn is_degraded(&self) -> bool {
		matches!(self, HealthStatus::Degraded)
	}

	pub fn is_unhealthy(&self) -> bool {
		matches!(self, HealthStatus::Unhealthy)
	}
}

/// Health check result with details
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
	pub status: HealthStatus,
	pub message: String,
	pub timestamp: Instant,
	pub duration: Duration,
	pub details: HashMap<String, String>,
}

impl HealthCheckResult {
	pub fn healthy(message: impl Into<String>, duration: Duration) -> Self {
		Self {
			status: HealthStatus::Healthy,
			message: message.into(),
			timestamp: Instant::now(),
			duration,
			details: HashMap::new(),
		}
	}

	pub fn degraded(message: impl Into<String>, duration: Duration) -> Self {
		Self {
			status: HealthStatus::Degraded,
			message: message.into(),
			timestamp: Instant::now(),
			duration,
			details: HashMap::new(),
		}
	}

	pub fn unhealthy(message: impl Into<String>, duration: Duration) -> Self {
		Self {
			status: HealthStatus::Unhealthy,
			message: message.into(),
			timestamp: Instant::now(),
			duration,
			details: HashMap::new(),
		}
	}

	pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
		self.details.insert(key.into(), value.into());
		self
	}
}

/// Trait for implementing health checks
#[async_trait::async_trait]
pub trait HealthCheck: Send + Sync {
	async fn check(&self) -> HealthCheckResult;
	fn name(&self) -> &str;
}

/// Health check manager
pub struct HealthChecker {
	checks: Arc<RwLock<HashMap<String, Box<dyn HealthCheck>>>>,
	last_results: Arc<RwLock<HashMap<String, HealthCheckResult>>>,
	check_interval: Duration,
}

impl HealthChecker {
	pub fn new(check_interval: Duration) -> Self {
		Self {
			checks: Arc::new(RwLock::new(HashMap::new())),
			last_results: Arc::new(RwLock::new(HashMap::new())),
			check_interval,
		}
	}

	pub async fn register_check(&self, check: Box<dyn HealthCheck>) {
		let name = check.name().to_string();
		let mut checks = self.checks.write().await;
		checks.insert(name, check);
	}

	pub async fn run_all_checks(&self) -> HashMap<String, HealthCheckResult> {
		let checks = self.checks.read().await;
		let mut results = HashMap::new();

		for (name, check) in checks.iter() {
			let start = Instant::now();
			debug!("Running health check: {}", name);

			let result = check.check().await;
			let duration = start.elapsed();

			match result.status {
				HealthStatus::Healthy => info!("Health check '{}' passed in {:?}", name, duration),
				HealthStatus::Degraded => warn!(
					"Health check '{}' degraded in {:?}: {}",
					name, duration, result.message
				),
				HealthStatus::Unhealthy => error!(
					"Health check '{}' failed in {:?}: {}",
					name, duration, result.message
				),
			}

			results.insert(name.clone(), result);
		}

		// Update last results
		let mut last_results = self.last_results.write().await;
		*last_results = results.clone();

		results
	}

	pub async fn get_overall_health(&self) -> HealthStatus {
		let results = self.run_all_checks().await;

		if results.is_empty() {
			return HealthStatus::Healthy;
		}

		let mut has_degraded = false;

		for result in results.values() {
			match result.status {
				HealthStatus::Unhealthy => return HealthStatus::Unhealthy,
				HealthStatus::Degraded => has_degraded = true,
				HealthStatus::Healthy => {}
			}
		}

		if has_degraded {
			HealthStatus::Degraded
		} else {
			HealthStatus::Healthy
		}
	}

	pub async fn get_last_results(&self) -> HashMap<String, HealthCheckResult> {
		self.last_results.read().await.clone()
	}

	pub async fn start_periodic_checks(&self) {
		let checks = self.checks.clone();
		let last_results = self.last_results.clone();
		let interval = self.check_interval;

		tokio::spawn(async move {
			let mut interval_timer = tokio::time::interval(interval);

			loop {
				interval_timer.tick().await;

				let checks_guard = checks.read().await;
				let mut results = HashMap::new();

				for (name, check) in checks_guard.iter() {
					let start = Instant::now();
					let result = check.check().await;
					let duration = start.elapsed();

					match result.status {
						HealthStatus::Healthy => {
							debug!("Periodic health check '{}' passed in {:?}", name, duration)
						}
						HealthStatus::Degraded => warn!(
							"Periodic health check '{}' degraded in {:?}: {}",
							name, duration, result.message
						),
						HealthStatus::Unhealthy => error!(
							"Periodic health check '{}' failed in {:?}: {}",
							name, duration, result.message
						),
					}

					results.insert(name.clone(), result);
				}

				// Update last results
				let mut last_results_guard = last_results.write().await;
				*last_results_guard = results;
			}
		});
	}
}

/// Simple ping health check
pub struct PingHealthCheck {
	name: String,
}

impl PingHealthCheck {
	pub fn new(name: impl Into<String>) -> Self {
		Self { name: name.into() }
	}
}

#[async_trait::async_trait]
impl HealthCheck for PingHealthCheck {
	async fn check(&self) -> HealthCheckResult {
		let start = Instant::now();
		tokio::time::sleep(Duration::from_millis(1)).await;
		let duration = start.elapsed();

		HealthCheckResult::healthy("Ping successful", duration)
	}

	fn name(&self) -> &str {
		&self.name
	}
}

/// Memory usage health check
pub struct MemoryHealthCheck {
	name: String,
	warning_threshold_mb: u64,
	critical_threshold_mb: u64,
}

impl MemoryHealthCheck {
	pub fn new(
		name: impl Into<String>,
		warning_threshold_mb: u64,
		critical_threshold_mb: u64,
	) -> Self {
		Self {
			name: name.into(),
			warning_threshold_mb,
			critical_threshold_mb,
		}
	}
}

#[async_trait::async_trait]
impl HealthCheck for MemoryHealthCheck {
	async fn check(&self) -> HealthCheckResult {
		let start = Instant::now();

		// This is a simplified memory check - in a real implementation,
		// you'd want to use a proper memory monitoring crate
		let memory_usage_mb = 100; // Placeholder

		let duration = start.elapsed();

		if memory_usage_mb > self.critical_threshold_mb {
			HealthCheckResult::unhealthy(
				format!("Memory usage too high: {}MB", memory_usage_mb),
				duration,
			)
			.with_detail("memory_usage_mb", memory_usage_mb.to_string())
			.with_detail(
				"critical_threshold_mb",
				self.critical_threshold_mb.to_string(),
			)
		} else if memory_usage_mb > self.warning_threshold_mb {
			HealthCheckResult::degraded(
				format!("Memory usage elevated: {}MB", memory_usage_mb),
				duration,
			)
			.with_detail("memory_usage_mb", memory_usage_mb.to_string())
			.with_detail(
				"warning_threshold_mb",
				self.warning_threshold_mb.to_string(),
			)
		} else {
			HealthCheckResult::healthy(
				format!("Memory usage normal: {}MB", memory_usage_mb),
				duration,
			)
			.with_detail("memory_usage_mb", memory_usage_mb.to_string())
		}
	}

	fn name(&self) -> &str {
		&self.name
	}
}
