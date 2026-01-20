//! Balance query module for Creditcoin3 accounts.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use subxt::{
    backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
    OnlineClient, PolkadotConfig,
};

use crate::CTC_DECIMALS;

/// Account balance data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    /// Free balance (CTC)
    pub free: f64,
    /// Reserved balance (CTC)
    pub reserved: f64,
    /// Frozen balance (CTC)
    pub frozen: f64,
}

impl Balance {
    /// Create a zero balance
    pub fn zero() -> Self {
        Self {
            free: 0.0,
            reserved: 0.0,
            frozen: 0.0,
        }
    }

    /// Total balance (free + reserved)
    pub fn total(&self) -> f64 {
        self.free + self.reserved
    }
}

impl Default for Balance {
    fn default() -> Self {
        Self::zero()
    }
}

/// Balance tracker for Creditcoin3 accounts
pub struct BalanceTracker {
    url: String,
    client: Option<OnlineClient<PolkadotConfig>>,
    _rpc: Option<LegacyRpcMethods<PolkadotConfig>>,
}

impl BalanceTracker {
    /// Create a new balance tracker
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: None,
            _rpc: None,
        }
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

        self.client = Some(client);
        self._rpc = Some(rpc);
        Ok(())
    }

    /// Ensure connected
    async fn ensure_connected(&mut self) -> Result<()> {
        if self.client.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Get the client
    fn client(&self) -> Result<&OnlineClient<PolkadotConfig>> {
        self.client
            .as_ref()
            .context("Not connected. Call connect() first.")
    }

    /// Get account balance at a specific block
    pub async fn get_balance(&mut self, address: &str, block_hash: &str) -> Result<Balance> {
        self.ensure_connected().await?;
        let client = self.client()?;

        // Parse the block hash
        let hash_bytes =
            hex::decode(block_hash.trim_start_matches("0x")).context("Invalid block hash")?;
        let hash: [u8; 32] = hash_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid hash length"))?;
        let block_hash = subxt::utils::H256::from(hash);

        // Parse address as AccountId32
        let account_id = crate::parse_ss58_address(address)?;

        // Convert AccountId32 to dynamic Value for storage key
        let account_value = subxt::dynamic::Value::from_bytes(account_id.0);

        // Query System.Account storage using dynamic address
        let storage_address = subxt::dynamic::storage("System", "Account", vec![account_value]);

        let storage_value = crate::retry!(client.storage().at(block_hash).fetch(&storage_address))?;

        match storage_value {
            Some(value) => {
                // Convert to Value for parsing
                let decoded = value.to_value()?;
                let divisor = 10u128.pow(CTC_DECIMALS) as f64;

                // Use debug representation to extract values
                let debug_str = format!("{:?}", decoded);

                let free = parse_field_value(&debug_str, "free").unwrap_or(0);
                let reserved = parse_field_value(&debug_str, "reserved").unwrap_or(0);
                let frozen = parse_field_value(&debug_str, "frozen").unwrap_or(0);

                Ok(Balance {
                    free: free as f64 / divisor,
                    reserved: reserved as f64 / divisor,
                    frozen: frozen as f64 / divisor,
                })
            }
            None => Ok(Balance::zero()),
        }
    }

    /// Get balances for multiple accounts in parallel
    pub async fn get_all_balances(
        &mut self,
        accounts: &HashMap<String, String>,
        block_hash: &str,
    ) -> Result<HashMap<String, Balance>> {
        self.ensure_connected().await?;

        let client = self.client.clone().context("Client not initialized")?;
        let block_hash_str = block_hash.to_string();

        let mut tasks = Vec::new();
        for (name, address) in accounts {
            let name = name.clone();
            let address = address.clone();
            let mut tracker = BalanceTracker {
                url: self.url.clone(),
                client: Some(client.clone()),
                _rpc: None,
            };
            let block_hash_task = block_hash_str.clone();

            tasks.push(tokio::spawn(async move {
                let res = tracker.get_balance(&address, &block_hash_task).await;
                (name, res)
            }));
        }

        let results = futures::future::join_all(tasks).await;
        let mut balances = HashMap::new();
        for res in results {
            let (name, balance_res) = res.context("Join error in get_all_balances")?;
            let balance = balance_res?;
            balances.insert(name, balance);
        }

        Ok(balances)
    }
}

/// Parse a field value from debug string
///
/// Looks for patterns like:
/// - `("free", Value { value: Primitive(U128(12345))`
fn parse_field_value(debug_str: &str, field_name: &str) -> Option<u128> {
    // Pattern for the actual format: ("field", Value { value: Primitive(U128(number))
    let pattern1 = format!("(\"{}\", Value", field_name);
    let pattern2 = format!("\"{}\", Value", field_name);

    for pattern in [&pattern1, &pattern2] {
        if let Some(pos) = debug_str.find(pattern.as_str()) {
            // Find U128( after this position
            let remaining = &debug_str[pos..];
            if let Some(u128_pos) = remaining.find("U128(") {
                let after_u128 = &remaining[(u128_pos + 5)..];
                // Extract the number until the closing paren
                let num_str: String = after_u128
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if !num_str.is_empty() {
                    return num_str.parse().ok();
                }
            }
        }
    }

    None
}

/// Parse SS58 address to AccountId32
// Moved to lib.rs

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_field_value() {
        let debug_str = r#"Composite(Named([("free", Value { value: Primitive(U128(1000000000000000000)) }), ("reserved", Value { value: Primitive(U128(500000000000000000)) }), ("frozen", Value { value: Primitive(U128(0)) })]))"#;

        assert_eq!(
            parse_field_value(debug_str, "free"),
            Some(1000000000000000000)
        );
        assert_eq!(
            parse_field_value(debug_str, "reserved"),
            Some(500000000000000000)
        );
        assert_eq!(parse_field_value(debug_str, "frozen"), Some(0));
        assert_eq!(parse_field_value(debug_str, "nonexistent"), None);
    }

    #[test]
    fn test_parse_ss58_address() {
        // Valid SS58 address (Creditcoin/Substrate)
        let addr = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
        assert!(crate::parse_ss58_address(addr).is_ok());

        // Invalid address
        let invalid_addr = "invalid";
        assert!(crate::parse_ss58_address(invalid_addr).is_err());
    }

    #[test]
    fn test_balance_total() {
        let b = Balance {
            free: 100.0,
            reserved: 50.0,
            frozen: 10.0,
        };
        assert_eq!(b.total(), 150.0);
    }
}
