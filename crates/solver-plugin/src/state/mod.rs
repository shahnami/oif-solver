//! # State Plugin Implementations
//!
//! Provides concrete implementations of state storage plugins.
//!
//! This module contains implementations of the state plugin trait for various
//! storage backends including in-memory storage for development/testing and
//! file-based storage for persistent state management. Each implementation
//! provides the full StateStore interface with backend-specific optimizations.

pub mod file;
pub mod memory;

pub use file::{FileConfig, FileStatePlugin, FileStore};
pub use memory::{InMemoryConfig, InMemoryStatePlugin, InMemoryStore};
