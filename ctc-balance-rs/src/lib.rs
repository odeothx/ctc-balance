//! CTC Balance Tracker - Rust Implementation
//!
//! Tracks Creditcoin3 wallet balances from genesis to present.

pub mod accounts;
pub mod balance;
pub mod cache;
pub mod chain;
pub mod csv_output;
pub mod plot;
pub mod reward;
pub use accounts::load_accounts;
pub use balance::{Balance, BalanceTracker};
pub use cache::{
    load_block_cache, load_reward_cache, save_block_cache, save_reward_cache, BlockCache,
    RewardCache,
};
pub use chain::ChainConnector;
pub use reward::{RewardTracker, StakingReward};

/// Creditcoin3 mainnet genesis date (2024-08-29)
pub const GENESIS_DATE: &str = "2024-08-29";

/// CTC decimals (18)
pub const CTC_DECIMALS: u32 = 18;

/// CTC divisor for f64 conversion
pub const CTC_DIVISOR: f64 = 1_000_000_000_000_000_000.0;

/// Block time in seconds
pub const BLOCK_TIME_SECONDS: u64 = 15;

/// Default RPC URL
pub const NODE_URL: &str = "wss://mainnet3.creditcoin.network";

/// Concurrency: Number of dates to process in parallel for block finding
pub const CONCURRENCY_DATES: usize = 5;

/// Concurrency: Number of dates to process in parallel for balances
pub const CONCURRENCY_BALANCES: usize = 3;

/// Concurrency: Number of dates to process in parallel for rewards
pub const CONCURRENCY_REWARDS: usize = 2;

/// Concurrency: Number of accounts/storage queries in parallel within a date
pub const CONCURRENCY_STORAGE: usize = 10;

/// Concurrency: Number of blocks to scan in parallel for event fallback
pub const CONCURRENCY_EVENTS: usize = 50;

/// Concurrency: Number of validator exposures to fetch in parallel
pub const CONCURRENCY_EXPOSURES: usize = 20;

/// Parse SS58 address to AccountId32
pub fn parse_ss58_address(address: &str) -> anyhow::Result<subxt::utils::AccountId32> {
    use std::str::FromStr;
    subxt::utils::AccountId32::from_str(address)
        .map_err(|e| anyhow::anyhow!("Invalid SS58 address '{}': {}", address, e))
}

/// Centralized retry macro with exponential backoff
#[macro_export]
macro_rules! retry {
    ($logic:expr) => {{
        let mut retry_count = 0;
        let max_retries = 3;
        loop {
            match $logic.await {
                Ok(val) => break Ok(val),
                Err(e) => {
                    if retry_count >= max_retries {
                        break Err(anyhow::anyhow!(
                            "Operation failed after {} retries. Last error: {}",
                            max_retries,
                            e
                        ));
                    }
                    retry_count += 1;
                    // Exponential backoff: 250ms, 500ms, 1000ms
                    let delay = 125 * 2u64.pow(retry_count as u32);
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }};
}
