//! Order and solution validation framework.
//!
//! This crate provides a comprehensive validation pipeline for orders and
//! solutions before execution. It ensures that only valid, profitable, and
//! safe orders are processed by the solver.
//!
//! # Validation Pipeline
//!
//! The validation system performs multiple checks:
//! - Order validity and signature verification
//! - Liquidity availability across chains
//! - Profitability threshold enforcement
//! - Risk assessment and limits
//! - Slippage and price impact analysis
//!
//! # Validator Types
//!
//! - `order_validity`: Basic order validation rules
//! - `liquidity_check`: Ensures sufficient liquidity exists
//! - `profitability_threshold`: Minimum profit requirements
//! - `risk_assessment`: Risk scoring and limits
//! - `pipeline`: Orchestrates validation sequence
