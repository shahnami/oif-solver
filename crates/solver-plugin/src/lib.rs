//! # Solver Plugin Implementations
//!
//! Concrete implementations of solver plugins for various protocols and services.
//!
//! This crate provides the actual plugin implementations that can be used with
//! the solver system, including delivery mechanisms, discovery sources, order
//! processors, settlement strategies, and state storage backends. These plugins
//! implement the traits defined in `solver-types` and are registered with the
//! plugin factory for dynamic loading.

pub mod delivery;
pub mod discovery;
pub mod factory;
pub mod order;
pub mod settlement;
pub mod state;
