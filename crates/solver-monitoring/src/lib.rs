//! Monitoring and observability for the OIF solver.
//!
//! This crate provides comprehensive monitoring capabilities including metrics
//! collection, health checks, and distributed tracing. It enables operators
//! to monitor solver performance, track key metrics, and diagnose issues.
//!
//! # Components
//!
//! - `health`: Health check endpoints and system status monitoring
//! - `metrics`: Performance metrics collection and aggregation
//! - `tracing`: Distributed tracing for request flow analysis
//!
//! # Metrics Categories
//!
//! The monitoring system tracks various metric types:
//! - Order processing metrics (success rate, processing time)
//! - Transaction metrics (submission rate, confirmation time)
//! - System metrics (resource usage, queue depths)
//! - Business metrics (volume, fees collected)

pub mod health;
pub mod metrics;
pub mod tracing;
