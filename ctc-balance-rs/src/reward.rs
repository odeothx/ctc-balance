//! Staking reward tracking module for Creditcoin3.
//!
//! Queries staking data from the chain to track rewards.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use subxt::{
    backend::{legacy::LegacyRpcMethods, rpc::RpcClient},
    ext::scale_value::{Composite, Primitive, Value, ValueDef},
    OnlineClient, PolkadotConfig,
};

use crate::CTC_DIVISOR;

/// Staking reward data for an account
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StakingReward {
    /// Claimed reward
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

    /// Set the RPC methods (injection for tracker reuse)
    pub fn set_rpc(&mut self, rpc: LegacyRpcMethods<PolkadotConfig>) {
        self.rpc = Some(rpc);
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

    /// Get block hash for a block number
    pub async fn get_block_hash(&self, block_number: u64) -> Result<subxt::utils::H256> {
        let rpc = self.rpc()?;
        let hash = rpc
            .chain_get_block_hash(Some(block_number.into()))
            .await?
            .context(format!("Block {} not found", block_number))?;
        Ok(hash)
    }

    /// Get active era at a specific block hash
    pub async fn get_active_era(&self, block_hash: subxt::utils::H256) -> Result<u32> {
        let client = self.client()?;
        let storage_address = subxt::dynamic::storage("Staking", "ActiveEra", ());
        let storage_value = crate::retry!(client.storage().at(block_hash).fetch(&storage_address))?;

        if let Some(value) = storage_value {
            let decoded = value.to_value()?;
            if let ValueDef::Composite(Composite::Named(fields)) = decoded.value {
                for (name, field) in fields {
                    if name == "index" {
                        if let ValueDef::Primitive(Primitive::U128(idx)) = field.value {
                            return Ok(idx as u32);
                        }
                    }
                }
            }
        }
        anyhow::bail!("ActiveEra not found at block {:?}", block_hash)
    }

    /// Check if a block has staking events
    pub async fn has_events(&mut self, block_number: u64) -> bool {
        self.ensure_connected().await.ok();
        let hash_res = crate::retry!(async { self.get_block_hash(block_number).await });

        if let Ok(hash) = hash_res {
            if let Ok(client) = self.client() {
                if let Ok(block) = crate::retry!(client.blocks().at(hash)) {
                    if let Ok(events) = block.events().await {
                        for event in events.iter().flatten() {
                            if event.pallet_name() == "Staking"
                                && (event.variant_name() == "Rewarded"
                                    || event.variant_name() == "Reward")
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// Get rewards for accounts in a block range using Eras
    pub async fn get_rewards_via_eras(
        &mut self,
        accounts: &HashMap<String, String>,
        start_block: u64,
        end_block: u64,
    ) -> Result<HashMap<String, StakingReward>> {
        self.ensure_connected().await?;
        let client = self.client.clone().context("Client not initialized")?;

        let start_hash = self.get_block_hash(start_block).await?;
        let end_hash = self.get_block_hash(end_block).await?;

        let start_era = self.get_active_era(start_hash).await.unwrap_or(0);
        let end_era = self.get_active_era(end_hash).await.unwrap_or(0);

        let mut results = HashMap::new();
        for name in accounts.keys() {
            results.insert(name.clone(), StakingReward { claimed: 0.0 });
        }

        if start_era == 0 || end_era == 0 {
            return Ok(results);
        }

        let mut cumulative_reward_map: HashMap<String, f64> = HashMap::new();
        for name in accounts.keys() {
            cumulative_reward_map.insert(name.clone(), 0.0);
        }

        let mut account_map: HashMap<[u8; 32], String> = HashMap::new();
        for (name, address) in accounts {
            if let Ok(id) = crate::parse_ss58_address(address) {
                account_map.insert(id.0, name.clone());
            }
        }

        for era in start_era..=end_era {
            self.process_era_rewards(
                era,
                end_hash,
                &client,
                &account_map,
                &mut cumulative_reward_map,
            )
            .await?;
        }

        let mut final_results = HashMap::new();
        for (name, amt) in cumulative_reward_map {
            final_results.insert(
                name,
                StakingReward {
                    claimed: amt / CTC_DIVISOR,
                },
            );
        }

        Ok(final_results)
    }

    /// Process rewards for a single era
    async fn process_era_rewards(
        &self,
        era: u32,
        end_hash: subxt::utils::H256,
        client: &OnlineClient<PolkadotConfig>,
        account_map: &HashMap<[u8; 32], String>,
        cumulative_reward_map: &mut HashMap<String, f64>,
    ) -> Result<()> {
        let total_reward_addr = subxt::dynamic::storage(
            "Staking",
            "ErasValidatorReward",
            vec![subxt::dynamic::Value::u128(era as u128)],
        );
        let points_addr = subxt::dynamic::storage(
            "Staking",
            "ErasRewardPoints",
            vec![subxt::dynamic::Value::u128(era as u128)],
        );

        let total_reward_val =
            match crate::retry!(client.storage().at(end_hash).fetch(&total_reward_addr))? {
                Some(v) => {
                    let val = v.to_value()?;
                    match val.value {
                        ValueDef::Primitive(Primitive::U128(r)) => r as f64,
                        _ => 0.0,
                    }
                }
                None => return Ok(()),
            };

        let points_data = match crate::retry!(client.storage().at(end_hash).fetch(&points_addr))? {
            Some(v) => v.to_value()?,
            None => return Ok(()),
        };

        let (total_points, validator_points) = parse_reward_points_def(points_data);

        if total_points == 0.0 || total_reward_val == 0.0 {
            return Ok(());
        }

        use futures::stream::{self, StreamExt};
        let validator_keys: Vec<[u8; 32]> = validator_points.keys().cloned().collect();

        let mut stream = stream::iter(validator_keys)
            .map(|v_bytes| {
                let client = client.clone();
                async move {
                    let exposure_addr = subxt::dynamic::storage(
                        "Staking",
                        "ErasStakersOverview",
                        vec![
                            subxt::dynamic::Value::u128(era as u128),
                            subxt::dynamic::Value::from_bytes(v_bytes),
                        ],
                    );
                    let legacy_exposure_addr = subxt::dynamic::storage(
                        "Staking",
                        "ErasStakersClipped",
                        vec![
                            subxt::dynamic::Value::u128(era as u128),
                            subxt::dynamic::Value::from_bytes(v_bytes),
                        ],
                    );
                    let prefs_addr = subxt::dynamic::storage(
                        "Staking",
                        "ErasValidatorPrefs",
                        vec![
                            subxt::dynamic::Value::u128(era as u128),
                            subxt::dynamic::Value::from_bytes(v_bytes),
                        ],
                    );

                    let exposure =
                        match crate::retry!(client.storage().at(end_hash).fetch(&exposure_addr)) {
                            Ok(Some(e)) => Some(e),
                            _ => crate::retry!(client
                                .storage()
                                .at(end_hash)
                                .fetch(&legacy_exposure_addr))
                            .ok()
                            .flatten(),
                        };
                    let prefs = crate::retry!(client.storage().at(end_hash).fetch(&prefs_addr))
                        .ok()
                        .flatten();

                    (v_bytes, exposure, prefs)
                }
            })
            .buffer_unordered(crate::CONCURRENCY_EXPOSURES);

        while let Some((v_bytes, exposure_val, prefs_val)) = stream.next().await {
            let p_v = *validator_points.get(&v_bytes).unwrap_or(&0.0);
            if p_v == 0.0 {
                continue;
            }

            let r_v_total = (total_reward_val * p_v) / total_points;

            let commission_ratio = if let Some(p) = prefs_val {
                let decoded = p.to_value()?;
                parse_commission_def(decoded)
            } else {
                0.0
            };

            if let Some(e) = exposure_val {
                let decoded = e.to_value()?;
                let (e_total, e_own, mut nominators, page_count) = parse_exposure_def(decoded);

                if e_total == 0.0 {
                    continue;
                }

                // If nominators is empty but page_count > 0, fetch from ErasStakersPaged
                if nominators.is_empty() && page_count > 0 {
                    for page_idx in 0..page_count {
                        let paged_addr = subxt::dynamic::storage(
                            "Staking",
                            "ErasStakersPaged",
                            vec![
                                subxt::dynamic::Value::u128(era as u128),
                                subxt::dynamic::Value::from_bytes(v_bytes),
                                subxt::dynamic::Value::u128(page_idx as u128),
                            ],
                        );
                        if let Ok(Some(page_val)) =
                            crate::retry!(client.storage().at(end_hash).fetch(&paged_addr))
                        {
                            if let Ok(page_decoded) = page_val.to_value() {
                                let page_nominators = parse_paged_exposure(page_decoded);
                                nominators.extend(page_nominators);
                            }
                        }
                    }
                }

                if let Some(name) = account_map.get(&v_bytes) {
                    let validator_reward = (r_v_total * commission_ratio)
                        + (r_v_total * (1.0 - commission_ratio) * (e_own / e_total));
                    *cumulative_reward_map.entry(name.clone()).or_insert(0.0) += validator_reward;
                }

                for (n_bytes, n_value) in nominators {
                    if let Some(name) = account_map.get(&n_bytes) {
                        let nominator_reward =
                            r_v_total * (1.0 - commission_ratio) * (n_value / e_total);

                        *cumulative_reward_map.entry(name.clone()).or_insert(0.0) +=
                            nominator_reward;
                    }
                }
            }
        }
        Ok(())
    }

    /// Fallback method using event scanning
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

        let mut account_lookup: HashMap<[u8; 32], (String, u128, String)> = HashMap::new();
        for (name, address) in accounts {
            if let Ok(account_id) = crate::parse_ss58_address(address) {
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
                    let hash = match crate::retry!(rpc.chain_get_block_hash(Some(block.into()))) {
                        Ok(Some(h)) => h,
                        _ => return (block, None),
                    };
                    let events = crate::retry!(client.blocks().at(hash))
                        .map(|b| async move { b.events().await.ok() })
                        .ok();
                    match events {
                        Some(f) => (block, f.await),
                        None => (block, None),
                    }
                }
            })
            .buffer_unordered(crate::CONCURRENCY_EVENTS);

        while let Some((_block, events)) = stream.next().await {
            processed_count += 1;
            if total_blocks > 100 && (processed_count % 100 == 0 || processed_count == total_blocks)
            {
                println!(
                    "    Scanning blocks: {}% ({}/{})",
                    processed_count * 100 / total_blocks,
                    processed_count,
                    total_blocks
                );
            }

            if let Some(events) = events {
                for event in events.iter().flatten() {
                    if event.pallet_name() == "Staking"
                        && (event.variant_name() == "Rewarded" || event.variant_name() == "Reward")
                    {
                        if let Ok(decoded) = event.field_values() {
                            let mut stash = None;
                            let mut amount = 0u128;

                            match decoded {
                                subxt::ext::scale_value::Composite::Named(named) => {
                                    for (name, field) in named {
                                        match name.as_str() {
                                            "stash" => stash = extract_account_id_from_value(&field),
                                            "amount" | "reward" | "value" => {
                                                if let subxt::ext::scale_value::ValueDef::Primitive(
                                                    subxt::ext::scale_value::Primitive::U128(val),
                                                ) = field.value
                                                {
                                                    amount = val;
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                subxt::ext::scale_value::Composite::Unnamed(unnamed) => {
                                    if unnamed.len() >= 2 {
                                        stash = extract_account_id_from_value(&unnamed[0]);
                                        if let subxt::ext::scale_value::ValueDef::Primitive(
                                            subxt::ext::scale_value::Primitive::U128(val),
                                        ) = unnamed[1].value
                                        {
                                            amount = val;
                                        }
                                    }
                                }
                            }

                            if let Some(stash_bytes) = stash {
                                if let Some((_name, total, _ss58)) =
                                    account_lookup.get_mut(&stash_bytes)
                                {
                                    *total += amount;
                                }
                            }
                        }
                    }
                }
            }
        }

        for (_bytes, (name, amount, _)) in account_lookup {
            results.insert(
                name,
                StakingReward {
                    claimed: amount as f64 / CTC_DIVISOR,
                },
            );
        }
        for name in accounts.keys() {
            results.entry(name.clone()).or_insert(StakingReward::zero());
        }

        Ok(results)
    }
}

fn parse_reward_points_def(val: Value<u32>) -> (f64, HashMap<[u8; 32], f64>) {
    let mut total = 0.0;
    let mut map = HashMap::new();
    if let ValueDef::Composite(Composite::Named(fields)) = val.value {
        for (name, field) in fields {
            if name == "total" {
                if let ValueDef::Primitive(Primitive::U128(t)) = field.value {
                    total = t as f64;
                }
            } else if name == "individual" {
                if let ValueDef::Composite(Composite::Unnamed(outer_items)) = field.value {
                    let validator_list: &[Value<u32>] = if outer_items.len() == 1 {
                        if let ValueDef::Composite(Composite::Unnamed(ref inner)) =
                            outer_items[0].value
                        {
                            inner.as_slice()
                        } else {
                            outer_items.as_slice()
                        }
                    } else {
                        outer_items.as_slice()
                    };

                    for item in validator_list {
                        if let ValueDef::Composite(Composite::Unnamed(ref pair)) = item.value {
                            if pair.len() == 2 {
                                if let Some(id) = extract_account_id_from_value(&pair[0]) {
                                    if let ValueDef::Primitive(Primitive::U128(pts)) = pair[1].value
                                    {
                                        map.insert(id, pts as f64);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    (total, map)
}
fn parse_commission_def(val: Value<u32>) -> f64 {
    if let ValueDef::Composite(Composite::Named(fields)) = val.value {
        for (name, field) in fields {
            if name == "commission" {
                if let ValueDef::Primitive(Primitive::U128(com)) = field.value {
                    return com as f64 / 1_000_000_000.0;
                }
            }
        }
    }
    0.0
}

fn parse_exposure_def(val: Value<u32>) -> (f64, f64, Vec<([u8; 32], f64)>, u32) {
    let mut total = 0.0;
    let mut own = 0.0;
    let mut others = Vec::new();
    let mut page_count = 0u32;

    if let ValueDef::Composite(Composite::Named(fields)) = val.value {
        for (name, field) in fields {
            match name.as_str() {
                "total" => {
                    if let ValueDef::Primitive(Primitive::U128(t)) = field.value {
                        total = t as f64;
                    }
                }
                "own" => {
                    if let ValueDef::Primitive(Primitive::U128(o)) = field.value {
                        own = o as f64;
                    }
                }
                "page_count" => {
                    if let ValueDef::Primitive(Primitive::U128(p)) = field.value {
                        page_count = p as u32;
                    }
                }
                "others" => {
                    // Legacy ErasStakersClipped format - has inline nominators
                    if let ValueDef::Composite(Composite::Unnamed(items)) = field.value {
                        let nominator_list: &[Value<u32>] = if items.len() == 1 {
                            if let ValueDef::Composite(Composite::Unnamed(ref inner)) =
                                items[0].value
                            {
                                inner.as_slice()
                            } else {
                                items.as_slice()
                            }
                        } else {
                            items.as_slice()
                        };

                        for item in nominator_list {
                            if let ValueDef::Composite(Composite::Named(ifields)) = &item.value {
                                let mut who = None;
                                let mut value = 0.0;
                                for (iname, ifield) in ifields {
                                    if iname == "who" {
                                        who = extract_account_id_from_value(ifield);
                                    } else if iname == "value" {
                                        if let ValueDef::Primitive(Primitive::U128(v)) =
                                            ifield.value
                                        {
                                            value = v as f64;
                                        }
                                    }
                                }
                                if let Some(w) = who {
                                    others.push((w, value));
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    (total, own, others, page_count)
}

/// Parse a single page from ErasStakersPaged - returns list of (nominator_id, stake_value)
fn parse_paged_exposure(val: Value<u32>) -> Vec<([u8; 32], f64)> {
    let mut nominators = Vec::new();

    // ErasStakersPaged structure: { page_total: u128, others: Vec<IndividualExposure> }
    if let ValueDef::Composite(Composite::Named(fields)) = val.value {
        for (name, field) in fields {
            if name == "others" {
                if let ValueDef::Composite(Composite::Unnamed(items)) = field.value {
                    // Handle potential wrapper layer
                    let nominator_list: &[Value<u32>] = if items.len() == 1 {
                        if let ValueDef::Composite(Composite::Unnamed(ref inner)) = items[0].value {
                            inner.as_slice()
                        } else {
                            items.as_slice()
                        }
                    } else {
                        items.as_slice()
                    };

                    for item in nominator_list {
                        if let ValueDef::Composite(Composite::Named(ifields)) = &item.value {
                            let mut who = None;
                            let mut value = 0.0;
                            for (iname, ifield) in ifields {
                                if iname == "who" {
                                    who = extract_account_id_from_value(ifield);
                                } else if iname == "value" {
                                    if let ValueDef::Primitive(Primitive::U128(v)) = ifield.value {
                                        value = v as f64;
                                    }
                                }
                            }
                            if let Some(w) = who {
                                nominators.push((w, value));
                            }
                        }
                    }
                }
            }
        }
    }
    nominators
}

fn extract_account_id_from_value(mut val: &Value<u32>) -> Option<[u8; 32]> {
    loop {
        match &val.value {
            ValueDef::Composite(Composite::Unnamed(items)) => {
                if items.len() == 32 {
                    let mut bytes = [0u8; 32];
                    for (i, v) in items.iter().enumerate() {
                        if let ValueDef::Primitive(Primitive::U128(b)) = v.value {
                            bytes[i] = b as u8;
                        } else {
                            return None;
                        }
                    }
                    return Some(bytes);
                } else if items.len() == 1 {
                    val = &items[0];
                    continue;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }
}
