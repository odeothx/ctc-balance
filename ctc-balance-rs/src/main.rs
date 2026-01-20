//! CTC Balance Tracker - Main Entry Point
//!
//! Tracks Creditcoin3 wallet balances from genesis to present.

use anyhow::Result;
use chrono::{Days, NaiveDate, Utc};
use clap::Parser;
use std::collections::HashMap;
use std::path::PathBuf;

use ctc_balance::{
    accounts::load_accounts,
    balance::BalanceTracker,
    cache::{
        load_block_cache, load_reward_cache, save_block_cache, save_reward_cache, BlockCache,
        RewardCache,
    },
    chain::ChainConnector,
    csv_output::{
        calculate_diffs, load_existing_csv, save_combined_csv, save_individual_csvs, HistoryEntry,
    },
    plot::plot_balances,
    reward::RewardTracker,
    CONCURRENCY_BALANCES, CONCURRENCY_DATES, CONCURRENCY_REWARDS, GENESIS_DATE, NODE_URL,
};

/// CTC Balance Tracker - Track Creditcoin3 wallet balances
#[derive(Parser, Debug)]
#[command(name = "ctc-balance")]
#[command(about = "Track Creditcoin3 wallet balances from genesis to present")]
struct Args {
    /// Wallet addresses file
    #[arg(short, long)]
    file: Option<PathBuf>,

    /// Single wallet address
    #[arg(short, long)]
    address: Option<String>,

    /// Name for single wallet
    #[arg(short, long, default_value = "wallet")]
    name: String,

    /// Start date (YYYY-MM-DD)
    #[arg(long)]
    start: Option<String>,

    /// End date (YYYY-MM-DD)
    #[arg(long)]
    end: Option<String>,

    /// Output CSV file
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Generate graph
    #[arg(short, long)]
    graph: bool,

    /// Skip staking rewards fetching
    #[arg(long)]
    no_rewards: bool,

    /// Local RPC URL for recent blocks (faster, may have pruned data)
    #[arg(long)]
    local_rpc: Option<String>,

    /// Ignore caches
    #[arg(long)]
    no_cache: bool,

    /// Re-fetch and overwrite entries with zero balance
    #[arg(long)]
    refetch_zero: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("{}", "=".repeat(60));
    println!("CTC Balance Tracker - Rust Version");
    println!("{}", "=".repeat(60));

    // 1. Load accounts
    println!("\n[1/6] Loading accounts...");
    let (accounts, source_name) = if let Some(file_path) = &args.file {
        let accts = load_accounts(file_path)?;
        let name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("accounts")
            .to_string();
        println!("  Loaded: {} accounts from {:?}", accts.len(), file_path);
        (accts, name)
    } else if let Some(address) = &args.address {
        let mut accts = HashMap::new();
        accts.insert(args.name.clone(), address.clone());
        println!("  Single wallet: {}", args.name);
        (accts, args.name.clone())
    } else {
        anyhow::bail!("Either --file or --address must be specified");
    };

    // 2. Connect to chain
    println!("\n[2/6] Connecting to RPC...");
    let mut chain = ChainConnector::new(Some(NODE_URL));
    chain.connect().await?;

    let info = chain.get_chain_info().await?;
    println!("  Remote RPC: {} ({})", NODE_URL, info);

    // Connect to local RPC if provided and detect first block
    let local_first_block: Option<u64> = if let Some(local_url) = &args.local_rpc {
        let mut local_chain = ChainConnector::new(Some(local_url));
        if local_chain.connect().await.is_ok() {
            let latest = chain.get_latest_block_number().await.unwrap_or(0);
            let first_block = detect_first_block(local_url, latest).await;
            if first_block > 0 {
                println!(
                    "  Local RPC: {} (Archived from block: {})",
                    local_url, first_block
                );
            } else {
                println!("  Local RPC: {} (Full history detected)", local_url);
            }
            Some(first_block)
        } else {
            println!("  Warning: Failed to connect to local RPC: {}", local_url);
            None
        }
    } else {
        None
    };

    let local_rpc_url = args.local_rpc.clone();
    let latest_block = chain.get_latest_block_number().await.unwrap_or(0);

    // 3. Find blocks for dates
    println!("\n[3/6] Finding blocks for dates...");
    let start_date = args
        .start
        .as_ref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()?
        .unwrap_or_else(|| NaiveDate::parse_from_str(GENESIS_DATE, "%Y-%m-%d").unwrap());

