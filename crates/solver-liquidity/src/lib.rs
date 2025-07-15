//! Liquidity management and routing for the OIF solver.
//!
//! This crate handles liquidity discovery, management, and routing across
//! different decentralized exchanges and liquidity sources. It provides
//! abstractions for finding optimal trade paths and managing liquidity
//! availability for order fulfillment.
//!
//! # Key Responsibilities
//!
//! - Liquidity source discovery and monitoring
//! - Trade route optimization
//! - Liquidity availability tracking
//! - Price feed aggregation
//! - Slippage estimation
//!
//! # Architecture
//!
//! The liquidity system integrates with various on-chain and off-chain
//! liquidity sources to provide comprehensive market coverage for the solver.

pub mod implementations;
