use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, span, Level, Span};
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt};

/// Tracing configuration
#[derive(Debug, Clone)]
pub struct TracingConfig {
	pub level: Level,
	pub with_thread_ids: bool,
	pub with_thread_names: bool,
	pub with_file_and_line: bool,
	pub with_target: bool,
	pub with_span_events: FmtSpan,
	pub json_format: bool,
	pub include_solver_spans: bool,
}

impl Default for TracingConfig {
	fn default() -> Self {
		Self {
			level: Level::INFO,
			with_thread_ids: true,
			with_thread_names: true,
			with_file_and_line: true,
			with_target: true,
			with_span_events: FmtSpan::ENTER | FmtSpan::CLOSE,
			json_format: false,
			include_solver_spans: true,
		}
	}
}

impl TracingConfig {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn with_level(mut self, level: Level) -> Self {
		self.level = level;
		self
	}

	pub fn with_json_format(mut self, json: bool) -> Self {
		self.json_format = json;
		self
	}

	pub fn with_solver_spans(mut self, include: bool) -> Self {
		self.include_solver_spans = include;
		self
	}

	pub fn debug() -> Self {
		Self::default().with_level(Level::DEBUG)
	}

	pub fn production() -> Self {
		Self {
			level: Level::INFO,
			with_thread_ids: false,
			with_thread_names: false,
			with_file_and_line: false,
			with_target: false,
			with_span_events: FmtSpan::NONE,
			json_format: true,
			include_solver_spans: false,
		}
	}
}

/// Initialize tracing with the given configuration
pub fn init_tracing(config: TracingConfig) -> Result<(), Box<dyn std::error::Error>> {
	let subscriber = tracing_subscriber::registry();

	if config.json_format {
		let json_layer = tracing_subscriber::fmt::layer()
			.json()
			.with_span_events(config.with_span_events)
			.with_thread_ids(config.with_thread_ids)
			.with_thread_names(config.with_thread_names)
			.with_file(config.with_file_and_line)
			.with_line_number(config.with_file_and_line)
			.with_target(config.with_target);

		subscriber
			.with(tracing_subscriber::filter::LevelFilter::from_level(
				config.level,
			))
			.with(json_layer)
			.try_init()
			.map_err(|e| format!("Failed to initialize tracing: {}", e))?;
	} else {
		let fmt_layer = tracing_subscriber::fmt::layer()
			.pretty()
			.with_span_events(config.with_span_events)
			.with_thread_ids(config.with_thread_ids)
			.with_thread_names(config.with_thread_names)
			.with_file(config.with_file_and_line)
			.with_line_number(config.with_file_and_line)
			.with_target(config.with_target);

		subscriber
			.with(tracing_subscriber::filter::LevelFilter::from_level(
				config.level,
			))
			.with(fmt_layer)
			.try_init()
			.map_err(|e| format!("Failed to initialize tracing: {}", e))?;
	}

	info!("Tracing initialized with level: {:?}", config.level);
	Ok(())
}

/// Span metadata for tracking
#[derive(Debug, Clone)]
pub struct SpanMetadata {
	pub name: String,
	pub level: Level,
	pub target: String,
	pub start_time: Instant,
	pub duration: Option<Duration>,
	pub fields: HashMap<String, String>,
}

impl SpanMetadata {
	pub fn new(name: String, level: Level, target: String) -> Self {
		Self {
			name,
			level,
			target,
			start_time: Instant::now(),
			duration: None,
			fields: HashMap::new(),
		}
	}

	pub fn finish(&mut self) {
		self.duration = Some(self.start_time.elapsed());
	}

	pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
		self.fields.insert(key.into(), value.into());
		self
	}
}

/// Span tracker for monitoring span lifecycle
#[derive(Clone)]
pub struct SpanTracker {
	active_spans: Arc<RwLock<HashMap<String, SpanMetadata>>>,
	completed_spans: Arc<RwLock<Vec<SpanMetadata>>>,
	max_completed_spans: usize,
}

impl SpanTracker {
	pub fn new(max_completed_spans: usize) -> Self {
		Self {
			active_spans: Arc::new(RwLock::new(HashMap::new())),
			completed_spans: Arc::new(RwLock::new(Vec::new())),
			max_completed_spans,
		}
	}

