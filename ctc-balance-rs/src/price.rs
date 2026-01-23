use anyhow::{anyhow, Result};
use std::collections::HashMap;

/// Fetch the current CTC price in USD from CoinGecko
pub async fn fetch_ctc_price() -> Result<f64> {
    let url = "https://api.coingecko.com/api/v3/simple/price?ids=creditcoin-2&vs_currencies=usd";

    let client = reqwest::Client::builder()
        .user_agent("ctc-balance-tracker/0.1.0")
        .build()?;

    let response = client.get(url).send().await?;

    if !response.status().is_success() {
        return Err(anyhow!(
            "Failed to fetch price from CoinGecko: {}",
            response.status()
        ));
    }

    // Response format: {"creditcoin-2": {"usd": 0.262483}}
    let data: HashMap<String, HashMap<String, f64>> = response.json().await?;

    data.get("creditcoin-2")
        .and_then(|price_map| price_map.get("usd"))
        .copied()
        .ok_or_else(|| {
            anyhow!(
                "Price data for creditcoin-2 not found in response: {:?}",
                data
            )
        })
}
