use solver_types::SolverEvent;
use tokio::sync::broadcast;

pub struct EventBus {
	sender: broadcast::Sender<SolverEvent>,
}

impl EventBus {
	pub fn new(capacity: usize) -> Self {
		let (sender, _) = broadcast::channel(capacity);
		Self { sender }
	}

	pub fn subscribe(&self) -> broadcast::Receiver<SolverEvent> {
		self.sender.subscribe()
	}

	pub fn publish(
		&self,
		event: SolverEvent,
	) -> Result<(), broadcast::error::SendError<SolverEvent>> {
		self.sender.send(event)?;
		Ok(())
	}
}

impl Clone for EventBus {
	fn clone(&self) -> Self {
		Self {
			sender: self.sender.clone(),
		}
	}
}
