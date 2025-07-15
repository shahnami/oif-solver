//! Component registry for managing solver modules.

use solver_types::{
    chains::{ChainAdapter, ChainId},
    errors::{Result, SolverError},
};
use std::{collections::HashMap, sync::Arc};

/// Registry for managing solver components
pub struct ComponentRegistry {
    chains: HashMap<ChainId, Arc<dyn ChainAdapter>>,
}

impl ComponentRegistry {
    pub fn new() -> Self {
        Self {
            chains: HashMap::new(),
        }
    }

    /// Register a chain adapter
    pub fn register_chain(&mut self, adapter: Arc<dyn ChainAdapter>) -> Result<()> {
        let chain_id = adapter.chain_id();
        if self.chains.contains_key(&chain_id) {
            return Err(SolverError::Config(format!(
                "Chain {} already registered",
                chain_id
            )));
        }

        self.chains.insert(chain_id, adapter);
        Ok(())
    }

    /// Get chain adapter
    pub fn get_chain(&self, chain_id: &ChainId) -> Option<Arc<dyn ChainAdapter>> {
        self.chains.get(chain_id).cloned()
    }
}

impl Default for ComponentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
