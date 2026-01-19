//! Block cache management module.
//!
//! Caches date->block mappings in JSON format for performance.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::chain::BlockInfo;

/// Block cache type alias
pub type BlockCache = HashMap<String, BlockInfo>;

/// Load block cache from JSON file
pub fn load_block_cache<P: AsRef<Path>>(cache_file: P) -> Result<BlockCache> {
    let path = cache_file.as_ref();

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let file = File::open(path).context("Failed to open cache file")?;
    let reader = BufReader::new(file);

    let cache: BlockCache = serde_json::from_reader(reader).context("Failed to parse cache")?;

    Ok(cache)
}

/// Save block cache to JSON file
pub fn save_block_cache<P: AsRef<Path>>(cache_file: P, cache: &BlockCache) -> Result<()> {
    let path = cache_file.as_ref();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create cache directory")?;
    }

    let file = File::create(path).context("Failed to create cache file")?;
    let writer = BufWriter::new(file);

    serde_json::to_writer(writer, cache).context("Failed to write cache")?;

    Ok(())
}

/// Merge new entries into existing cache
pub fn merge_cache(cache: &mut BlockCache, new_entries: BlockCache) {
    for (date, info) in new_entries {
        cache.insert(date, info);
    }
}

/// Get cached block info for a date
pub fn get_cached_block<'a>(cache: &'a BlockCache, date: &str) -> Option<&'a BlockInfo> {
    cache.get(date)
}

// ============================================================================
// Reward Cache
// ============================================================================

/// Reward cache type: account_name -> date -> reward_amount
pub type RewardCache = HashMap<String, HashMap<String, f64>>;

/// Load reward cache from JSON file
pub fn load_reward_cache<P: AsRef<Path>>(cache_file: P) -> Result<RewardCache> {
    let path = cache_file.as_ref();

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let file = File::open(path).context("Failed to open reward cache file")?;
    let reader = BufReader::new(file);

    let cache: RewardCache =
        serde_json::from_reader(reader).context("Failed to parse reward cache")?;

    Ok(cache)
}

/// Save reward cache to JSON file
pub fn save_reward_cache<P: AsRef<Path>>(cache_file: P, cache: &RewardCache) -> Result<()> {
    let path = cache_file.as_ref();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create cache directory")?;
    }

    let file = File::create(path).context("Failed to create reward cache file")?;
    let writer = BufWriter::new(file);

    serde_json::to_writer(writer, cache).context("Failed to write reward cache")?;

    Ok(())
}

/// Merge new reward entries into existing cache
pub fn merge_reward_cache(cache: &mut RewardCache, new_entries: RewardCache) {
    for (account, date_rewards) in new_entries {
        let account_cache = cache.entry(account).or_insert_with(HashMap::new);
        for (date, reward) in date_rewards {
            account_cache.insert(date, reward);
        }
    }
}

/// Get cached reward for an account and date
pub fn get_cached_reward(cache: &RewardCache, account: &str, date: &str) -> Option<f64> {
    cache
        .get(account)
        .and_then(|dates| dates.get(date).copied())
}
