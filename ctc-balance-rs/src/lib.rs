//! CTC Balance Tracker - Rust Implementation
//!
//! Tracks Creditcoin3 wallet balances from genesis to present.

pub mod accounts;
pub mod balance;
pub mod cache;
pub mod chain;
pub mod csv_output;
pub mod plot;

pub use accounts::load_accounts;
pub use balance::{Balance, BalanceTracker};
pub use cache::{load_block_cache, save_block_cache, BlockCache};
pub use chain::ChainConnector;

/// Creditcoin3 mainnet genesis date (2024-08-29)
pub const GENESIS_DATE: &str = "2024-08-29";

/// CTC decimals (18)
pub const CTC_DECIMALS: u32 = 18;

/// Block time in seconds
pub const BLOCK_TIME_SECONDS: u64 = 15;

/// Default RPC URL
pub const NODE_URL: &str = "wss://mainnet3.creditcoin.network";
