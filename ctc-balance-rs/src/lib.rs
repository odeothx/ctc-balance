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

/// Block time in seconds
pub const BLOCK_TIME_SECONDS: u64 = 15;

/// Default RPC URL
pub const NODE_URL: &str = "wss://mainnet3.creditcoin.network";

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
                            "Operation failed after {} retries: {}",
                            max_retries,
                            e
                        ));
                    }
                    retry_count += 1;
                    let delay = 100 * 2u64.pow(retry_count as u32);
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
            }
        }
    }};
}
