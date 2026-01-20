//! Balance query module for Creditcoin3 accounts.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use subxt::{
    backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
    OnlineClient, PolkadotConfig,
};

use crate::CTC_DIVISOR;

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

    /// Set the online client (injection for tracker reuse)
    pub fn set_client(&mut self, client: OnlineClient<PolkadotConfig>) {
        self.client = Some(client);
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
                let decoded = value.to_value()?;
                let mut free = 0u128;
                let mut reserved = 0u128;
                let mut frozen = 0u128;

                // System.Account structure: { nonce, consumers, providers, sufficients, data: { free, reserved, frozen, flags } }
                if let subxt::ext::scale_value::ValueDef::Composite(
                    subxt::ext::scale_value::Composite::Named(fields),
                ) = decoded.value
                {
                    for (name, field) in fields {
                        if name.as_str() == "data" {
                            // Extract balance data from the nested 'data' field
                            if let subxt::ext::scale_value::ValueDef::Composite(
                                subxt::ext::scale_value::Composite::Named(data_fields),
                            ) = field.value
                            {
                                for (data_name, data_field) in data_fields {
                                    match data_name.as_str() {
                                        "free" => {
                                            if let subxt::ext::scale_value::ValueDef::Primitive(
                                                subxt::ext::scale_value::Primitive::U128(val),
                                            ) = data_field.value
                                            {
                                                free = val;
                                            }
                                        }
                                        "reserved" => {
                                            if let subxt::ext::scale_value::ValueDef::Primitive(
                                                subxt::ext::scale_value::Primitive::U128(val),
                                            ) = data_field.value
                                            {
                                                reserved = val;
                                            }
                                        }
                                        "frozen" => {
                                            if let subxt::ext::scale_value::ValueDef::Primitive(
                                                subxt::ext::scale_value::Primitive::U128(val),
                                            ) = data_field.value
                                            {
                                                frozen = val;
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(Balance {
                    free: free as f64 / CTC_DIVISOR,
                    reserved: reserved as f64 / CTC_DIVISOR,
                    frozen: frozen as f64 / CTC_DIVISOR,
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

        use futures::stream::{self, StreamExt};
        let mut stream = stream::iter(accounts.iter())
            .map(|(name, address)| {
                let name = name.clone();
                let address = address.clone();
                let client = client.clone();
                let block_hash = block_hash_str.clone();
                let url = self.url.clone();

                async move {
                    let mut tracker = BalanceTracker {
                        url,
                        client: Some(client),
                        _rpc: None,
                    };
                    let res = tracker.get_balance(&address, &block_hash).await;
                    (name, res)
                }
            })
            .buffer_unordered(crate::CONCURRENCY_STORAGE);

        let mut balances = HashMap::new();
        while let Some((name, res)) = stream.next().await {
            let balance = res?;
            balances.insert(name, balance);
        }

        Ok(balances)
    }
}

/// Parse SS58 address to AccountId32
// Moved to lib.rs

#[cfg(test)]
mod tests {
    use super::*;

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
