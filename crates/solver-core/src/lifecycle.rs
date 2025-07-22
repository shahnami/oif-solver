//! # Lifecycle Management
//!
//! Manages the lifecycle state of the orchestrator and provides coordination
//! for startup and shutdown operations.
//!
//! The lifecycle manager ensures that the orchestrator transitions through
//! states in a controlled manner and provides shutdown signaling to all
//! components that need to perform cleanup operations.

use crate::error::CoreError;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Represents the current lifecycle state of the orchestrator.
///
/// The orchestrator progresses through these states in a defined order,
/// with specific transitions allowed between states to ensure proper
/// initialization and shutdown procedures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
	/// Initial state before any initialization
	Uninitialized,
	/// Currently initializing services and components
	Initializing,
	/// Fully operational and processing requests
	Running,
	/// Shutdown has been requested and is in progress
	Stopping,
	/// Shutdown has completed successfully
	Stopped,
	/// An error occurred and the system is in a failed state
	Failed,
}

impl std::fmt::Display for LifecycleState {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Uninitialized => write!(f, "Uninitialized"),
			Self::Initializing => write!(f, "Initializing"),
			Self::Running => write!(f, "Running"),
			Self::Stopping => write!(f, "Stopping"),
			Self::Stopped => write!(f, "Stopped"),
			Self::Failed => write!(f, "Failed"),
		}
	}
}

/// Manages the lifecycle state and shutdown coordination for the orchestrator.
///
/// The lifecycle manager provides thread-safe state management and coordinates
/// shutdown procedures across all system components through broadcast signaling.
pub struct LifecycleManager {
	/// Current lifecycle state protected by read-write lock
	state: Arc<RwLock<LifecycleState>>,
	/// Broadcast channel for coordinating shutdown across components
	shutdown_tx: broadcast::Sender<()>,
}

impl LifecycleManager {
	/// Create a new lifecycle manager.
	///
	/// Initializes the manager in the `Uninitialized` state with a broadcast
	/// channel for shutdown coordination.
	pub fn new() -> Self {
		let (shutdown_tx, _) = broadcast::channel(16);

		Self {
			state: Arc::new(RwLock::new(LifecycleState::Uninitialized)),
			shutdown_tx,
		}
	}

	/// Get the current lifecycle state.
	///
	/// # Returns
	/// The current `LifecycleState`
	pub async fn get_state(&self) -> LifecycleState {
		*self.state.read().await
	}

	/// Set the lifecycle state with validation.
	///
	/// Validates that the state transition is valid according to the defined
	/// state machine rules before updating the state.
	///
	/// # Arguments
	/// * `new_state` - The target state to transition to
	///
	/// # Returns
	/// `Ok(())` if the transition is valid and successful
	///
	/// # Errors
	/// Returns `CoreError::Lifecycle` if the state transition is invalid
	pub async fn set_state(&self, new_state: LifecycleState) -> Result<(), CoreError> {
		let mut state = self.state.write().await;
		let old_state = *state;

		if !self.is_valid_transition(old_state, new_state) {
			return Err(CoreError::Lifecycle(format!(
				"Invalid state transition from {} to {}",
				old_state, new_state
			)));
		}

		*state = new_state;

		Ok(())
	}

	/// Transition to the initializing state.
	///
	/// Marks the beginning of the initialization process.
	///
	/// # Errors
	/// Returns `CoreError::Lifecycle` if transition is invalid
	pub async fn initialize(&self) -> Result<(), CoreError> {
		self.set_state(LifecycleState::Initializing).await?;
		Ok(())
	}

	/// Transition to the running state.
	///
	/// Indicates that initialization is complete and the system is operational.
	///
	/// # Errors
	/// Returns `CoreError::Lifecycle` if transition is invalid
	pub async fn start(&self) -> Result<(), CoreError> {
		self.set_state(LifecycleState::Running).await?;
		Ok(())
	}

	/// Initiate shutdown sequence.
	///
	/// Transitions to stopping state, broadcasts shutdown signal to all
	/// subscribers, then transitions to stopped state.
	///
	/// # Errors
	/// Returns `CoreError::Lifecycle` if state transitions are invalid
	pub async fn shutdown(&self) -> Result<(), CoreError> {
		self.set_state(LifecycleState::Stopping).await?;
		let _ = self.shutdown_tx.send(());
		self.set_state(LifecycleState::Stopped).await?;
		Ok(())
	}

	/// Subscribe to shutdown notifications.
	///
	/// Components can use this to receive shutdown signals and perform
	/// graceful cleanup operations.
	///
	/// # Returns
	/// A broadcast receiver for shutdown signals
	pub fn subscribe_shutdown(&self) -> broadcast::Receiver<()> {
		self.shutdown_tx.subscribe()
	}

	fn is_valid_transition(&self, from: LifecycleState, to: LifecycleState) -> bool {
		use LifecycleState::*;

		match (from, to) {
			(Uninitialized, Initializing) => true,
			(Initializing, Running) => true,
			(Initializing, Failed) => true,
			(Running, Stopping) => true,
			(Stopping, Stopped) => true,
			(_, Failed) => true, // Can fail from any state
			_ => false,
		}
	}

	pub async fn is_running(&self) -> bool {
		*self.state.read().await == LifecycleState::Running
	}

	pub async fn is_stopped(&self) -> bool {
		matches!(
			*self.state.read().await,
			LifecycleState::Stopped | LifecycleState::Failed
		)
	}
}

impl Default for LifecycleManager {
	fn default() -> Self {
		Self::new()
	}
}
