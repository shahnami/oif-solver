// solver-core/src/lib.rs

pub mod engine;
pub mod error;
pub mod lifecycle;

pub use engine::{EventSender, HealthReport, Orchestrator, OrchestratorBuilder};

// Re-export event types from solver-types
pub use error::CoreError;
pub use lifecycle::{LifecycleManager, LifecycleState};
pub use solver_types::{
	Event, FillEvent, FillStatus, OrderEvent, ServiceStatus, SettlementEvent, SettlementStatus,
	StatusEvent,
};
