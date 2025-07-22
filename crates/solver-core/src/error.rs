//! # Core Error Types
//!
//! Defines error types used throughout the solver-core crate.
//!
//! This module provides a comprehensive error type that covers all possible
//! error conditions that can occur during solver core operations, including
//! configuration errors, service initialization failures, event processing
//! issues, and plugin-related errors.

use thiserror::Error;

/// Core error type for all solver-core operations.
///
/// This enum encompasses all error types that can occur within the core
/// orchestrator and its associated operations. Each variant represents
/// a specific category of error with descriptive context.
#[derive(Error, Debug)]
pub enum CoreError {
	/// Configuration-related errors including invalid config values or missing settings
	#[error("Configuration error: {0}")]
	Configuration(String),

	/// Errors that occur during service initialization and startup
	#[error("Service initialization error: {0}")]
	ServiceInit(String),

	/// Errors in the event processing pipeline
	#[error("Event processing error: {0}")]
	EventProcessing(String),

	/// Lifecycle management errors during startup or shutdown
	#[error("Lifecycle error: {0}")]
	Lifecycle(String),

	/// State service errors including storage and retrieval failures
	#[error("State error: {0}")]
	State(String),

	/// Discovery service errors including plugin failures
	#[error("Discovery error: {0}")]
	Discovery(String),

	/// Delivery service errors including transaction execution failures
	#[error("Delivery error: {0}")]
	Delivery(String),

	/// Plugin-related errors that are forwarded from the plugin system
	#[error("Plugin error: {0}")]
	Plugin(#[from] solver_types::plugins::PluginError),

	/// Communication channel errors for internal messaging
	#[error("Channel error: {0}")]
	Channel(String),

	/// Data serialization and deserialization errors
	#[error("Serialization error: {0}")]
	Serialization(String),

	/// Errors that occur during graceful shutdown procedures
	#[error("Shutdown error: {0}")]
	Shutdown(String),

	/// Catch-all for unexpected or unclassified errors
	#[error("Unknown error: {0}")]
	Unknown(String),
}
