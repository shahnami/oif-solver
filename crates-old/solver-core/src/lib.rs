//! # Solver Core
//!
//! The core orchestration engine for the OIF solver system.
//!
//! This crate provides the central orchestrator that coordinates between different solver plugins
//! for order discovery, processing, and settlement. It manages the lifecycle of solver operations
//! and provides event-driven communication between components.
//!
//! ## Main Components
//!
//! - **Orchestrator**: The main coordinator that manages plugin interactions and order processing
//! - **Lifecycle Management**: Handles the startup, operation, and shutdown phases of the solver
//! - **Event System**: Provides event-driven communication for order processing and status updates
//! - **Error Handling**: Centralized error types for core operations

pub mod engine;
pub mod error;
pub mod lifecycle;
pub mod utils;

pub use engine::{EventSender, HealthReport, Orchestrator, OrchestratorBuilder, OrderInfo};

// Re-export event types from solver-types
pub use error::CoreError;
pub use lifecycle::{LifecycleManager, LifecycleState};
pub use solver_types::{
	Event, FillEvent, FillStatus, OrderEvent, ServiceStatus, SettlementEvent, SettlementStatus,
	StatusEvent,
};