	pub async fn start_span(
		&self,
		name: impl Into<String>,
		level: Level,
		target: impl Into<String>,
	) {
		let name = name.into();
		let target = target.into();
		let span_id = format!("{}:{}", target, name);
		let metadata = SpanMetadata::new(name, level, target);

		let mut active_spans = self.active_spans.write().await;
		active_spans.insert(span_id, metadata);
	}

	pub async fn finish_span(&self, name: impl Into<String>, target: impl Into<String>) {
		let name = name.into();
		let target = target.into();
		let span_id = format!("{}:{}", target, name);
		let mut active_spans = self.active_spans.write().await;

		if let Some(mut metadata) = active_spans.remove(&span_id) {
			metadata.finish();

			let mut completed_spans = self.completed_spans.write().await;
			completed_spans.push(metadata);

			// Keep only the most recent spans
			if completed_spans.len() > self.max_completed_spans {
				let drain_to = completed_spans.len() - self.max_completed_spans;
				completed_spans.drain(..drain_to);
			}
		}
	}

	pub async fn get_active_spans(&self) -> Vec<SpanMetadata> {
		self.active_spans.read().await.values().cloned().collect()
	}

	pub async fn get_completed_spans(&self) -> Vec<SpanMetadata> {
		self.completed_spans.read().await.clone()
	}

	pub async fn get_span_stats(&self) -> SpanStats {
		let active = self.active_spans.read().await;
		let completed = self.completed_spans.read().await;

		let mut stats = SpanStats::new();
		stats.active_count = active.len();
		stats.completed_count = completed.len();

		// Calculate statistics from completed spans
		let durations: Vec<Duration> = completed.iter().filter_map(|s| s.duration).collect();

		if !durations.is_empty() {
			let total_nanos: u64 = durations.iter().map(|d| d.as_nanos() as u64).sum();
			stats.avg_duration = Duration::from_nanos(total_nanos / durations.len() as u64);

			let mut sorted_durations = durations.clone();
			sorted_durations.sort();

			let p50_idx = (sorted_durations.len() as f64 * 0.5) as usize;
			let p95_idx = (sorted_durations.len() as f64 * 0.95) as usize;
			let p99_idx = (sorted_durations.len() as f64 * 0.99) as usize;

			stats.p50_duration = sorted_durations
				.get(p50_idx)
				.copied()
				.unwrap_or(Duration::ZERO);
			stats.p95_duration = sorted_durations
				.get(p95_idx)
				.copied()
				.unwrap_or(Duration::ZERO);
			stats.p99_duration = sorted_durations
				.get(p99_idx)
				.copied()
				.unwrap_or(Duration::ZERO);
		}

		stats
	}
}

/// Statistics about spans
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpanStats {
	pub active_count: usize,
	pub completed_count: usize,
	pub avg_duration: Duration,
	pub p50_duration: Duration,
	pub p95_duration: Duration,
	pub p99_duration: Duration,
}

impl SpanStats {
	pub fn new() -> Self {
		Self {
			active_count: 0,
			completed_count: 0,
			avg_duration: Duration::ZERO,
			p50_duration: Duration::ZERO,
			p95_duration: Duration::ZERO,
			p99_duration: Duration::ZERO,
		}
	}
}

impl Default for SpanStats {
	fn default() -> Self {
		Self::new()
	}
}

impl fmt::Display for SpanStats {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"SpanStats {{ active: {}, completed: {}, avg: {:?}, p50: {:?}, p95: {:?}, p99: {:?} }}",
			self.active_count,
			self.completed_count,
			self.avg_duration,
			self.p50_duration,
			self.p95_duration,
			self.p99_duration
		)
	}
}

/// Solver-specific tracing utilities
pub struct SolverTracing {
	span_tracker: SpanTracker,
	solver_id: String,
}

impl SolverTracing {
	pub fn new(solver_id: impl Into<String>) -> Self {
		Self {
			span_tracker: SpanTracker::new(1000),
			solver_id: solver_id.into(),
		}
	}

