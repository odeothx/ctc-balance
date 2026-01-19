//! Subscan API client for fetching staking rewards.
//!
//! Uses the Subscan API to efficiently fetch reward data instead of scanning blocks.

use anyhow::{Context, Result};
use chrono::{NaiveDate, TimeZone, Utc};
use serde::Deserialize;
use std::collections::HashMap;

use crate::CTC_DECIMALS;

const SUBSCAN_API_URL: &str = "https://creditcoin.api.subscan.io";

/// Subscan API reward/slash response
#[derive(Debug, Deserialize)]
struct RewardSlashResponse {
    code: i32,
    message: String,
    data: Option<RewardSlashData>,
}

#[derive(Debug, Deserialize)]
struct RewardSlashData {
    #[allow(dead_code)]
    count: u64,
    list: Option<Vec<RewardItem>>,
}

#[derive(Debug, Deserialize)]
struct RewardItem {
    #[allow(dead_code)]
    stash: String,
    amount: String,
    block_timestamp: i64,
    event_id: String,  // v2 API uses event_id instead of event_method
}

/// Subscan account search response
#[derive(Debug, Deserialize)]
struct AccountSearchResponse {
    code: i32,
    data: Option<AccountSearchData>,
}

#[derive(Debug, Deserialize)]
struct AccountSearchData {
    account: Option<AccountInfo>,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    stash: Option<String>,
}

/// Subscan API client
pub struct SubscanClient {
    client: reqwest::Client,
    base_url: String,
}

