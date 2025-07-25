//! Main entry point for the OIF solver service.
//!
//! This binary provides a complete solver implementation that discovers,
//! validates, executes, and settles cross-chain orders. It uses a modular
//! architecture with pluggable implementations for different components.

use clap::Parser;
use solver_config::Config;
use solver_core::{SolverBuilder, SolverEngine};
use std::path::PathBuf;
use tracing::Level;

// Import implementations from individual crates
use solver_account::implementations::local::create_account;
use solver_delivery::implementations::evm::alloy::create_http_delivery;
use solver_discovery::implementations::onchain::_7683::create_discovery;
use solver_order::implementations::{
	standards::_7683::create_order_impl, strategies::simple::create_strategy,
};
use solver_settlement::implementations::direct::create_settlement;
use solver_storage::implementations::file::create_storage;

/// Command-line arguments for the solver service.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
	/// Path to configuration file
	#[arg(short, long, default_value = "config.toml")]
	config: PathBuf,

	/// Log level (trace, debug, info, warn, error)
	#[arg(short, long, default_value = "info")]
	log_level: String,
}

/// Main entry point for the solver service.
///
/// This function:
/// 1. Parses command-line arguments
/// 2. Initializes logging infrastructure
/// 3. Loads configuration from file
/// 4. Builds the solver engine with all implementations
/// 5. Runs the solver until interrupted
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::parse();

	// Initialize tracing
	let log_level = match args.log_level.to_lowercase().as_str() {
		"trace" => Level::TRACE,
		"debug" => Level::DEBUG,
		"info" => Level::INFO,
		"warn" => Level::WARN,
		"error" => Level::ERROR,
		_ => Level::INFO,
	};

	tracing_subscriber::fmt()
		.with_max_level(log_level)
		.with_thread_ids(true)
		.with_target(true)
		.init();

	tracing::info!("Started solver");

	// Load configuration
	let config = Config::from_file(args.config.to_str().unwrap())?;
	tracing::info!("Loaded configuration [{}]", config.solver.id);

	// Build solver engine with implementations
	let solver = build_solver(config)?;
	tracing::info!("Loaded solver engine");

	// Run the solver
	solver.run().await?;

	tracing::info!("Stopped solver");
	Ok(())
}

/// Builds the solver engine with all necessary implementations.
///
/// This function wires up all the concrete implementations for:
/// - Storage backends (e.g., in-memory, Redis)
/// - Account providers (e.g., local keys, AWS KMS)
/// - Delivery mechanisms (e.g., HTTP RPC, WebSocket)
/// - Discovery sources (e.g., on-chain events, off-chain APIs)
/// - Order implementations (e.g., EIP-7683)
/// - Settlement mechanisms (e.g., direct settlement)
/// - Execution strategies (e.g., always execute, limit orders)
fn build_solver(config: Config) -> Result<SolverEngine, Box<dyn std::error::Error>> {
	let builder = SolverBuilder::new(config)
        // Storage implementations
        .with_storage_factory(create_storage)
        // Account implementations
        .with_account_factory(create_account)
        // Delivery implementations
        .with_delivery_factory("origin", create_http_delivery)
        .with_delivery_factory("destination", create_http_delivery)

        // Discovery implementations
        .with_discovery_factory("origin_eip7683", create_discovery)
        .with_discovery_factory("destination_eip7683", create_discovery)
        // Order implementations
        .with_order_factory("eip7683", create_order_impl)
        // Settlement implementations
        .with_settlement_factory("eip7683", create_settlement)
        // Strategy implementation
        .with_strategy_factory(create_strategy);

	Ok(builder.build()?)
}
