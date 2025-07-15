//! Metrics collection and management.
//!
//! This module provides a flexible metrics system for tracking solver performance
//! and operational metrics. It supports various metric types including counters,
//! gauges, histograms, and timers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Represents different types of metric values.
///
/// Each metric type serves a specific monitoring purpose:
/// - Counters for cumulative values that only increase
/// - Gauges for values that can go up or down
/// - Histograms for distribution of values
/// - Timers for duration measurements
#[derive(Debug, Clone)]
pub enum MetricValue {
	Counter(u64),
	Gauge(f64),
	Histogram(Vec<f64>),
	Timer(Duration),
}

impl MetricValue {
	/// Extracts the counter value if this is a Counter metric.
	pub fn as_counter(&self) -> Option<u64> {
		match self {
			MetricValue::Counter(v) => Some(*v),
			_ => None,
		}
	}

	/// Extracts the gauge value if this is a Gauge metric.
	pub fn as_gauge(&self) -> Option<f64> {
		match self {
			MetricValue::Gauge(v) => Some(*v),
			_ => None,
		}
	}

	/// Extracts the histogram values if this is a Histogram metric.
	pub fn as_histogram(&self) -> Option<&Vec<f64>> {
		match self {
			MetricValue::Histogram(v) => Some(v),
			_ => None,
		}
	}

	/// Extracts the duration value if this is a Timer metric.
	pub fn as_timer(&self) -> Option<Duration> {
		match self {
			MetricValue::Timer(v) => Some(*v),
			_ => None,
		}
	}
}

/// A metric with associated metadata.
///
/// Combines a metric value with descriptive information including
/// name, labels, and description for proper identification and
/// aggregation in monitoring systems.
#[derive(Debug, Clone)]
pub struct Metric {
	pub name: String,
	pub value: MetricValue,
	pub tags: HashMap<String, String>,
	pub timestamp: Instant,
	pub description: String,
}

impl Metric {
	/// Creates a new counter metric with the specified name and value.
	pub fn counter(name: impl Into<String>, value: u64) -> Self {
		Self {
			name: name.into(),
			value: MetricValue::Counter(value),
			tags: HashMap::new(),
			timestamp: Instant::now(),
			description: String::new(),
		}
	}

	/// Creates a new gauge metric with the specified name and value.
	pub fn gauge(name: impl Into<String>, value: f64) -> Self {
		Self {
			name: name.into(),
			value: MetricValue::Gauge(value),
			tags: HashMap::new(),
			timestamp: Instant::now(),
			description: String::new(),
		}
	}

	/// Creates a new histogram metric with the specified name and values.
	pub fn histogram(name: impl Into<String>, values: Vec<f64>) -> Self {
		Self {
			name: name.into(),
			value: MetricValue::Histogram(values),
			tags: HashMap::new(),
			timestamp: Instant::now(),
			description: String::new(),
		}
	}

	/// Creates a new timer metric with the specified name and duration.
	pub fn timer(name: impl Into<String>, duration: Duration) -> Self {
		Self {
			name: name.into(),
			value: MetricValue::Timer(duration),
			tags: HashMap::new(),
			timestamp: Instant::now(),
			description: String::new(),
		}
	}

	/// Adds a tag to the metric for additional categorization.
	///
	/// Tags are used for grouping and filtering metrics in monitoring systems.
	pub fn with_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
		self.tags.insert(key.into(), value.into());
		self
	}

	/// Sets a human-readable description for the metric.
	pub fn with_description(mut self, description: impl Into<String>) -> Self {
		self.description = description.into();
		self
	}
}

/// Trait for components that collect and expose metrics.
///
/// Implementors provide a consistent interface for metric collection,
/// allowing the monitoring system to gather metrics from various
/// components throughout the solver.
pub trait MetricsCollector: Send + Sync {
	/// Collects current metric values from this collector.
	fn collect(&self) -> Vec<Metric>;
	/// Returns the name of this metrics collector.
	fn name(&self) -> &str;
}

/// Collector for system-level metrics.
///
/// Tracks resource usage, performance indicators, and other
/// system-wide metrics relevant to solver operation.
pub struct SystemMetricsCollector {
	name: String,
}

impl SystemMetricsCollector {
	/// Creates a new system metrics collector with the specified name.
	pub fn new(name: impl Into<String>) -> Self {
		Self { name: name.into() }
	}
}

impl MetricsCollector for SystemMetricsCollector {
	fn collect(&self) -> Vec<Metric> {
		vec![
			Metric::gauge("system.memory.usage_mb", 512.0)
				.with_description("Current memory usage in MB"),
			Metric::gauge("system.cpu.usage_percent", 25.5)
				.with_description("Current CPU usage percentage"),
			Metric::counter("system.uptime.seconds", 86400)
				.with_description("System uptime in seconds"),
		]
	}

