//! Concrete implementations of solver interfaces.
//!
//! This module contains the actual implementations of all the traits
//! defined in the various solver crates. Each submodule provides
//! implementations for a specific aspect of the solver functionality.

/// Account provider implementations (e.g., local keys, AWS KMS).
pub mod account;
/// Transaction delivery implementations (e.g., HTTP RPC providers).
pub mod delivery;
/// Intent discovery implementations (e.g., on-chain event monitoring).
pub mod discovery;
/// Order standard implementations (e.g., EIP-7683).
pub mod order;
/// Settlement mechanism implementations (e.g., direct settlement).
pub mod settlement;
/// Storage backend implementations (e.g., in-memory, Redis).
pub mod storage;
/// Execution strategy implementations (e.g., profit-based strategies).
pub mod strategy;
