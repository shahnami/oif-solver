//! Storage backend traits and utilities.

use crate::{
	implementations::{file::FileStorage, memory::MemoryStorage},
	types::OrderState,
};
use async_trait::async_trait;
use solver_discovery::OrderStatus;
use solver_types::{errors::Result, orders::OrderId};
use std::collections::HashMap;
use std::path::PathBuf;

/// Storage backend trait
#[async_trait]
pub trait Storage: Send + Sync {
	/// Store order state
	async fn store_order_state(&self, state: &OrderState) -> Result<()>;

	/// Get order state
	async fn get_order_state(&self, order_id: &OrderId) -> Result<Option<OrderState>>;

	/// Get orders by status
	async fn get_orders_by_status(&self, status: OrderStatus) -> Result<Vec<OrderId>>;

	/// Count orders by status
	async fn count_by_status(&self) -> Result<HashMap<OrderStatus, usize>>;

	/// Delete order state
	async fn delete_order_state(&self, order_id: &OrderId) -> Result<()>;
}

/// Storage backend type
#[derive(Debug, Clone)]
pub enum StorageBackend {
	/// In-memory storage (lost on restart)
	Memory,
	/// File-based storage (persisted)
	File { path: PathBuf },
}

/// Storage implementation wrapper
#[derive(Clone)]
pub enum StorageImpl {
	Memory(MemoryStorage),
	File(FileStorage),
}

#[async_trait]
impl Storage for StorageImpl {
	async fn store_order_state(&self, state: &OrderState) -> Result<()> {
		match self {
			StorageImpl::Memory(storage) => storage.store_order_state(state).await,
			StorageImpl::File(storage) => storage.store_order_state(state).await,
		}
	}

	async fn get_order_state(&self, order_id: &OrderId) -> Result<Option<OrderState>> {
		match self {
			StorageImpl::Memory(storage) => storage.get_order_state(order_id).await,
			StorageImpl::File(storage) => storage.get_order_state(order_id).await,
		}
	}

	async fn get_orders_by_status(&self, status: OrderStatus) -> Result<Vec<OrderId>> {
		match self {
			StorageImpl::Memory(storage) => storage.get_orders_by_status(status).await,
			StorageImpl::File(storage) => storage.get_orders_by_status(status).await,
		}
	}

	async fn count_by_status(&self) -> Result<HashMap<OrderStatus, usize>> {
		match self {
			StorageImpl::Memory(storage) => storage.count_by_status().await,
			StorageImpl::File(storage) => storage.count_by_status().await,
		}
	}

	async fn delete_order_state(&self, order_id: &OrderId) -> Result<()> {
		match self {
			StorageImpl::Memory(storage) => storage.delete_order_state(order_id).await,
			StorageImpl::File(storage) => storage.delete_order_state(order_id).await,
		}
	}
}

/// Create storage instance based on backend type
pub async fn create_storage(backend: StorageBackend) -> Result<StorageImpl> {
	match backend {
		StorageBackend::Memory => Ok(StorageImpl::Memory(MemoryStorage::new())),
		StorageBackend::File { path } => Ok(StorageImpl::File(FileStorage::new(path).await?)),
	}
}
