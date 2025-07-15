//! Event types and filtering for intent discovery.

use futures::Stream;
use serde::{Deserialize, Serialize};
use solver_types::{
	chains::{ChainId, Log},
	common::{Address, BlockNumber, Bytes32, TxHash},
	errors::Result,
};
use std::pin::Pin;

/// Event emitted by smart contracts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
	pub address: Address,
	pub topics: Vec<Bytes32>,
	pub data: Vec<u8>,
	pub block_number: BlockNumber,
	pub transaction_hash: TxHash,
	pub log_index: u64,
	pub chain_id: ChainId,
}

impl Event {
	/// Convert from chain log
	pub fn from_log(log: Log, chain_id: ChainId) -> Self {
		Self {
			address: log.address,
			topics: log.topics,
			data: log.data,
			block_number: log.block_number,
			transaction_hash: log.transaction_hash,
			log_index: log.log_index,
			chain_id,
		}
	}
}

/// Event filter for watching specific events
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
	pub address: Option<Address>,
	pub topics: Vec<Option<Bytes32>>,
	pub from_block: Option<BlockNumber>,
	pub to_block: Option<BlockNumber>,
}

impl EventFilter {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn address(mut self, address: Address) -> Self {
		self.address = Some(address);
		self
	}

	pub fn topic0(mut self, topic: Bytes32) -> Self {
		if self.topics.is_empty() {
			self.topics.resize(1, None);
		}
		self.topics[0] = Some(topic);
		self
	}

	pub fn from_block(mut self, block: BlockNumber) -> Self {
		self.from_block = Some(block);
		self
	}

	pub fn to_block(mut self, block: BlockNumber) -> Self {
		self.to_block = Some(block);
		self
	}
}

/// Event stream type
pub type EventStream<'a> = Pin<Box<dyn Stream<Item = Result<Event>> + Send + 'a>>;

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_event_from_log() {
		let log = Log {
			address: Address::zero(),
			topics: vec![Bytes32::zero()],
			data: vec![1, 2, 3],
			block_number: 100,
			transaction_hash: Bytes32::zero(),
			log_index: 0,
		};

		let event = Event::from_log(log.clone(), ChainId(1));
		assert_eq!(event.address, log.address);
		assert_eq!(event.topics, log.topics);
		assert_eq!(event.data, log.data);
		assert_eq!(event.block_number, log.block_number);
		assert_eq!(event.chain_id, ChainId(1));
	}

	#[test]
	fn test_event_filter_builder() {
		let filter = EventFilter::new()
			.address(Address::from([1u8; 20]))
			.topic0(Bytes32::from([2u8; 32]))
			.from_block(100)
			.to_block(200);

		assert_eq!(filter.address, Some(Address::from([1u8; 20])));
		assert_eq!(filter.topics.len(), 1);
		assert_eq!(filter.topics[0], Some(Bytes32::from([2u8; 32])));
		assert_eq!(filter.from_block, Some(100));
		assert_eq!(filter.to_block, Some(200));
	}

	#[test]
	fn test_eip7683_event_topic() {
		let topic = solver_types::events::eip7683::open_event_topic();
		// Should be deterministic
		let topic2 = solver_types::events::eip7683::open_event_topic();
		assert_eq!(topic, topic2);

		// Should not be zero
		assert_ne!(topic, Bytes32::zero());

		// Print the actual hash for debugging
		println!(
			"Generated Open event topic: 0x{}",
			hex::encode(topic.as_ref())
		);

		// The expected hash from the actual contract
		let expected =
			hex::decode("a576d0af275d0c6207ef43ceee8c498a5d7a26b8157a32d3fdf361e64371628c")
				.unwrap();
		println!("Expected Open event topic: 0x{}", hex::encode(&expected));

		// Check if it matches
		assert_eq!(
			topic.as_ref(),
			expected.as_slice(),
			"Event signature mismatch"
		);
	}
}
