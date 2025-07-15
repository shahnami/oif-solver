//! Chain adapters for connecting to different blockchains.
//!
//! This crate provides a unified interface for interacting with various blockchain
//! networks through the `ChainAdapter` trait. It abstracts away the differences
//! between blockchain implementations, allowing the solver to work with multiple
//! chains using a consistent API.
//!
//! # Architecture
//!
//! The crate is organized into several modules:
//!
//! - `registry`: Manages a collection of chain adapters, allowing dynamic registration
//!   and retrieval of adapters by chain ID
//! - `utils`: Provides utility types and functions, including retry logic for network calls
//! - `implementations`: Contains concrete adapter implementations for different blockchain types

pub mod registry;
pub mod utils;

pub mod implementations;

pub use registry::ChainRegistry;

// Re-export adapters based on features
pub use implementations::evm::EthersAdapter;
