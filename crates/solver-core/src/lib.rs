//! Core solver engine and coordination logic.

pub mod coordinator;
pub mod engine;
// pub mod registry;

pub use coordinator::SolverCoordinator;
pub use engine::{EngineStats, SolverEngine};
// pub use registry::ComponentRegistry;
