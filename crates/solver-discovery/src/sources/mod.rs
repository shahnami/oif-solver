//! Different sources for discovering intents.

pub mod offchain;
pub mod onchain;

pub use offchain::OffchainSource;
pub use onchain::{OnChainConfig, OnChainSource};

use serde::{Deserialize, Serialize};
use solver_types::{chains::ChainId, common::BlockNumber};

/// Source location of discovered intent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IntentSourceLocation {
	OnChain {
		chain_id: ChainId,
		block: BlockNumber,
		transaction_hash: solver_types::common::TxHash,
		log_index: u64,
	},
	OffChain {
		api: String,
		timestamp: u64,
	},
}
