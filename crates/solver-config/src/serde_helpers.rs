//! Serde helpers for configuration deserialization

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solver_types::chains::ChainId;
use std::collections::HashMap;

/// Custom deserializer for HashMap<ChainId, String> that handles string keys
pub fn deserialize_chain_id_map<'de, D>(
	deserializer: D,
) -> Result<HashMap<ChainId, String>, D::Error>
where
	D: Deserializer<'de>,
{
	let map = HashMap::<String, String>::deserialize(deserializer)?;

	map.into_iter()
		.map(|(k, v)| {
			k.parse::<u64>()
				.map(|id| (ChainId(id), v))
				.map_err(|_| serde::de::Error::custom(format!("Invalid chain ID: {}", k)))
		})
		.collect()
}

/// Custom deserializer for HashMap<ChainId, T> that handles string keys
pub fn deserialize_chain_id_map_generic<'de, D, T>(
	deserializer: D,
) -> Result<HashMap<ChainId, T>, D::Error>
where
	D: Deserializer<'de>,
	T: Deserialize<'de>,
{
	let map = HashMap::<String, T>::deserialize(deserializer)?;

	map.into_iter()
		.map(|(k, v)| {
			k.parse::<u64>()
				.map(|id| (ChainId(id), v))
				.map_err(|_| serde::de::Error::custom(format!("Invalid chain ID: {}", k)))
		})
		.collect()
}

/// Custom serializer for HashMap<ChainId, String> that converts ChainId to string keys
pub fn serialize_chain_id_map<S>(
	map: &HashMap<ChainId, String>,
	serializer: S,
) -> Result<S::Ok, S::Error>
where
	S: Serializer,
{
	let string_map: HashMap<String, &String> =
		map.iter().map(|(k, v)| (k.0.to_string(), v)).collect();

	string_map.serialize(serializer)
}

/// Custom serializer for HashMap<ChainId, T> that converts ChainId to string keys
pub fn serialize_chain_id_map_generic<S, T>(
	map: &HashMap<ChainId, T>,
	serializer: S,
) -> Result<S::Ok, S::Error>
where
	S: Serializer,
	T: Serialize,
{
	let string_map: HashMap<String, &T> = map.iter().map(|(k, v)| (k.0.to_string(), v)).collect();

	string_map.serialize(serializer)
}

#[cfg(test)]
mod tests {
	use super::*;
	use serde::Deserialize;

	#[derive(Debug, Deserialize, Serialize)]
	struct TestStruct {
		#[serde(
			deserialize_with = "deserialize_chain_id_map",
			serialize_with = "serialize_chain_id_map"
		)]
		endpoints: HashMap<ChainId, String>,
	}

	#[test]
	fn test_deserialize_chain_id_map() {
		let toml = r#"
            [endpoints]
            1 = "endpoint1"
            42161 = "endpoint2"
        "#;

		let result: TestStruct = toml::from_str(toml).unwrap();
		assert_eq!(result.endpoints.get(&ChainId(1)).unwrap(), "endpoint1");
		assert_eq!(result.endpoints.get(&ChainId(42161)).unwrap(), "endpoint2");
	}

	#[test]
	fn test_serialize_chain_id_map() {
		let mut endpoints = HashMap::new();
		endpoints.insert(ChainId(1), "endpoint1".to_string());
		endpoints.insert(ChainId(42161), "endpoint2".to_string());

		let test_struct = TestStruct { endpoints };

		let toml = toml::to_string(&test_struct).unwrap();

		// Verify the TOML contains the expected string keys
		assert!(toml.contains("1 = \"endpoint1\""));
		assert!(toml.contains("42161 = \"endpoint2\""));

		// Verify round-trip works
		let parsed: TestStruct = toml::from_str(&toml).unwrap();
		assert_eq!(parsed.endpoints.get(&ChainId(1)).unwrap(), "endpoint1");
		assert_eq!(parsed.endpoints.get(&ChainId(42161)).unwrap(), "endpoint2");
	}
}
