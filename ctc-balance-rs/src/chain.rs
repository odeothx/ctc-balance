//! Chain connection module for Creditcoin3.
//!
//! Provides WebSocket RPC connection and block query functionality.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use subxt::{
    backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
    OnlineClient, PolkadotConfig,
};

use crate::{BLOCK_TIME_SECONDS, NODE_URL};

/// Block information with number and hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockInfo {
    pub block: u64,
    pub hash: String,
}

/// Chain connector for Creditcoin3
pub struct ChainConnector {
    url: String,
    client: Option<Arc<OnlineClient<PolkadotConfig>>>,
    rpc: Option<Arc<LegacyRpcMethods<PolkadotConfig>>>,
    genesis_timestamp: Option<u64>,
}

impl ChainConnector {
    /// Create a new chain connector
    pub fn new(url: Option<&str>) -> Self {
        Self {
            url: url.unwrap_or(NODE_URL).to_string(),
            client: None,
            rpc: None,
            genesis_timestamp: None,
        }
    }

    /// Get the URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Connect to the node
    pub async fn connect(&mut self) -> Result<()> {
        let rpc_client = RpcClient::from_url(&self.url)
            .await
            .context("Failed to connect to RPC")?;

        let client = OnlineClient::<PolkadotConfig>::from_rpc_client(rpc_client.clone())
            .await
            .context("Failed to create online client")?;

        let rpc = LegacyRpcMethods::<PolkadotConfig>::new(rpc_client);

        self.client = Some(Arc::new(client));
        self.rpc = Some(Arc::new(rpc));

        Ok(())
    }

    /// Ensure connected, connect if not
    async fn ensure_connected(&mut self) -> Result<()> {
        if self.client.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Get the online client
    fn client(&self) -> Result<&Arc<OnlineClient<PolkadotConfig>>> {
        self.client
            .as_ref()
            .context("Not connected. Call connect() first.")
    }

    /// Get the RPC methods
    fn rpc(&self) -> Result<&Arc<LegacyRpcMethods<PolkadotConfig>>> {
        self.rpc
            .as_ref()
            .context("Not connected. Call connect() first.")
    }

    /// Get chain information
    pub async fn get_chain_info(&mut self) -> Result<ChainInfo> {
        self.ensure_connected().await?;

        let client = self.client()?;
        let rpc = self.rpc()?;

        let genesis_hash = client.genesis_hash();
        let runtime_version = client.runtime_version();

        // Get chain name from system properties
        let chain_name = rpc
            .system_chain()
            .await
            .unwrap_or_else(|_| "Unknown".to_string());

        Ok(ChainInfo {
            chain: chain_name,
            version: format!(
                "{}.{}",
                runtime_version.spec_version, runtime_version.transaction_version
            ),
            genesis_hash: format!("{:?}", genesis_hash),
        })
    }

    /// Get block hash by block number
    pub async fn get_block_hash(&mut self, block_number: u64) -> Result<String> {
        self.ensure_connected().await?;
        let rpc = self.rpc()?;

        let hash = rpc
            .chain_get_block_hash(Some(block_number.into()))
            .await?
            .context(format!("Block {} not found", block_number))?;

        Ok(format!("{:?}", hash))
    }

    /// Get latest finalized block number
    pub async fn get_latest_block_number(&mut self) -> Result<u64> {
        self.ensure_connected().await?;
        let rpc = self.rpc()?;

        let header = rpc.chain_get_header(None).await?.context("No header")?;

        Ok(header.number as u64)
    }

    /// Get block timestamp in seconds (Unix timestamp)
    pub async fn get_block_timestamp(&mut self, block_hash: &str) -> Result<u64> {
        self.ensure_connected().await?;
        let client = self.client()?;

        // Parse the block hash
        let hash_bytes =
            hex::decode(block_hash.trim_start_matches("0x")).context("Invalid block hash")?;
        let hash: [u8; 32] = hash_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid hash length"))?;
        let block_hash = subxt::utils::H256::from(hash);

        // Query Timestamp.Now storage
        let storage_address = subxt::dynamic::storage("Timestamp", "Now", ());

        let storage_value = client
            .storage()
            .at(block_hash)
            .fetch(&storage_address)
            .await?
            .context("Timestamp not found")?;

        // Decode as u64 (milliseconds)
        let timestamp_ms: u128 = storage_value
            .as_type()
            .context("Failed to decode timestamp")?;

        Ok((timestamp_ms / 1000) as u64)
    }

    /// Get genesis timestamp (from block 1)
    pub async fn get_genesis_timestamp(&mut self) -> Result<u64> {
        if let Some(ts) = self.genesis_timestamp {
            return Ok(ts);
        }

        let hash = self.get_block_hash(1).await?;
        let ts = self.get_block_timestamp(&hash).await?;
        self.genesis_timestamp = Some(ts);

        Ok(ts)
    }

    /// Find block at target timestamp using binary search
    pub async fn find_block_at_timestamp(
        &mut self,
        target_timestamp: u64,
        tolerance_seconds: u64,
    ) -> Result<BlockInfo> {
        let latest_block = self.get_latest_block_number().await?;

        // Estimate block number
        let genesis_ts = self.get_genesis_timestamp().await?;
        let estimated_block = ((target_timestamp - genesis_ts) / BLOCK_TIME_SECONDS) as u64;

        // Search window
        let window = 20000u64;
        let mut low = estimated_block.saturating_sub(window);
        let mut high = std::cmp::min(latest_block, estimated_block + window);

        let mut best_block = 0u64;
        let mut best_hash = String::new();
        let mut best_diff = u64::MAX;

        while low <= high {
            let mid = (low + high) / 2;
            let block_hash = self.get_block_hash(mid).await?;
            let block_time = self.get_block_timestamp(&block_hash).await?;

            let diff = if block_time > target_timestamp {
                block_time - target_timestamp
            } else {
                target_timestamp - block_time
            };

            if diff < best_diff {
                best_diff = diff;
                best_block = mid;
                best_hash = block_hash.clone();
            }

            if diff <= tolerance_seconds {
                return Ok(BlockInfo {
                    block: mid,
                    hash: block_hash,
                });
            }

            if block_time < target_timestamp {
                low = mid + 1;
            } else {
                if mid == 0 {
                    break;
                }
                high = mid - 1;
            }
        }

        Ok(BlockInfo {
            block: best_block,
            hash: best_hash,
        })
    }
}

/// Chain information
#[derive(Debug, Clone)]
pub struct ChainInfo {
    pub chain: String,
    pub version: String,
    pub genesis_hash: String,
}

impl std::fmt::Display for ChainInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} v{}", self.chain, self.version)
    }
}
