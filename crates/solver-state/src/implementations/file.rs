//! File-based storage implementation.

use crate::{storage::Storage, types::OrderState};
use async_trait::async_trait;
use solver_discovery::OrderStatus;
use solver_types::{
	errors::{Result, SolverError},
	orders::OrderId,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use tracing::{debug, warn};

/// File-based storage implementation
#[derive(Clone)]
pub struct FileStorage {
	base_path: PathBuf,
	/// In-memory cache for performance
	cache: dashmap::DashMap<OrderId, OrderState>,
}

impl FileStorage {
	pub async fn new(base_path: PathBuf) -> Result<Self> {
		// Create directory if it doesn't exist
		fs::create_dir_all(&base_path)
			.await
			.map_err(|e| SolverError::Other(e.into()))?;

		let storage = Self {
			base_path,
			cache: dashmap::DashMap::new(),
		};

		// Load existing data into cache
		storage.load_all().await?;

		Ok(storage)
	}

	/// Get file path for an order
	fn order_path(&self, order_id: &OrderId) -> PathBuf {
		self.base_path.join(format!("order_{}.json", order_id))
	}

	/// Load all orders from disk into cache
	async fn load_all(&self) -> Result<()> {
		let mut entries = fs::read_dir(&self.base_path)
			.await
			.map_err(|e| SolverError::Other(e.into()))?;

		while let Some(entry) = entries
			.next_entry()
			.await
			.map_err(|e| SolverError::Other(e.into()))?
		{
			let path = entry.path();
			if path.extension().and_then(|s| s.to_str()) == Some("json") {
				match fs::read_to_string(&path).await {
					Ok(content) => match serde_json::from_str::<OrderState>(&content) {
						Ok(state) => {
							self.cache.insert(state.id, state);
						}
						Err(e) => {
							warn!("Failed to parse order file {:?}: {}", path, e);
						}
					},
					Err(e) => {
						warn!("Failed to read order file {:?}: {}", path, e);
					}
				}
			}
		}

		debug!("Loaded {} orders from disk", self.cache.len());
		Ok(())
	}

	/// Persist order to disk
	async fn persist_order(&self, state: &OrderState) -> Result<()> {
		let path = self.order_path(&state.id);
		let content =
			serde_json::to_string_pretty(state).map_err(|e| SolverError::Other(e.into()))?;

		fs::write(&path, content)
			.await
			.map_err(|e| SolverError::Other(e.into()))?;

		Ok(())
	}

	/// Remove order from disk
	async fn remove_order(&self, order_id: &OrderId) -> Result<()> {
		let path = self.order_path(order_id);

		if path.exists() {
			fs::remove_file(&path)
				.await
				.map_err(|e| SolverError::Other(e.into()))?;
		}

		Ok(())
	}
}

#[async_trait]
impl Storage for FileStorage {
	async fn store_order_state(&self, state: &OrderState) -> Result<()> {
		// Update cache
		self.cache.insert(state.id, state.clone());

		// Persist to disk
		self.persist_order(state).await?;

		Ok(())
	}

	async fn get_order_state(&self, order_id: &OrderId) -> Result<Option<OrderState>> {
		Ok(self.cache.get(order_id).map(|entry| entry.clone()))
	}

	async fn get_orders_by_status(&self, status: OrderStatus) -> Result<Vec<OrderId>> {
		Ok(self
			.cache
			.iter()
			.filter(|entry| entry.status == status)
			.map(|entry| entry.id)
			.collect())
	}

	async fn count_by_status(&self) -> Result<HashMap<OrderStatus, usize>> {
		let mut counts = HashMap::new();

		for entry in self.cache.iter() {
			*counts.entry(entry.status).or_insert(0) += 1;
		}

		Ok(counts)
	}

	async fn delete_order_state(&self, order_id: &OrderId) -> Result<()> {
		// Remove from cache
		self.cache.remove(order_id);

		// Remove from disk
		self.remove_order(order_id).await?;

		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use solver_types::common::Bytes32;

	#[tokio::test]
	async fn test_file_storage() {
		use tempfile::TempDir;

		let temp_dir = TempDir::new().unwrap();
		let storage = FileStorage::new(temp_dir.path().to_path_buf())
			.await
			.unwrap();

		let order_id = Bytes32::from([2u8; 32]);

		// Create test state
		let state = OrderState {
			id: order_id,
			order_data: vec![4, 5, 6],
			status: OrderStatus::Ready,
			priority: crate::types::OrderPriority::calculate(
				solver_orders::classification::Urgency::High,
				Some(100),
				0,
			),
			discovered_at: 2000,
			queued_at: Some(2100),
			processed_at: None,
			completed_at: None,
			attempts: 0,
			last_error: None,
		};

		// Store
		storage.store_order_state(&state).await.unwrap();

		// Verify file exists
		let file_path = storage.order_path(&order_id);
		assert!(file_path.exists());

		// Create new storage instance to test persistence
		let storage2 = FileStorage::new(temp_dir.path().to_path_buf())
			.await
			.unwrap();

		// Should load from disk
		let retrieved = storage2.get_order_state(&order_id).await.unwrap();
		assert!(retrieved.is_some());
		assert_eq!(retrieved.unwrap().id, order_id);

		// Delete
		storage2.delete_order_state(&order_id).await.unwrap();
		assert!(!file_path.exists());
	}
}
