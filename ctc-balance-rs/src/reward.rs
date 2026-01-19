//! Staking reward tracking module for Creditcoin3.
//!
//! Queries Staking.Rewarded events from the chain to track daily rewards.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use subxt::{
    backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
    OnlineClient, PolkadotConfig,
};

use crate::CTC_DECIMALS;

/// Staking reward data for an account
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StakingReward {
    /// Claimed reward (from Staking.Rewarded events)
    pub claimed: f64,
}

impl StakingReward {
    /// Create a zero reward
    pub fn zero() -> Self {
        Self { claimed: 0.0 }
    }
}

/// Reward tracker for Creditcoin3 accounts
pub struct RewardTracker {
    url: String,
    client: Option<OnlineClient<PolkadotConfig>>,
    rpc: Option<LegacyRpcMethods<PolkadotConfig>>,
}

impl RewardTracker {
    /// Create a new reward tracker
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            client: None,
            rpc: None,
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
        self.rpc = Some(rpc);
        Ok(())
    }

    /// Ensure connected
    pub async fn ensure_connected(&mut self) -> Result<()> {
        if self.client.is_none() {
            self.connect().await?;
        }
        Ok(())
    }

    /// Get the client
    pub fn client(&self) -> Result<&OnlineClient<PolkadotConfig>> {
        self.client
            .as_ref()
            .context("Not connected. Call connect() first.")
    }

    /// Get the RPC
    pub fn rpc(&self) -> Result<&LegacyRpcMethods<PolkadotConfig>> {
        self.rpc
            .as_ref()
            .context("Not connected. Call connect() first.")
    }

    /// Check if events are available at a block
    pub async fn has_events(&mut self, block: u64) -> bool {
        if self.ensure_connected().await.is_err() {
            return false;
        }
        let rpc = match self.rpc() {
            Ok(r) => r,
            Err(_) => return false,
        };
        let client = match self.client() {
            Ok(c) => c,
            Err(_) => return false,
        };

        match rpc.chain_get_block_hash(Some(block.into())).await {
            Ok(Some(hash)) => match client.blocks().at(hash).await {
                Ok(b) => b.events().await.is_ok(),
                Err(_) => false,
            },
            _ => false,
        }
    }

    /// Get block hash for a block number
    pub async fn get_block_hash(&self, block_number: u64) -> Result<subxt::utils::H256> {
        let rpc = self.rpc()?;
        let hash = rpc
            .chain_get_block_hash(Some(block_number.into()))
            .await?
            .context(format!("Block {} not found", block_number))?;
        Ok(hash)
    }

    /// Get staking rewards for an account in a block range
    pub async fn get_rewards_in_range(
        &mut self,
        address: &str,
        start_block: u64,
        end_block: u64,
    ) -> Result<StakingReward> {
        let mut accounts = HashMap::new();
        accounts.insert("default".to_string(), address.to_string());
        let results = self
            .get_all_rewards_in_range(&accounts, start_block, end_block)
            .await?;
        Ok(results.get("default").cloned().unwrap_or_default())
    }

    /// Get rewards for multiple accounts in a block range
    pub async fn get_all_rewards_in_range(
        &mut self,
        accounts: &HashMap<String, String>,
        start_block: u64,
        end_block: u64,
    ) -> Result<HashMap<String, StakingReward>> {
        self.ensure_connected().await?;
        let client = self.client.clone().context("Client not initialized")?;
        let rpc = self.rpc.clone().context("RPC not initialized")?;

        let mut results = HashMap::new();
        let divisor = 10u128.pow(CTC_DECIMALS) as f64;

        // Build account lookup map: [u8; 32] -> (Name, Total, SS58)
        let mut account_lookup: HashMap<[u8; 32], (String, u128, String)> = HashMap::new();
        for (name, address) in accounts {
            if let Ok(account_id) = parse_ss58_address(address) {
                account_lookup.insert(account_id.0, (name.clone(), 0, address.clone()));
            }
        }

        use futures::stream::{self, StreamExt};
        let blocks: Vec<u64> = (start_block..=end_block).collect();
        let total_blocks = blocks.len();

        let mut processed_count = 0;

        let mut stream = stream::iter(blocks)
            .map(|block| {
                let rpc = rpc.clone();
                let client = client.clone();
                async move {
                    let hash = match rpc.chain_get_block_hash(Some(block.into())).await {
                        Ok(Some(h)) => h,
                        _ => return (block, None),
                    };
                    let events = match client.blocks().at(hash).await {
                        Ok(b) => match b.events().await {
                            Ok(e) => Some(e),
                            Err(_) => None,
                        },
                        Err(_) => None,
                    };
                    (block, events)
                }
            })
            .buffer_unordered(50); // Process 50 blocks in parallel

        while let Some((block, events)) = stream.next().await {
            processed_count += 1;

            if total_blocks > 100 && (processed_count % 100 == 0 || processed_count == total_blocks)
            {
                let percent = processed_count * 100 / total_blocks;
                println!(
                    "    Scanning blocks: {}% ({}/{})",
                    percent, processed_count, total_blocks
                );
            }

            if let Some(events) = events {
                for event in events.iter() {
                    if let Ok(event) = event {
                        let pallet = event.pallet_name();
                        let variant = event.variant_name();

                        if pallet == "Staking"
                            || pallet == "StakingReward"
                            || pallet == "Rewards"
                            || pallet == "Creditstaking"
                        {
                            if let Ok(decoded) = event.field_values() {
                                let is_reward_variant =
                                    variant == "Rewarded" || variant == "Reward";

                                // Stringify only once per event
                                let debug_str = format!("{:?}", decoded);

                                for (account_bytes, (name, total, ss58_addr)) in
                                    account_lookup.iter_mut()
                                {
                                    let matched = match_account_in_debug_str(
                                        &debug_str,
                                        account_bytes,
                                        ss58_addr,
                                    );

                                    if matched {
                                        if is_reward_variant {
                                            if cfg!(debug_assertions) {
                                                println!(
                                                    "  [DEBUG] Found {}.{} at block {}: {}",
                                                    pallet, variant, block, debug_str
                                                );
                                            }
                                            let amount =
                                                parse_u128_from_debug(&debug_str, "amount")
                                                    .or_else(|| {
                                                        parse_u128_from_debug(&debug_str, "reward")
                                                    })
                                                    .or_else(|| {
                                                        parse_u128_from_debug(&debug_str, "value")
                                                    })
                                                    .or_else(|| {
                                                        parse_u128_from_debug(&debug_str, "1")
                                                    })
                                                    .or_else(|| find_any_u128(&debug_str));

                                            if let Some(amt) = amount {
                                                if cfg!(debug_assertions) {
                                                    println!(
                                                        "  [DEBUG] Matched account {} with reward {}",
                                                        name, amt
                                                    );
                                                }
                                                *total += amt;
                                            } else {
                                                if cfg!(debug_assertions) {
                                                    println!(
                                                        "  [DEBUG] Matched account {} but could not parse amount from: {}",
                                                        name, debug_str
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Build results
        for (_account_bytes, (name, amount, _)) in account_lookup {
            results.insert(
                name,
                StakingReward {
                    claimed: amount as f64 / divisor,
                },
            );
        }

        for name in accounts.keys() {
            if !results.contains_key(name) {
                results.insert(name.clone(), StakingReward::zero());
            }
        }

        Ok(results)
    }
}

/// Parse a u128 value from debug string
fn parse_u128_from_debug(debug_str: &str, field_name: &str) -> Option<u128> {
    let pattern1 = format!("(\"{}\", Value", field_name);
    let pattern2 = format!("\"{}\", Value", field_name);

    for pattern in [&pattern1, &pattern2] {
        if let Some(pos) = debug_str.find(pattern.as_str()) {
            let remaining = &debug_str[pos..];
            if let Some(u128_pos) = remaining.find("U128(") {
                let after_u128 = &remaining[(u128_pos + 5)..];
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

/// Find any U128 value in the debug string
fn find_any_u128(debug_str: &str) -> Option<u128> {
    let mut last_val = None;
    let mut current_pos = 0;

    while let Some(pos) = debug_str[current_pos..].find("U128(") {
        let abs_pos = current_pos + pos;
        let after_u128 = &debug_str[(abs_pos + 5)..];
        let num_str: String = after_u128
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();

        if !num_str.is_empty() {
            if let Ok(val) = num_str.parse::<u128>() {
                // Ignore small values that might be indices or contexts
                if val > 1000 {
                    last_val = Some(val);
                }
            }
        }
        current_pos = abs_pos + 5 + num_str.len();
    }

    last_val
}

/// Match account in a debug string by looking for byte sequences or SS58
fn match_account_in_debug_str(debug_str: &str, target_bytes: &[u8; 32], ss58_addr: &str) -> bool {
    // 1. Try SS58 match
    if debug_str.contains(ss58_addr) {
        return true;
    }

    // 2. Try hex match (0x prefix or not)
    let hex_addr = hex::encode(target_bytes);
    if debug_str.to_lowercase().contains(&hex_addr.to_lowercase()) {
        return true;
    }

    // 3. Try byte sequence match U128(b1), U128(b2), ...
    // Extract all numbers after U128(
    let mut values = Vec::new();
    let mut current_pos = 0;
    while let Some(pos) = debug_str[current_pos..].find("U128(") {
        let abs_pos = current_pos + pos;
        let after_u128 = &debug_str[(abs_pos + 5)..];
        let num_str: String = after_u128
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(val) = num_str.parse::<u128>() {
            values.push(val);
        }
        current_pos = abs_pos + 5 + num_str.len();
    }

    // Check if target_bytes appears as a subsequence in values
    if values.len() >= 32 {
        for i in 0..=(values.len() - 32) {
            let mut matched = true;
            for j in 0..32 {
                if values[i + j] != target_bytes[j] as u128 {
                    matched = false;
                    break;
                }
            }
            if matched {
                return true;
            }
        }
    }

    false
}

/// Parse SS58 address to AccountId32
fn parse_ss58_address(address: &str) -> Result<subxt::utils::AccountId32> {
    use std::str::FromStr;
    subxt::utils::AccountId32::from_str(address)
        .map_err(|e| anyhow::anyhow!("Invalid SS58 address '{}': {}", address, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staking_reward_default() {
        let reward = StakingReward::default();
        assert_eq!(reward.claimed, 0.0);
    }

    #[test]
    fn test_staking_reward_zero() {
        let reward = StakingReward::zero();
        assert_eq!(reward.claimed, 0.0);
    }

    #[test]
    fn test_parse_u128() {
        let debug_str = r#"[("stash", Value { value: Primitive(U128(12345)) }), ("amount", Value { value: Primitive(U128(67890)) })]"#;
        assert_eq!(parse_u128_from_debug(debug_str, "amount"), Some(67890));
        assert_eq!(parse_u128_from_debug(debug_str, "stash"), Some(12345));

        let tuple_str = r#"[("0", Value { value: Primitive(U128(111)) }), ("1", Value { value: Primitive(U128(222)) })]"#;
        assert_eq!(parse_u128_from_debug(tuple_str, "0"), Some(111));
        assert_eq!(parse_u128_from_debug(tuple_str, "1"), Some(222));
    }

    #[test]
    fn test_find_any_u128() {
        let debug_str = r#"[("stash", Value { value: Primitive(U128(12345)) }), ("amount", Value { value: Primitive(U128(67890)) })]"#;
        // Should find the last one (amount)
        assert_eq!(find_any_u128(debug_str), Some(67890));

        let single = r#"[("value", Value { value: Primitive(U128(5000)) })]"#;
        assert_eq!(find_any_u128(single), Some(5000));
    }

    #[test]
    fn test_match_account_in_debug_str() {
        let mut target = [0u8; 32];
        target[0] = 198;
        target[1] = 152;
        target[31] = 40;

        let ss58 = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";

        // 1. SS58 match
        assert!(match_account_in_debug_str(
            "stash: 5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
            &target,
            ss58
        ));

        // 2. Hex match
        assert!(match_account_in_debug_str(
            "stash: 0xc698000000000000000000000000000000000000000000000000000000000028",
            &target,
            ss58
        ));

        // 3. Byte sequence match
        let debug_str = "U128(198), U128(152), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(0), U128(40)";
        assert!(match_account_in_debug_str(debug_str, &target, ss58));

        // 4. No match
        assert!(!match_account_in_debug_str("nothing", &target, ss58));
    }

    #[test]
    fn test_parse_ss58() {
        let addr = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
        assert!(parse_ss58_address(addr).is_ok());
    }
}