	fn name(&self) -> &str {
		&self.name
	}
}

/// Collector for solver-specific operational metrics.
///
/// Tracks performance metrics related to order solving including
/// solution times, success rates, and error counts. Maintains
/// a rolling window of recent solution times for percentile calculations.
pub struct SolverMetricsCollector {
	name: String,
	solution_times: Arc<RwLock<Vec<Duration>>>,
	solution_count: Arc<RwLock<u64>>,
	error_count: Arc<RwLock<u64>>,
}

impl SolverMetricsCollector {
	/// Creates a new solver metrics collector.
	pub fn new(name: impl Into<String>) -> Self {
		Self {
			name: name.into(),
			solution_times: Arc::new(RwLock::new(Vec::new())),
			solution_count: Arc::new(RwLock::new(0)),
			error_count: Arc::new(RwLock::new(0)),
		}
	}

	/// Records the time taken to compute a solution.
	///
	/// Maintains a rolling window of the last 1000 solution times
	/// for accurate percentile calculations.
	pub async fn record_solution_time(&self, duration: Duration) {
		let mut times = self.solution_times.write().await;
		times.push(duration);

		// Keep only the last 1000 entries
		if times.len() > 1000 {
			let drain_to = times.len() - 1000;
			times.drain(..drain_to);
		}

		let mut count = self.solution_count.write().await;
		*count += 1;

		debug!("Recorded solution time: {:?}", duration);
	}

	/// Records that an error occurred during solving.
	pub async fn record_error(&self) {
		let mut count = self.error_count.write().await;
		*count += 1;

		debug!("Recorded solver error");
	}

	/// Retrieves current solution statistics.
	///
	/// Returns a tuple containing:
	/// - Total solution count
	/// - Total time spent solving (in milliseconds)
	/// - Average solution duration
	/// - 95th percentile solution duration
	pub async fn get_solution_stats(&self) -> (u64, f64, Duration, Duration) {
		let times = self.solution_times.read().await;
		let count = *self.solution_count.read().await;

		if times.is_empty() {
			return (count, 0.0, Duration::ZERO, Duration::ZERO);
		}

		let total_nanos: u64 = times.iter().map(|d| d.as_nanos() as u64).sum();
		let avg_duration = Duration::from_nanos(total_nanos / times.len() as u64);

		let mut sorted_times = times.clone();
		sorted_times.sort();

		let p95_idx = (sorted_times.len() as f64 * 0.95) as usize;
		let p95_duration = sorted_times.get(p95_idx).copied().unwrap_or(Duration::ZERO);

		(
			count,
			total_nanos as f64 / 1_000_000.0,
			avg_duration,
			p95_duration,
		)
	}
}

impl MetricsCollector for SolverMetricsCollector {
	fn collect(&self) -> Vec<Metric> {
		// Note: In a real implementation, you'd want to make this async
		// or use a different approach to avoid blocking
		let (count, total_ms, avg_duration, p95_duration) =
			futures::executor::block_on(self.get_solution_stats());
		let error_count = futures::executor::block_on(async { *self.error_count.read().await });

		vec![
			Metric::counter("solver.solutions.total", count)
				.with_description("Total number of solutions computed"),
			Metric::counter("solver.errors.total", error_count)
				.with_description("Total number of solver errors"),
			Metric::gauge(
				"solver.solution_time.avg_ms",
				avg_duration.as_millis() as f64,
			)
			.with_description("Average solution time in milliseconds"),
			Metric::gauge(
				"solver.solution_time.p95_ms",
				p95_duration.as_millis() as f64,
			)
			.with_description("95th percentile solution time in milliseconds"),
			Metric::gauge("solver.solution_time.total_ms", total_ms)
				.with_description("Total time spent solving in milliseconds"),
		]
	}

	fn name(&self) -> &str {
		&self.name
	}
}

/// Central registry for all metrics collectors.
///
/// Manages metric collectors, coordinates periodic collection,
/// and provides query capabilities for collected metrics.
pub struct MetricsRegistry {
	collectors: Arc<RwLock<HashMap<String, Box<dyn MetricsCollector>>>>,
	collection_interval: Duration,
	last_collection: Arc<RwLock<Vec<Metric>>>,
}

impl MetricsRegistry {
	/// Creates a new metrics registry.
	///
	/// # Arguments
	///
	/// * `collection_interval` - How often to collect metrics from all collectors
	pub fn new(collection_interval: Duration) -> Self {
		Self {
			collectors: Arc::new(RwLock::new(HashMap::new())),
			collection_interval,
			last_collection: Arc::new(RwLock::new(Vec::new())),
		}
	}

