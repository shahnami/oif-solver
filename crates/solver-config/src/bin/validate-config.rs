//! Configuration validation utility
//!
//! Usage: cargo run --bin validate-config config/example.toml

use std::env;
use std::process;

use solver_config::ConfigLoader;

fn main() {
	let args: Vec<String> = env::args().collect();

	if args.len() != 2 {
		eprintln!("Usage: {} <config-file>", args[0]);
		process::exit(1);
	}

	let config_path = &args[1];

	println!("Validating configuration file: {}", config_path);

	match ConfigLoader::from_file(config_path) {
		Ok(config) => {
			println!("✅ Configuration is valid!");
			println!("Solver name: {}", config.solver.name);
			println!("Chains configured: {}", config.chains.len());
			println!("Monitor chains: {:?}", config.discovery.monitor_chains);
			println!("Settlement type: {}", config.settlement.default_type);
			println!("Storage backend: {}", config.state.storage_backend);
			println!("Delivery service: {}", config.delivery.default_service);
		}
		Err(e) => {
			eprintln!("❌ Configuration validation failed:");
			eprintln!("{}", e);
			process::exit(1);
		}
	}
}
