use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
	#[error("Not found")]
	NotFound,
	#[error("Serialization error: {0}")]
	Serialization(String),
	#[error("Backend error: {0}")]
	Backend(String),
}

#[async_trait]
pub trait StorageInterface: Send + Sync {
	async fn get_bytes(&self, key: &str) -> Result<Vec<u8>, StorageError>;
	async fn set_bytes(
		&self,
		key: &str,
		value: Vec<u8>,
		ttl: Option<Duration>,
	) -> Result<(), StorageError>;
	async fn delete(&self, key: &str) -> Result<(), StorageError>;
	async fn exists(&self, key: &str) -> Result<bool, StorageError>;
}

pub struct StorageService {
	backend: Box<dyn StorageInterface>,
}

impl StorageService {
	pub fn new(backend: Box<dyn StorageInterface>) -> Self {
		Self { backend }
	}

	pub async fn store_with_ttl<T: Serialize>(
		&self,
		namespace: &str,
		id: &str,
		data: &T,
		ttl: Option<Duration>,
	) -> Result<(), StorageError> {
		let key = format!("{}:{}", namespace, id);
		let bytes =
			serde_json::to_vec(data).map_err(|e| StorageError::Serialization(e.to_string()))?;
		self.backend.set_bytes(&key, bytes, ttl).await
	}

	pub async fn store<T: Serialize>(
		&self,
		namespace: &str,
		id: &str,
		data: &T,
	) -> Result<(), StorageError> {
		let key = format!("{}:{}", namespace, id);
		let bytes =
			serde_json::to_vec(data).map_err(|e| StorageError::Serialization(e.to_string()))?;
		self.backend.set_bytes(&key, bytes, None).await
	}

	pub async fn retrieve<T: DeserializeOwned>(
		&self,
		namespace: &str,
		id: &str,
	) -> Result<T, StorageError> {
		let key = format!("{}:{}", namespace, id);
		let bytes = self.backend.get_bytes(&key).await?;
		serde_json::from_slice(&bytes).map_err(|e| StorageError::Serialization(e.to_string()))
	}

	pub async fn remove(&self, namespace: &str, id: &str) -> Result<(), StorageError> {
		let key = format!("{}:{}", namespace, id);
		self.backend.delete(&key).await
	}
}