	pub fn solver_span(&self, operation: &str) -> Span {
		let span = span!(
			Level::INFO,
			"solver_operation",
			solver_id = %self.solver_id,
			operation = %operation
		);

		// Start tracking this span
		let tracker = self.span_tracker.clone();
		let op_name = operation.to_string();
		let solver_id = self.solver_id.clone();

		tokio::spawn(async move {
			tracker
				.start_span(op_name, Level::INFO, format!("solver:{}", solver_id))
				.await;
		});

		span
	}

	pub async fn trace_solver_operation<F, T>(&self, operation: &str, f: F) -> T
	where
		F: std::future::Future<Output = T>,
	{
		let span = self.solver_span(operation);
		let _guard = span.enter();

		let start = Instant::now();
		info!("Starting solver operation: {}", operation);

		let result = f.await;

		let duration = start.elapsed();
		info!(
			"Completed solver operation: {} in {:?}",
			operation, duration
		);

		// Finish tracking this span
		let tracker = self.span_tracker.clone();
		let op_name = operation.to_string();
		let solver_id = self.solver_id.clone();

		tokio::spawn(async move {
			tracker
				.finish_span(op_name, format!("solver:{}", solver_id))
				.await;
		});

		result
	}

	pub async fn get_solver_stats(&self) -> SpanStats {
		self.span_tracker.get_span_stats().await
	}
}

/// Macro for creating solver spans
#[macro_export]
macro_rules! solver_span {
	($solver_tracing:expr, $operation:expr) => {
		$solver_tracing.solver_span($operation)
	};
}

/// Macro for tracing solver operations
#[macro_export]
macro_rules! trace_solver_operation {
	($solver_tracing:expr, $operation:expr, $body:expr) => {
		$solver_tracing
			.trace_solver_operation($operation, async move { $body })
			.await
	};
}

/// Performance monitoring utilities
pub struct PerformanceMonitor {
	operation_times: Arc<RwLock<HashMap<String, Vec<Duration>>>>,
	max_samples: usize,
}

impl PerformanceMonitor {
	pub fn new(max_samples: usize) -> Self {
		Self {
			operation_times: Arc::new(RwLock::new(HashMap::new())),
			max_samples,
		}
	}

	pub async fn record_operation_time(&self, operation: impl Into<String>, duration: Duration) {
		let operation = operation.into();
		let mut times = self.operation_times.write().await;
		let operation_times = times.entry(operation.clone()).or_insert_with(Vec::new);

		operation_times.push(duration);

		// Keep only the most recent samples
		if operation_times.len() > self.max_samples {
			operation_times.drain(..operation_times.len() - self.max_samples);
		}

		debug!("Recorded operation '{}' time: {:?}", operation, duration);
	}

	pub async fn get_operation_stats(&self, operation: &str) -> Option<OperationStats> {
		let times = self.operation_times.read().await;
		let operation_times = times.get(operation)?;

		if operation_times.is_empty() {
			return None;
		}

		let total_nanos: u64 = operation_times.iter().map(|d| d.as_nanos() as u64).sum();
		let avg_duration = Duration::from_nanos(total_nanos / operation_times.len() as u64);

		let mut sorted_times = operation_times.clone();
		sorted_times.sort();

		let min_duration = sorted_times.first().copied().unwrap_or(Duration::ZERO);
		let max_duration = sorted_times.last().copied().unwrap_or(Duration::ZERO);

		Some(OperationStats {
			operation: operation.to_string(),
			sample_count: operation_times.len(),
			avg_duration,
			min_duration,
			max_duration,
		})
	}

	pub async fn get_all_stats(&self) -> Vec<OperationStats> {
		let times = self.operation_times.read().await;
		let mut stats = Vec::new();

		for operation in times.keys() {
			if let Some(stat) = self.get_operation_stats(operation).await {
				stats.push(stat);
			}
		}

		stats
	}
}

/// Statistics for a specific operation
#[derive(Debug, Clone)]
pub struct OperationStats {
	pub operation: String,
	pub sample_count: usize,
	pub avg_duration: Duration,
	pub min_duration: Duration,
	pub max_duration: Duration,
}

impl fmt::Display for OperationStats {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(
			f,
			"{}: {} samples, avg: {:?}, min: {:?}, max: {:?}",
			self.operation,
			self.sample_count,
			self.avg_duration,
			self.min_duration,
			self.max_duration
		)
	}
}
