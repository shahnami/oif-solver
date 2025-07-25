use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::broadcast;

use crate::{ExecutionParams, FillProof, Intent, Order, TransactionHash, TransactionReceipt};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SolverEvent {
	Discovery(DiscoveryEvent),
	Order(OrderEvent),
	Delivery(DeliveryEvent),
	Settlement(SettlementEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryEvent {
	IntentDiscovered { intent: Intent },
	IntentValidated { intent_id: String, order: Order },
	IntentRejected { intent_id: String, reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderEvent {
	Executing {
		order: Order,
		params: ExecutionParams,
	},
	Skipped {
		order_id: String,
		reason: String,
	},
	Deferred {
		order_id: String,
		retry_after: Duration,
	},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeliveryEvent {
	TransactionPending {
		order_id: String,
		tx_hash: TransactionHash,
		tx_type: TransactionType,
	},
	TransactionConfirmed {
		tx_hash: TransactionHash,
		receipt: TransactionReceipt,
		tx_type: TransactionType,
	},
	TransactionFailed {
		tx_hash: TransactionHash,
		error: String,
	},
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SettlementEvent {
	FillDetected {
		order_id: String,
		tx_hash: TransactionHash,
	},
	ProofReady {
		order_id: String,
		proof: FillProof,
	},
	ClaimReady {
		order_id: String,
	},
	Completed {
		order_id: String,
	},
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TransactionType {
	Fill,
	Claim,
}

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
