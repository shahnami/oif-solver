//! Solver strategy implementations for order execution.
//!
//! This crate provides various strategies for solving cross-chain orders,
//! including route optimization, execution planning, and fallback mechanisms.
//! It contains the core logic for finding optimal paths to fulfill orders
//! while minimizing costs and maximizing efficiency.
//!
//! # Strategy Components
//!
//! The strategy system is designed to be modular and extensible:
//! - **Execution Planning**: Determines the sequence of operations needed to fulfill an order
//! - **Route Optimization**: Finds the most efficient path across chains and liquidity sources
//! - **Cost Minimization**: Optimizes for gas costs, slippage, and fees
//! - **Fallback Strategies**: Provides alternative execution paths when primary strategies fail
//!
//! # Strategy Selection
//!
//! The solver selects strategies based on multiple factors:
//! - Order characteristics (size, chains, tokens)
//! - Market conditions (liquidity, gas prices)
//! - Historical performance data
//! - Risk parameters and constraints
//!
//! # Implementation Design
//!
//! Strategies implement a common interface allowing:
//! - Dynamic strategy selection at runtime
//! - Parallel strategy evaluation
//! - Performance metrics collection
//! - Graceful degradation through fallbacks
//!
//! # Future Extensions
//!
//! This crate is designed to support:
//! - Machine learning-based route optimization
//! - Multi-objective optimization (speed vs cost)
//! - Cross-protocol arbitrage strategies
//! - MEV-aware execution paths
