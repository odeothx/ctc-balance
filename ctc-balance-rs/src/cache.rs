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
