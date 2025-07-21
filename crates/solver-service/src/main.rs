use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use solver_config::ConfigLoader;
use solver_core::OrchestratorBuilder;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod service;

#[derive(Parser)]
#[command(name = "solver-service")]
#[command(about = "OIF Solver Service", long_about = None)]
struct Cli {
	#[command(subcommand)]
	command: Option<Commands>,

	#[arg(short, long, value_name = "FILE", default_value = "config/local.toml")]
	config: PathBuf,

	#[arg(long, env = "SOLVER_LOG_LEVEL", default_value = "info")]
	log_level: String,
}

#[derive(Subcommand)]
enum Commands {
	/// Start the solver service
	Start,
	/// Validate the configuration file
	Validate,
}

#[tokio::main]
async fn main() -> Result<()> {
	let cli = Cli::parse();

	// Initialize tracing
	setup_tracing(&cli.log_level)?;

	// Handle commands
	match cli.command {
		Some(Commands::Start) | None => start_service(cli).await,
		Some(Commands::Validate) => validate_config(cli).await,
	}
}

async fn start_service(cli: Cli) -> Result<()> {
	info!("Starting OIF Solver Service");
	info!("Loading configuration from: {:?}", cli.config);

	// Load configuration
	let config = ConfigLoader::new()
		.with_file(&cli.config)
		.load()
		.await
		.context("Failed to load configuration")?;

	info!("Configuration loaded successfully");
	info!("Solver name: {}", config.solver.name);
	info!("HTTP port: {}", config.solver.http_port);
	info!("Metrics port: {}", config.solver.metrics_port);

	// Create orchestrator
	let orchestrator = OrchestratorBuilder::new()
		.with_config(config.clone())
		.build()
		.await
		.context("Failed to build orchestrator")?;

	let orchestrator = Arc::new(orchestrator);

	// Start the orchestrator
	orchestrator
		.start()
		.await
		.context("Failed to start orchestrator")?;

	// Create the service with orchestrator
	let service = service::SolverService::new(orchestrator.clone(), config.clone());

	// Start HTTP server
	let http_handle =
		tokio::spawn(async move { api::start_http_server(service, config.solver.http_port).await });

	// Start metrics server
	let metrics_port = config.solver.metrics_port;
	let metrics_handle = tokio::spawn(async move { api::start_metrics_server(metrics_port).await });

	// Setup graceful shutdown
	let shutdown_signal = setup_shutdown_signal();

	info!("OIF Solver Service started successfully");

	// Wait for shutdown signal
	shutdown_signal.await;

	info!("Shutdown signal received, stopping services...");

	// Shutdown orchestrator
	orchestrator
		.shutdown()
		.await
		.context("Failed to shutdown orchestrator")?;

	// Cancel the server tasks
	http_handle.abort();
	metrics_handle.abort();

	info!("OIF Solver Service stopped");
	Ok(())
}

async fn validate_config(cli: Cli) -> Result<()> {
	info!("Validating configuration file: {:?}", cli.config);

	// Try to load the configuration
	let config = ConfigLoader::new()
		.with_file(&cli.config)
		.load()
		.await
		.context("Failed to load configuration")?;

	info!("Configuration is valid");
	info!("Solver name: {}", config.solver.name);
	info!("Enabled plugins:");

	// Print enabled plugins
	for (name, plugin) in &config.plugins.discovery {
		if plugin.enabled {
			info!("  Discovery: {} ({})", name, plugin.plugin_type);
		}
	}

	for (name, plugin) in &config.plugins.delivery {
		if plugin.enabled {
			info!("  Delivery: {} ({})", name, plugin.plugin_type);
		}
	}

	for (name, plugin) in &config.plugins.state {
		if plugin.enabled {
			info!("  State: {} ({})", name, plugin.plugin_type);
		}
	}

	Ok(())
}

fn setup_tracing(log_level: &str) -> Result<()> {
	let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
		.unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

	tracing_subscriber::registry()
		.with(env_filter)
		.with(tracing_subscriber::fmt::layer())
		.init();

	Ok(())
}

async fn setup_shutdown_signal() {
	let ctrl_c = async {
		signal::ctrl_c()
			.await
			.expect("failed to install Ctrl+C handler");
	};

	#[cfg(unix)]
	let terminate = async {
		signal::unix::signal(signal::unix::SignalKind::terminate())
			.expect("failed to install signal handler")
			.recv()
			.await;
	};

	#[cfg(not(unix))]
	let terminate = std::future::pending::<()>();

	tokio::select! {
		_ = ctrl_c => {},
		_ = terminate => {},
	}
}