impl SubscanClient {
    /// Create a new Subscan client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: SUBSCAN_API_URL.to_string(),
        }
    }

    /// Get the stash address for an account (if it's a controller or nominator)
    pub async fn get_stash_address(&self, address: &str) -> Result<String> {
        let url = format!("{}/api/v2/scan/search", self.base_url);

        let body = serde_json::json!({
            "key": address
        });

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send search request to Subscan")?;

        let result: AccountSearchResponse = response
            .json()
            .await
            .context("Failed to parse account search response")?;

        if result.code != 0 {
            return Ok(address.to_string());
        }

        if let Some(data) = result.data {
            if let Some(account) = data.account {
                if let Some(stash) = account.stash {
                    if !stash.is_empty() {
                        return Ok(stash);
                    }
                }
            }
        }

        // Return original address if no stash found
        Ok(address.to_string())
    }

    /// Get rewards for a single account within a date range
    pub async fn get_rewards_for_account(
        &self,
        address: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<f64> {
        let start_ts = Utc
            .from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap())
            .timestamp();
        let end_ts = Utc
            .from_utc_datetime(&end_date.and_hms_opt(23, 59, 59).unwrap())
            .timestamp();

        let mut total_reward: u128 = 0;
        let mut page = 0;
        let row = 100;

        loop {
            let response = self
                .fetch_reward_page(address, page, row)
                .await
                .context("Failed to fetch reward page")?;

            if response.code != 0 {
                anyhow::bail!("Subscan API error: {}", response.message);
            }

            let data = match response.data {
                Some(d) => d,
                None => break,
            };

            let list = match data.list {
                Some(l) => l,
                None => break,
            };

            if list.is_empty() {
                break;
            }

            let mut found_older = false;
            for item in &list {
                // Only count Rewarded events (not slashes)
                if item.event_id != "Rewarded" {
                    continue;
                }

                // Check if within date range
                if item.block_timestamp >= start_ts && item.block_timestamp <= end_ts {
                    if let Ok(amt) = item.amount.parse::<u128>() {
                        total_reward += amt;
                    }
                }

                // If we've gone past the start date, we can stop
                if item.block_timestamp < start_ts {
                    found_older = true;
                }
            }

            // If we found items older than start date, stop paginating
            if found_older {
                break;
            }

            // If we got fewer items than requested, no more pages
            if list.len() < row {
                break;
            }

            page += 1;

            // Safety limit
            if page > 1000 {
                break;
            }
        }

        let divisor = 10u128.pow(CTC_DECIMALS) as f64;
        Ok(total_reward as f64 / divisor)
    }

    /// Get rewards for multiple accounts within a date range
    pub async fn get_all_rewards(
        &self,
        accounts: &HashMap<String, String>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<HashMap<String, f64>> {
        let mut results = HashMap::new();

        for (name, address) in accounts {
            let reward = self
                .get_rewards_for_account(address, start_date, end_date)
                .await
                .unwrap_or(0.0);
            results.insert(name.clone(), reward);
        }

        Ok(results)
    }

    /// Get daily rewards for an account over a date range
    pub async fn get_daily_rewards(
        &self,
        address: &str,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<HashMap<String, f64>> {
        let start_ts = Utc
            .from_utc_datetime(&start_date.and_hms_opt(0, 0, 0).unwrap())
            .timestamp();
        let end_ts = Utc
            .from_utc_datetime(&end_date.and_hms_opt(23, 59, 59).unwrap())
            .timestamp();

        let mut daily_rewards: HashMap<String, u128> = HashMap::new();
        let mut page = 0;
        let row = 100;

        loop {
            let response = self.fetch_reward_page(address, page, row).await?;

            if response.code != 0 {
                break;
            }

            let data = match response.data {
                Some(d) => d,
                None => break,
            };

            let list = match data.list {
                Some(l) => l,
                None => break,
            };

            if list.is_empty() {
                break;
            }

            let mut found_older = false;
            for item in &list {
                if item.event_id != "Rewarded" {
                    continue;
                }

                if item.block_timestamp >= start_ts && item.block_timestamp <= end_ts {
                    let date = Utc
                        .timestamp_opt(item.block_timestamp, 0)
                        .unwrap()
                        .format("%Y-%m-%d")
                        .to_string();

                    if let Ok(amt) = item.amount.parse::<u128>() {
                        *daily_rewards.entry(date).or_insert(0) += amt;
                    }
                }

                if item.block_timestamp < start_ts {
                    found_older = true;
                }
            }

            if found_older {
                break;
            }

            if list.len() < row {
                break;
            }

            page += 1;

            if page > 1000 {
                break;
            }
        }

        let divisor = 10u128.pow(CTC_DECIMALS) as f64;
        let result: HashMap<String, f64> = daily_rewards
            .into_iter()
            .map(|(date, amt)| (date, amt as f64 / divisor))
            .collect();

        Ok(result)
    }

    /// Get daily rewards for all accounts over a date range
    /// Note: Subscan returns rewards indexed by stash address, so we first resolve
    /// controller addresses to their stash addresses.
    pub async fn get_all_daily_rewards(
        &self,
        accounts: &HashMap<String, String>,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<HashMap<String, HashMap<String, f64>>> {
        let mut results: HashMap<String, HashMap<String, f64>> = HashMap::new();

        for (name, address) in accounts {
            // First, resolve to stash address (Subscan returns rewards by stash)
            let stash_address = self.get_stash_address(address).await.unwrap_or(address.clone());
            
            let addr_display = if stash_address != *address {
                format!("{} (stash: {}...)", name, &stash_address[..12])
            } else {
                name.clone()
            };
            println!("  Fetching rewards for {} via Subscan API...", addr_display);
            
            let daily = self
                .get_daily_rewards(&stash_address, start_date, end_date)
                .await
                .unwrap_or_default();
            results.insert(name.clone(), daily);
        }

        Ok(results)
    }

    async fn fetch_reward_page(
        &self,
        address: &str,
        page: usize,
        row: usize,
    ) -> Result<RewardSlashResponse> {
        // Use v2 API for complete staking reward data
        let url = format!("{}/api/v2/scan/account/reward_slash", self.base_url);

        let body = serde_json::json!({
            "address": address,
            "page": page,
            "row": row
        });

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Subscan")?;

        let result: RewardSlashResponse = response
            .json()
            .await
            .context("Failed to parse Subscan response")?;

        Ok(result)
    }
}

impl Default for SubscanClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subscan_client_new() {
        let client = SubscanClient::new();
        assert_eq!(client.base_url, SUBSCAN_API_URL);
    }
}
