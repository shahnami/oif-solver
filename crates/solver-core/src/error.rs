// solver-core/src/error.rs

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoreError {
	#[error("Configuration error: {0}")]
	Configuration(String),

	#[error("Service initialization error: {0}")]
	ServiceInit(String),

	#[error("Event processing error: {0}")]
	EventProcessing(String),

	#[error("Lifecycle error: {0}")]
	Lifecycle(String),

	#[error("State error: {0}")]
	State(String),

	#[error("Discovery error: {0}")]
	Discovery(String),

	#[error("Delivery error: {0}")]
	Delivery(String),

	#[error("Plugin error: {0}")]
	Plugin(#[from] solver_types::plugins::PluginError),

	#[error("Channel error: {0}")]
	Channel(String),

	#[error("Serialization error: {0}")]
	Serialization(String),

	#[error("Shutdown error: {0}")]
	Shutdown(String),

	#[error("Unknown error: {0}")]
	Unknown(String),
}
