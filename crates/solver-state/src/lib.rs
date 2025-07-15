//! State management for the OIF solver.

pub mod implementations;
pub mod manager;
pub mod queue;
pub mod storage;
pub mod types;

pub use manager::{StateConfig, StateManager};
pub use queue::OrderQueue;
pub use storage::{Storage, StorageBackend};
pub use types::{OrderPriority, OrderState, StateError};
