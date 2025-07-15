//! Price oracle services for the OIF solver.
//!
//! This crate provides price discovery and oracle services for accurate
//! valuation of assets across different chains. It aggregates data from
//! multiple sources to provide reliable price feeds for solver operations.
//!
//! # Key Features
//!
//! - Multi-source price aggregation
//! - Cross-chain price consistency
//! - Real-time and historical price data
//! - Price manipulation detection
//! - Fallback mechanisms for reliability
//!
//! # Oracle Types
//!
//! The crate supports various oracle types:
//! - On-chain oracles (Chainlink, Band Protocol, etc.)
//! - DEX-based price discovery
//! - Off-chain data providers
//! - Custom price feeds

pub mod implementations;
