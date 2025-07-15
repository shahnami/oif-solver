//! Discovery pipeline for processing discovered intents.

use crate::types::DiscoveredIntent;
use futures::Stream;
use solver_types::errors::Result;

/// Pipeline for processing discovered intents
pub struct DiscoveryPipeline {
	// Future: Add filters, transformers, etc.
}

impl DiscoveryPipeline {
	pub fn new() -> Self {
		Self {}
	}

	/// Process a stream of discovered intents
	pub fn process(
		&self,
		intents: impl Stream<Item = Result<DiscoveredIntent>>,
	) -> impl Stream<Item = Result<DiscoveredIntent>> {
		// For now, just pass through
		// Future: Add deduplication, filtering, transformation
		intents
	}
}

impl Default for DiscoveryPipeline {
	fn default() -> Self {
		Self::new()
	}
}
