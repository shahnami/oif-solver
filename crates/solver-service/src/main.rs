use clap::Parser;
use log::info;
use solver_config::Config;
use solver_core::{SolverBuilder, SolverEngine};
use std::path::PathBuf;

mod implementations;

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
	let args = Args::parse();

	// Initialize logger
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&args.log_level))
		.init();

	info!("Starting OIF Solver Service");

	// Load configuration
	let config = Config::from_file(args.config.to_str().unwrap())?;
	info!("Loaded configuration for solver: {}", config.solver.id);

	// Build solver engine with implementations
	let solver = build_solver(config)?;

	// Run the solver
	info!("Starting solver engine");
	solver.run().await?;

	info!("Solver service stopped");
	Ok(())
}

fn build_solver(config: Config) -> Result<SolverEngine, Box<dyn std::error::Error>> {
	let builder = SolverBuilder::new(config)
        // Storage implementations
        .with_storage_factory(implementations::storage::create_storage)
        // Account implementations
        .with_account_factory(implementations::account::create_account)
        // Delivery implementations
        .with_delivery_factory("origin", implementations::delivery::create_http_delivery)
        .with_delivery_factory("destination", implementations::delivery::create_http_delivery)

        // Discovery implementations
        .with_discovery_factory("origin_eip7683", implementations::discovery::create_discovery)
        // Order implementations
        .with_order_factory("eip7683", implementations::order::create_order_impl)
        // Settlement implementations
        .with_settlement_factory("direct", implementations::settlement::create_settlement)
        // Strategy implementation
        .with_strategy_factory(implementations::strategy::create_strategy);

	Ok(builder.build()?)
}
