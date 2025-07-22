//! # Solver Types
//!
//! Core type definitions shared across all solver components.
//!
//! This crate provides the fundamental data structures, enums, and traits
//! that define the interfaces and data models used throughout the solver
//! system. It includes configuration types, event definitions, and plugin
//! interfaces that enable the modular architecture of the solver.
//!
//! ## Modules
//!
//! - **configs**: Configuration structures for all solver components
//! - **events**: Event types for the event-driven architecture
//! - **plugins**: Plugin interfaces and base traits for extensibility

pub mod configs;
pub mod events;
pub mod plugins;

pub use configs::*;
pub use events::*;
pub use plugins::*;
