//! OIF Solver service executable.
//!
//! This is the main entry point for running the OIF solver as a service.
//! It handles initialization, configuration loading, and service lifecycle
//! management.
//!
//! # Usage
//!
//! The solver can be run with various command-line options to control
//! configuration, logging, and operational parameters.

use anyhow::Result;
use clap::Parser;
use solver_config::load_config;
use solver_monitoring::tracing::{init_tracing, TracingConfig};
use tracing::{error, info, Level};

mod api;
mod cli;
mod service;

use service::SolverService;

/// Main entry point for the solver service.
///
/// Initializes all components, loads configuration, and starts the service
/// with proper error handling and graceful shutdown support.
#[tokio::main]
async fn main() -> Result<()> {
	// Parse CLI arguments
	let args = cli::Args::parse();

	// Initialize basic logging first (will be enhanced later in service initialization)
	let log_level = args.log_level.as_deref().unwrap_or("info");
	let level = match log_level {
		"trace" => Level::TRACE,
		"debug" => Level::DEBUG,
		"info" => Level::INFO,
		"warn" => Level::WARN,
		"error" => Level::ERROR,
		_ => Level::INFO,
	};

	let tracing_config = TracingConfig::new()
		.with_level(level)
		.with_json_format(false); // CLI always uses pretty format

	init_tracing(tracing_config)
		.map_err(|e| anyhow::anyhow!("Failed to initialize tracing: {}", e))?;

	info!("Starting OIF Solver Service");

	// Handle different commands
	match args.command {
		Some(cli::Command::Status) => {
			handle_status_command(args).await?;
		}
		Some(cli::Command::Health) => {
			handle_health_command(args).await?;
		}
		Some(cli::Command::Metrics) => {
			handle_metrics_command(args).await?;
		}
		Some(cli::Command::Validate { config }) => {
			handle_validate_command(config).await?;
		}
		Some(cli::Command::GenerateConfig { output }) => {
			handle_generate_config_command(output).await?;
		}
		None => {
			// Run the solver
			handle_run_command(args).await?;
		}
	}

	Ok(())
}

async fn handle_status_command(args: cli::Args) -> Result<()> {
	info!("Fetching solver status...");

	// Load configuration
	let config = match args.config {
		Some(path) => solver_config::ConfigLoader::from_file(path)?,
		None => load_config()?,
	};

	// Create service and get status
	let service = SolverService::new(config).await?;
	let status = service.status().await;

	println!("📊 Solver Status Report");
	println!("=======================");
	println!();

	// Basic status
	println!(
		"🔧 Service: {}",
		if status.is_running {
			"✅ Running"
		} else {
			"❌ Stopped"
		}
	);
	println!("🏷️  Name: {}", status.config_summary.solver_name);
	println!(
		"🔗 Chains: {}",
		status.config_summary.monitored_chains.len()
	);
	println!("💾 Storage: {}", status.config_summary.storage_backend);
	println!("⚖️  Settlement: {}", status.config_summary.settlement_type);
	println!();

	// Engine stats
	println!("🚀 Engine Statistics");
	println!(
		"   Running: {}",
		if status.engine_stats.is_running {
			"✅"
		} else {
			"❌"
		}
	);
	println!(
		"   Processed Orders: {}",
		status.engine_stats.processed_orders
	);
	println!("   Failed Orders: {}", status.engine_stats.failed_orders);
	println!();

	// Health summary
	println!(
		"🏥 Health Status: {}",
		format_health_status(&status.health.overall_status)
	);
	for check in &status.health.checks {
		println!(
			"   {} {}: {} ({}ms)",
			format_health_status(&check.status),
			check.name,
			check.message,
			check.duration_ms
		);
	}
	println!();

	// Metrics summary
	println!("📈 Metrics Summary");
	println!("   Total Metrics: {}", status.metrics_summary.total_metrics);
	println!(
		"   Solutions: {}",
		status.metrics_summary.solver_metrics.total_solutions
	);
	println!(
		"   Errors: {}",
		status.metrics_summary.solver_metrics.total_errors
	);
	println!(
		"   Avg Solution Time: {:.2}ms",
		status.metrics_summary.solver_metrics.avg_solution_time_ms
	);
	println!(
		"   Memory Usage: {:.2}MB",
		status.metrics_summary.system_metrics.memory_usage_mb
	);
	println!(
		"   CPU Usage: {:.2}%",
		status.metrics_summary.system_metrics.cpu_usage_percent
	);
	println!();

	// Tracing stats
	println!("🔍 Tracing Statistics");
	println!("   Active Spans: {}", status.tracing_stats.active_count);
	println!(
		"   Completed Spans: {}",
		status.tracing_stats.completed_count
	);
	println!("   Avg Duration: {:?}", status.tracing_stats.avg_duration);
	println!("   P95 Duration: {:?}", status.tracing_stats.p95_duration);

	if args.verbose {
		println!();
		println!("🔧 Raw JSON Output:");
		println!("{}", serde_json::to_string_pretty(&status)?);
	}

	Ok(())
}

async fn handle_health_command(args: cli::Args) -> Result<()> {
	info!("Checking solver health...");

	// Load configuration
	let config = match args.config {
		Some(path) => solver_config::ConfigLoader::from_file(path)?,
		None => load_config()?,
	};

	// Create service and get health
	let service = SolverService::new(config).await?;
	let status = service.status().await;

	println!("🏥 Health Check Report");
	println!("======================");
	println!();

	println!(
		"Overall Status: {}",
		format_health_status(&status.health.overall_status)
	);
	println!();

	for check in &status.health.checks {
		println!("📋 {}", check.name);
		println!("   Status: {}", format_health_status(&check.status));
		println!("   Message: {}", check.message);
		println!("   Duration: {}ms", check.duration_ms);
		println!();
	}

	// Exit with appropriate code
	use solver_monitoring::health::HealthStatus;
	match status.health.overall_status {
		HealthStatus::Healthy => std::process::exit(0),
		HealthStatus::Degraded => {
			eprintln!("⚠️  Service is degraded");
			std::process::exit(1);
		}
		HealthStatus::Unhealthy => {
			eprintln!("❌ Service is unhealthy");
			std::process::exit(2);
		}
	}
}

