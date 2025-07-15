//! Command-line interface definitions with monitoring commands.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "oif-solver")]
#[command(about = "OIF Protocol Solver with Monitoring", long_about = None)]
#[command(version = env!("CARGO_PKG_VERSION"))]
pub struct Args {
	/// Path to configuration file
	#[arg(short, long, env = "CONFIG_FILE")]
	pub config: Option<PathBuf>,

	/// Log level override (trace, debug, info, warn, error)
	#[arg(short, long, env = "LOG_LEVEL")]
	pub log_level: Option<String>,

	/// Enable verbose output
	#[arg(short, long)]
	pub verbose: bool,

	/// Subcommand to execute
	#[command(subcommand)]
	pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
	/// Show comprehensive solver status including monitoring data
	#[command(about = "Display detailed solver status with health, metrics, and performance data")]
	Status,

	/// Check solver health status
	#[command(about = "Run health checks and display results")]
	Health,

	/// Display solver metrics and performance statistics
	#[command(about = "Show current metrics and performance statistics")]
	Metrics,

	/// Validate configuration file
	#[command(about = "Validate a solver configuration file")]
	Validate {
		/// Path to configuration file to validate
		#[arg(help = "Configuration file to validate")]
		config: PathBuf,
	},

	/// Generate example configuration
	#[command(about = "Generate an example configuration file")]
	GenerateConfig {
		/// Output file path
		#[arg(short, long, default_value = "config.toml")]
		#[arg(help = "Output path for the generated configuration")]
		output: PathBuf,
	},
}
