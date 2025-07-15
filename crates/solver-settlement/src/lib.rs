//! Settlement mechanisms for claiming solver rewards.
//!
//! This crate handles the settlement process for solver rewards across different
//! chains and protocols. It manages attestations, reward claims, and settlement
//! strategies to ensure solvers are properly compensated for their work.
//!
//! # Components
//!
//! - `encoders`: Settlement transaction encoding for different protocols
//! - `implementations`: Chain-specific settlement strategies
//! - `manager`: Settlement orchestration and lifecycle management
//! - `types`: Common types and data structures
//!
//! # Settlement Flow
//!
//! 1. Solution execution generates attestations
//! 2. Attestations are collected and verified
//! 3. Settlement strategy submits claims to appropriate chains
//! 4. Rewards are distributed to solver addresses

pub mod encoders;
pub mod implementations;
pub mod manager;
pub mod types;

pub use implementations::SettlementStrategy;
pub use manager::{SettlementConfig, SettlementManager};
pub use types::{Attestation, SettlementData, SettlementError, SettlementStatus, SettlementType};
