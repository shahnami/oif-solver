// Discovery plugin implementations

pub mod offchain;
pub mod onchain;

pub use onchain::eip7683::{Eip7683OnchainConfig, Eip7683OnchainDiscoveryPlugin};