    let end_date = args
        .end
        .as_ref()
        .map(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d"))
        .transpose()?
        .unwrap_or_else(|| Utc::now().date_naive());

    let mut dates: Vec<NaiveDate> = Vec::new();
    let mut current = start_date;
    while current <= end_date {
        dates.push(current);
        current = current.checked_add_days(Days::new(1)).unwrap();
    }

    println!(
        "  Date range: {} ~ {} ({} days)",
        start_date,
        end_date,
        dates.len()
    );

    let output_dir = PathBuf::from("output");
    let cache_file = output_dir.join("block_cache.json");
    let mut cache: BlockCache = if args.no_cache {
        HashMap::new()
    } else {
        load_block_cache(&cache_file).unwrap_or_default()
    };

    let dates_to_find: Vec<NaiveDate> = dates
        .iter()
        .filter(|d| !cache.contains_key(&d.format("%Y-%m-%d").to_string()))
        .cloned()
        .collect();

    if !dates_to_find.is_empty() {
        println!(
            "  Finding blocks for {} uncached dates...",
            dates_to_find.len()
        );
        use futures::stream::{self, StreamExt};
        let client = chain.client().ok().cloned();

        let mut stream = stream::iter(dates_to_find.iter())
            .map(|&d| {
                let client = client.clone();
                let date_str = d.format("%Y-%m-%d").to_string();
                let timestamp = d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as u64;
                async move {
                    // Create a temporary connector that reuses the client
                    let mut temp_chain = ChainConnector::new(Some(NODE_URL));
                    if let Some(c) = client {
                        temp_chain.set_client(c);
                    }
                    let res = temp_chain.find_block_at_timestamp(timestamp, 60).await;
                    (date_str, res)
                }
            })
            .buffer_unordered(CONCURRENCY_DATES);

        let mut count = 0;
        while let Some((date_str, res)) = stream.next().await {
            if let Ok(block_info) = res {
                cache.insert(date_str, block_info);
            }
            count += 1;
            if count % 10 == 0 || count == dates_to_find.len() {
                println!("    {}/{} blocks found...", count, dates_to_find.len());
                save_block_cache(&cache_file, &cache)?;
            }
        }
        save_block_cache(&cache_file, &cache)?;
    }

    // 4. Fetch balances
    println!("\n[4/6] Fetching balances...");
    let output_file = args
        .output
        .clone()
        .unwrap_or_else(|| output_dir.join(format!("{}_history.csv", source_name)));
    let mut existing_data = load_existing_csv(&output_file).unwrap_or_default();
    let account_names: Vec<String> = {
        let mut names: Vec<_> = accounts.keys().cloned().collect();
        names.sort();
        names
    };

    let dates_to_fetch: Vec<String> = dates
        .iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .filter(|date_str| {
            // 1. Always fetch if any account is missing data for this date
            let any_missing = account_names.iter().any(|name| {
                existing_data
                    .get(name)
                    .and_then(|h| h.get(date_str))
                    .is_none()
            });
            if any_missing {
                return true;
            }

            // 2. If refetch_zero is enabled, fetch if ALL accounts have 0.0 balance
            // This avoids re-fetching dates where some accounts legitimately have 0.0
            if args.refetch_zero {
                let all_zero = account_names.iter().all(|name| {
                    existing_data
                        .get(name)
                        .and_then(|h| h.get(date_str))
                        .map(|&v| v == 0.0)
                        .unwrap_or(true)
                });
                if all_zero {
                    return true;
                }
            }

            false
        })
        .collect();

    if !dates_to_fetch.is_empty() {
        println!("  Fetching {} new dates...", dates_to_fetch.len());
        use futures::stream::{self, StreamExt};
        let client = chain.client().ok().cloned();
        let mut stream = stream::iter(dates_to_fetch.iter())
            .map(|date_str| {
                let client = client.clone();
                let date_str = date_str.clone();
                let accounts = accounts.clone();
                let block_info = cache.get(&date_str).cloned();
                async move {
                    if let Some(block_info) = block_info {
                        let mut tracker = BalanceTracker::new(NODE_URL);
                        if let Some(c) = client {
                            tracker.set_client((*c).clone());
                        }
                        let res = tracker.get_all_balances(&accounts, &block_info.hash).await;
                        (date_str, Some(res))
                    } else {
                        (date_str, None)
                    }
                }
            })
            .buffer_unordered(CONCURRENCY_BALANCES);

        let mut count = 0;
        let mut failed_dates = Vec::new();
        while let Some((date_str, res_opt)) = stream.next().await {
            match res_opt {
                Some(Ok(balances)) => {
                    for (name, balance) in balances {
                        existing_data
                            .entry(name)
                            .or_insert_with(HashMap::new)
                            .insert(date_str.clone(), balance.free);
                    }
                }
                Some(Err(e)) => {
                    println!(
                        "    Warning: Failed to fetch balances for {}: {}",
                        date_str, e
                    );
                    failed_dates.push(date_str);
                }
                None => {
                    println!("    Warning: Missing block info for {}", date_str);
                    failed_dates.push(date_str);
                }
            }
            count += 1;
            if count % 10 == 0 || count == dates_to_fetch.len() {
                println!("  [{}/{}] completed", count, dates_to_fetch.len());
            }
        }

        if !failed_dates.is_empty() {
            println!(
                "\n  Caution: {} dates failed to fetch. These will appear as 0.0 in the output.",
                failed_dates.len()
            );
            println!("  Try running again to retry these dates.");
        }
    }

    // 5. Fetch staking rewards - BLOCK SCANNING
    let mut full_reward_history: RewardCache = HashMap::new();
    if !args.no_rewards {
        let reward_cache_file = output_dir.join("reward_cache.json");
        let mut reward_cache = if args.no_cache {
            HashMap::new()
        } else {
            load_reward_cache(&reward_cache_file).unwrap_or_default()
        };

        println!("\n[5/6] Fetching staking rewards (block scanning)...");
        let date_strings: Vec<String> = dates
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();
        let mut missing_date_block_ranges = Vec::new();

        for (i, date_str) in date_strings.iter().enumerate() {
            let mut all_present = true;
            for name in accounts.keys() {
                let present = reward_cache
                    .get(name)
                    .map(|h| h.contains_key(date_str))
                    .unwrap_or(false);

                if !present {
                    all_present = false;
                    break;
                }
            }

            if !all_present {
                if let Some(start_info) = cache.get(date_str) {
                    let next_block = date_strings
                        .get(i + 1)
                        .and_then(|next_date| cache.get(next_date))
                        .map(|b| b.block)
                        .unwrap_or(start_info.block + 5760);

                    // Cap end_block to current latest block to prevent scanning future blocks
                    let end_block = std::cmp::min(next_block, latest_block);

                    if end_block >= start_info.block {
                        missing_date_block_ranges.push((
                            date_str.clone(),
                            start_info.block,
                            end_block,
                        ));
                    }
                }
            }
        }

        if !missing_date_block_ranges.is_empty() {
            print!(
                "  Fetching rewards for {} uncached dates",
                missing_date_block_ranges.len()
            );
            if missing_date_block_ranges.len() <= 5 {
                let dates_list: Vec<_> = missing_date_block_ranges
                    .iter()
                    .map(|(d, _, _)| d.as_str())
                    .collect();
                print!(" ({})", dates_list.join(", "));
            }
            println!("...");

            use futures::stream::{self, StreamExt};
            let local_first = local_first_block;
            let local_url = local_rpc_url.clone();

            let client = chain.client().ok().cloned();
            let rpc = chain.rpc().ok().cloned();
            let mut stream = stream::iter(missing_date_block_ranges.iter())
                .map(|(date_str, start_block, end_block)| {
                    let rpc_url = match (&local_url, local_first) {
                        (Some(url), Some(first)) if *start_block >= first => url.clone(),
                        _ => NODE_URL.to_string(),
                    };
                    let mut tracker = RewardTracker::new(&rpc_url);
                    if rpc_url == NODE_URL {
                        if let Some(ref c) = client {
                            tracker.set_client((**c).clone());
                        }
                        if let Some(ref r) = rpc {
                            tracker.set_rpc((**r).clone());
                        }
                    }
                    let date_str = date_str.clone();
                    let start = *start_block;
                    let end = *end_block;
                    let accounts = accounts.clone();
                    async move {
                        if tracker.connect().await.is_ok() {
                            match tracker
                                .get_rewards_via_eras(&accounts, start, end)
                                .await
                            {
                                Ok(rewards) => (date_str, Some(rewards)),
                                Err(e) => {
                                    println!("    Warning: Era-based query failed for {}: {}. Falling back to scanning...", date_str, e);
                                    match tracker.get_all_rewards_in_range(&accounts, start, end).await {
                                        Ok(r) => (date_str, Some(r)),
                                        Err(_) => (date_str, None),
                                    }
                                },
                            }
                        } else {
                            (date_str, None)
                        }
                    }
                })
                .buffer_unordered(CONCURRENCY_REWARDS);

            let mut count = 0;
            while let Some((date_str, rewards_opt)) = stream.next().await {
                if let Some(rewards) = rewards_opt {
                    for (name, reward) in rewards {
                        reward_cache
                            .entry(name)
                            .or_insert_with(HashMap::new)
                            .insert(date_str.clone(), reward.claimed);
                    }
                }
                count += 1;
                println!(
                    "    [{}/{}] dates processed",
                    count,
                    missing_date_block_ranges.len()
                );
                save_reward_cache(&reward_cache_file, &reward_cache).ok();
            }
            save_reward_cache(&reward_cache_file, &reward_cache).ok();
        } else {
            println!("  All rewards found in cache!");
        }
        full_reward_history = reward_cache;
    }

    // 6. Save results
    println!("\n[6/6] Saving results...");
    let all_dates: Vec<String> = {
        let mut dates_set: std::collections::HashSet<String> = dates
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();
        for account_data in existing_data.values() {
            for date in account_data.keys() {
                dates_set.insert(date.clone());
            }
        }
        let mut dv: Vec<String> = dates_set.into_iter().collect();
        dv.sort();
        dv
    };

    let mut reward_cumulative = 0.0;
    let mut reward_history_for_avg: Vec<f64> = Vec::new();
    let mut daily_total_rewards: HashMap<String, f64> = HashMap::new();

    let mut entries: Vec<HistoryEntry> = all_dates
        .iter()
        .map(|date| {
            let mut balances = HashMap::new();
            let mut rewards = HashMap::new();
            let mut total = 0.0;
            let mut total_reward = 0.0;

            for name in &account_names {
                let balance = existing_data
                    .get(name)
                    .and_then(|h| h.get(date))
                    .copied()
                    .unwrap_or(0.0);
                balances.insert(name.clone(), balance);
                total += balance;

                let reward = full_reward_history
                    .get(name)
                    .and_then(|h| h.get(date))
                    .copied()
                    .unwrap_or(0.0);
                rewards.insert(name.clone(), reward);
                total_reward += reward;
            }

            daily_total_rewards.insert(date.clone(), total_reward);
            reward_cumulative += total_reward;
            reward_history_for_avg.push(total_reward);

            let reward_avg10 = if reward_history_for_avg.len() >= 10 {
                reward_history_for_avg.iter().rev().take(10).sum::<f64>() / 10.0
            } else if !reward_history_for_avg.is_empty() {
                reward_history_for_avg.iter().sum::<f64>() / reward_history_for_avg.len() as f64
            } else {
                0.0
            };

            HistoryEntry {
                date: date.clone(),
                balances,
                total,
                diff: 0.0,
                diff_avg10: 0.0,
                rewards,
                total_reward,
                reward_avg10,
                total_reward_cumulative: reward_cumulative,
            }
        })
        .collect();

    calculate_diffs(&mut entries);
    save_combined_csv(&output_file, &account_names, &entries, !args.no_rewards)?;

    let individual_dir = output_dir.join("individual");
    save_individual_csvs(
        &individual_dir,
        &account_names,
        &existing_data,
        &all_dates,
        if !args.no_rewards {
            Some(&full_reward_history)
        } else {
            None
        },
    )?;

    if args.graph && !entries.is_empty() {
        println!("  Generating graphs...");
        plot_balances(
            &output_file,
            &all_dates,
            &existing_data,
            &account_names,
            &source_name,
            if !args.no_rewards {
                Some(&daily_total_rewards)
            } else {
                None
            },
        )?;
    }

    if let Some(latest) = entries.last() {
        println!("\n  Latest ({}): {:.1} CTC", latest.date, latest.total);
    }

    println!("\n{}\nCOMPLETED!\n{}", "=".repeat(60), "=".repeat(60));
    Ok(())
}

async fn detect_first_block(url: &str, latest_block: u64) -> u64 {
    let mut tracker = RewardTracker::new(url);
    if tracker.connect().await.is_err() {
        return 0;
    }
    if tracker.has_events(0).await && tracker.has_events(1).await {
        return 0;
    }
    let mut low = 0u64;
    let mut high = latest_block;
    while low < high {
        let mid = (low + high) / 2;
        if mid == 0 {
            low = 1;
            continue;
        }
        if tracker.has_events(mid).await {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}
