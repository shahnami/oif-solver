//! Main service implementation for the OIF solver.
//!
//! This crate provides the core service infrastructure including the HTTP API,
//! command-line interface, and main service orchestration. It serves as the
//! entry point for running the solver as a standalone service.
//!
//! # Components
//!
//! - `api`: RESTful API endpoints for solver interaction
//! - `cli`: Command-line interface and configuration
//! - `service`: Core service implementation and lifecycle management
//!
//! # Service Architecture
//!
//! The service coordinates all solver components:
//! - Order discovery and monitoring
//! - Solution computation
//! - Transaction execution
//! - Result reporting

pub mod api;
pub mod cli;
pub mod service;