async fn handle_metrics_command(args: cli::Args) -> Result<()> {
	info!("Collecting solver metrics...");

	// Load configuration
	let config = match args.config {
		Some(path) => solver_config::ConfigLoader::from_file(path)?,
		None => load_config()?,
	};

	// Create service and get metrics
	let service = SolverService::new(config).await?;
	let status = service.status().await;

	println!("📊 Metrics Report");
	println!("=================");
	println!();

	println!("🎯 Solver Metrics");
	println!(
		"   Total Solutions: {}",
		status.metrics_summary.solver_metrics.total_solutions
	);
	println!(
		"   Total Errors: {}",
		status.metrics_summary.solver_metrics.total_errors
	);
	println!(
		"   Average Solution Time: {:.2}ms",
		status.metrics_summary.solver_metrics.avg_solution_time_ms
	);

	let total_ops = status.metrics_summary.solver_metrics.total_solutions
		+ status.metrics_summary.solver_metrics.total_errors;
	if total_ops > 0 {
		let success_rate = (status.metrics_summary.solver_metrics.total_solutions as f64
			/ total_ops as f64)
			* 100.0;
		println!("   Success Rate: {:.2}%", success_rate);
	}
	println!();

	println!("🖥️  System Metrics");
	println!(
		"   Memory Usage: {:.2}MB",
		status.metrics_summary.system_metrics.memory_usage_mb
	);
	println!(
		"   CPU Usage: {:.2}%",
		status.metrics_summary.system_metrics.cpu_usage_percent
	);
	println!();

	println!("📈 Performance Stats");
	println!("   Active Spans: {}", status.tracing_stats.active_count);
	println!(
		"   Completed Operations: {}",
		status.tracing_stats.completed_count
	);
	println!(
		"   Average Operation Time: {:?}",
		status.tracing_stats.avg_duration
	);
	println!(
		"   P50 Operation Time: {:?}",
		status.tracing_stats.p50_duration
	);
	println!(
		"   P95 Operation Time: {:?}",
		status.tracing_stats.p95_duration
	);
	println!(
		"   P99 Operation Time: {:?}",
		status.tracing_stats.p99_duration
	);

	if args.verbose {
		println!();
		println!("📊 Detailed Metrics (JSON):");
		println!("{}", serde_json::to_string_pretty(&status.metrics_summary)?);
	}

	Ok(())
}

async fn handle_validate_command(config_path: std::path::PathBuf) -> Result<()> {
	info!("Validating configuration file: {:?}", config_path);

	match solver_config::ConfigLoader::from_file(config_path.clone()) {
		Ok(config) => {
			println!("✅ Configuration is valid!");
			println!();
			println!("📋 Configuration Summary:");
			println!("   Solver Name: {}", config.solver.name);
			println!("   Monitor Chains: {:?}", config.discovery.monitor_chains);
			println!("   Chains Configured: {}", config.chains.len());
			println!(
				"   Discovery Chains: {}",
				config.discovery.monitor_chains.len()
			);
			println!("   Storage Backend: {}", config.state.storage_backend);
			println!("   Settlement Type: {}", config.settlement.default_type);
			println!("   Monitoring Enabled: {}", config.monitoring.enabled);

			if config.monitoring.enabled {
				println!("   Health Port: {}", config.monitoring.health_port);
			}
		}
		Err(e) => {
			eprintln!("❌ Configuration validation failed:");
			eprintln!("   File: {:?}", config_path);
			eprintln!("   Error: {}", e);
			std::process::exit(1);
		}
	}

	Ok(())
}

async fn handle_generate_config_command(output_path: std::path::PathBuf) -> Result<()> {
	info!("Generating example configuration: {:?}", output_path);

	let config = solver_config::SolverConfig::default();
	let toml_content = toml::to_string_pretty(&config)?;
	std::fs::write(&output_path, toml_content)?;

	println!("✅ Example configuration written to {:?}", output_path);
	println!();
	println!("📝 Next steps:");
	println!("   1. Edit the configuration file to match your setup");
	println!(
		"   2. Validate it: oif-solver validate --config {:?}",
		output_path
	);
	println!(
		"   3. Run the solver: oif-solver --config {:?}",
		output_path
	);

	Ok(())
}

async fn handle_run_command(args: cli::Args) -> Result<()> {
	// Load configuration
	let config = match args.config {
		Some(path) => solver_config::ConfigLoader::from_file(path)?,
		None => load_config()?,
	};

	info!("Configuration loaded successfully");
	info!("Starting solver: {}", config.solver.name);

	// Create and start the solver service with full monitoring
	let service = SolverService::new(config).await?;

	if let Err(e) = service.run().await {
		error!("Solver service error: {}", e);
		return Err(e);
	}

	Ok(())
}

fn format_health_status(status: &solver_monitoring::health::HealthStatus) -> &'static str {
	match status {
		solver_monitoring::health::HealthStatus::Healthy => "✅ Healthy",
		solver_monitoring::health::HealthStatus::Degraded => "⚠️  Degraded",
		solver_monitoring::health::HealthStatus::Unhealthy => "❌ Unhealthy",
	}
}