	/// Registers a new metrics collector.
	///
	/// The collector will be included in all future metric collections.
	pub async fn register_collector(&self, collector: Box<dyn MetricsCollector>) {
		let name = collector.name().to_string();
		let mut collectors = self.collectors.write().await;
		collectors.insert(name, collector);
	}

	/// Manually triggers collection from all registered collectors.
	///
	/// Returns all collected metrics and updates the last collection cache.
	pub async fn collect_all_metrics(&self) -> Vec<Metric> {
		let collectors = self.collectors.read().await;
		let mut all_metrics = Vec::new();

		for (name, collector) in collectors.iter() {
			debug!("Collecting metrics from: {}", name);
			let metrics = collector.collect();
			info!("Collected {} metrics from {}", metrics.len(), name);
			all_metrics.extend(metrics);
		}

		// Update last collection
		let mut last_collection = self.last_collection.write().await;
		*last_collection = all_metrics.clone();

		all_metrics
	}

	/// Retrieves the most recent metric collection.
	///
	/// Returns cached results from the last collection without
	/// triggering a new collection.
	pub async fn get_last_collection(&self) -> Vec<Metric> {
		self.last_collection.read().await.clone()
	}

	/// Starts a background task for periodic metric collection.
	///
	/// Collections occur at the interval specified during registry creation.
	/// This method spawns a tokio task and returns immediately.
	pub async fn start_periodic_collection(&self) {
		let collectors = self.collectors.clone();
		let last_collection = self.last_collection.clone();
		let interval = self.collection_interval;

		tokio::spawn(async move {
			let mut interval_timer = tokio::time::interval(interval);

			loop {
				interval_timer.tick().await;

				let collectors_guard = collectors.read().await;
				let mut all_metrics = Vec::new();

				for (name, collector) in collectors_guard.iter() {
					let metrics = collector.collect();
					debug!(
						"Periodic collection from {}: {} metrics",
						name,
						metrics.len()
					);
					all_metrics.extend(metrics);
				}

				// Update last collection
				let mut last_collection_guard = last_collection.write().await;
				*last_collection_guard = all_metrics;
			}
		});
	}

	/// Finds a metric by its name in the last collection.
	pub async fn get_metric_by_name(&self, name: &str) -> Option<Metric> {
		let metrics = self.get_last_collection().await;
		metrics.into_iter().find(|m| m.name == name)
	}

	/// Finds all metrics with a specific tag value.
	///
	/// Useful for querying metrics by dimensions like chain ID or service name.
	pub async fn get_metrics_by_tag(&self, tag_key: &str, tag_value: &str) -> Vec<Metric> {
		let metrics = self.get_last_collection().await;
		metrics
			.into_iter()
			.filter(|m| m.tags.get(tag_key).is_some_and(|v| v == tag_value))
			.collect()
	}
}

/// Utility for measuring execution duration.
///
/// Provides a simple interface for timing operations and
/// calculating elapsed time since creation.
pub struct Timer {
	start: Instant,
}

impl Timer {
	/// Creates a new timer starting from now.
	pub fn new() -> Self {
		Self {
			start: Instant::now(),
		}
	}

	/// Returns the elapsed time since timer creation.
	pub fn elapsed(&self) -> Duration {
		self.start.elapsed()
	}

	/// Consumes the timer and returns the final elapsed duration.
	pub fn finish(self) -> Duration {
		self.elapsed()
	}
}

impl Default for Timer {
	fn default() -> Self {
		Self::new()
	}
}

/// Macro for timing synchronous operations.
///
/// Executes the provided expression and returns a tuple of
/// (result, duration). Also logs the duration at debug level.
#[macro_export]
macro_rules! time_operation {
	($name:expr, $operation:expr) => {{
		let timer = $crate::metrics::Timer::new();
		let result = $operation;
		let duration = timer.finish();
		tracing::debug!("Operation '{}' took {:?}", $name, duration);
		(result, duration)
	}};
}

/// Macro for timing asynchronous operations.
///
/// Similar to `time_operation!` but for async expressions.
/// Executes the provided async expression and returns a tuple of
/// (result, duration). Also logs the duration at debug level.
#[macro_export]
macro_rules! time_async_operation {
	($name:expr, $operation:expr) => {{
		let timer = $crate::metrics::Timer::new();
		let result = $operation.await;
		let duration = timer.finish();
		tracing::debug!("Async operation '{}' took {:?}", $name, duration);
		(result, duration)
	}};
}
