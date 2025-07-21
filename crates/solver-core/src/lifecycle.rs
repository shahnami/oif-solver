// solver-core/src/lifecycle.rs

use crate::error::CoreError;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
	Uninitialized,
	Initializing,
	Running,
	Stopping,
	Stopped,
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

pub struct LifecycleManager {
	state: Arc<RwLock<LifecycleState>>,
	shutdown_tx: broadcast::Sender<()>,
}

impl LifecycleManager {
	pub fn new() -> Self {
		let (shutdown_tx, _) = broadcast::channel(16);

		Self {
			state: Arc::new(RwLock::new(LifecycleState::Uninitialized)),
			shutdown_tx,
		}
	}

	pub async fn get_state(&self) -> LifecycleState {
		*self.state.read().await
	}

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
		info!("Lifecycle state changed: {} -> {}", old_state, new_state);

		Ok(())
	}

	pub async fn initialize(&self) -> Result<(), CoreError> {
		self.set_state(LifecycleState::Initializing).await?;
		Ok(())
	}

	pub async fn start(&self) -> Result<(), CoreError> {
		self.set_state(LifecycleState::Running).await?;
		Ok(())
	}

	pub async fn shutdown(&self) -> Result<(), CoreError> {
		self.set_state(LifecycleState::Stopping).await?;
		let _ = self.shutdown_tx.send(());
		self.set_state(LifecycleState::Stopped).await?;
		Ok(())
	}

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
