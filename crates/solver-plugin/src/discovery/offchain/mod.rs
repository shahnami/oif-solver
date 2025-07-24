//! # Off-chain Discovery Implementations
//!
//! Provides discovery plugins for off-chain data sources.
//!
//! This module contains implementations for discovering orders and events
//! from off-chain sources such as APIs, webhooks, message queues, and
//! other external data providers.

pub mod webhook;
pub mod api_poller;

pub use webhook::{WebhookConfig, WebhookDiscoveryPlugin};
pub use api_poller::{ApiPollerConfig, ApiPollerDiscoveryPlugin};
