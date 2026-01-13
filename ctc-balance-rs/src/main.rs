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
    cache::{load_block_cache, save_block_cache, BlockCache},
    chain::ChainConnector,
    csv_output::{
        calculate_diffs, load_existing_csv, save_combined_csv, save_individual_csvs, HistoryEntry,
    },
    plot::plot_balances,
    GENESIS_DATE, NODE_URL,
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

    /// Ignore block cache
    #[arg(long)]
    no_cache: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("{}", "=".repeat(60));
    println!("CTC Balance Tracker - Rust Version");
    println!("{}", "=".repeat(60));

    // 1. Load accounts
    println!("\n[1/5] Loading accounts...");
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
    println!("\n[2/5] Connecting to RPC...");
    let mut chain = ChainConnector::new(Some(NODE_URL));
    chain.connect().await?;

    let info = chain.get_chain_info().await?;
    println!("  Chain: {}", info);

    // 3. Find blocks for dates
    println!("\n[3/5] Finding blocks for dates...");
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

    // Generate date range
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

    // Load cache
    let output_dir = PathBuf::from("output");
    let cache_file = output_dir.join("block_cache.json");
    let mut cache: BlockCache = if args.no_cache {
        HashMap::new()
    } else {
        load_block_cache(&cache_file).unwrap_or_default()
    };

    // Find uncached dates
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

        for (i, d) in dates_to_find.iter().enumerate() {
            let date_str = d.format("%Y-%m-%d").to_string();

            // Get UTC midnight timestamp
            let timestamp = d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() as u64;

            match chain.find_block_at_timestamp(timestamp, 60).await {
                Ok(block_info) => {
                    cache.insert(date_str, block_info);
                }
                Err(e) => {
                    eprintln!("    Error finding block for {}: {}", d, e);
                }
            }

            if (i + 1) % 10 == 0 || i + 1 == dates_to_find.len() {
                println!("    {}/{} blocks found...", i + 1, dates_to_find.len());
                save_block_cache(&cache_file, &cache)?;
            }
        }

        save_block_cache(&cache_file, &cache)?;
    }

    // 4. Fetch balances
    println!("\n[4/5] Fetching balances...");
    let output_file = args
        .output
        .clone()
        .unwrap_or_else(|| output_dir.join(format!("{}_history.csv", source_name)));

    // Load existing data
    let mut existing_data = load_existing_csv(&output_file).unwrap_or_default();

    // Find dates that need balance queries
    let account_names: Vec<String> = {
        let mut names: Vec<_> = accounts.keys().cloned().collect();
        names.sort();
        names
    };

    let dates_to_fetch: Vec<String> = dates
        .iter()
        .map(|d| d.format("%Y-%m-%d").to_string())
        .filter(|date_str| {
            // Check if all accounts have data for this date
            !account_names.iter().all(|name| {
                existing_data
                    .get(name)
                    .map(|h| h.contains_key(date_str))
                    .unwrap_or(false)
            })
        })
        .collect();

    if !dates_to_fetch.is_empty() {
        println!(
            "  Loading existing data, fetching {} new dates...",
            dates_to_fetch.len()
        );

        let mut tracker = BalanceTracker::new(NODE_URL);
        tracker.connect().await?;

        for (i, date_str) in dates_to_fetch.iter().enumerate() {
            if let Some(block_info) = cache.get(date_str) {
                match tracker.get_all_balances(&accounts, &block_info.hash).await {
                    Ok(balances) => {
                        for (name, balance) in balances {
                            existing_data
                                .entry(name)
                                .or_insert_with(HashMap::new)
                                .insert(date_str.clone(), balance.free);
                        }
                    }
                    Err(e) => {
                        eprintln!("    Error fetching balances for {}: {}", date_str, e);
                    }
                }
            }

            if (i + 1) % 10 == 0 || i + 1 == dates_to_fetch.len() {
                println!(
                    "  [{}/{}] {} completed",
                    i + 1,
                    dates_to_fetch.len(),
                    date_str
                );
            }
        }
    }

    // 5. Save results
    println!("\n[5/5] Saving results...");
    std::fs::create_dir_all(&output_dir)?;

    // Build sorted date list
    let all_dates: Vec<String> = {
        let mut dates_set: std::collections::HashSet<String> = dates
            .iter()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .collect();

        // Add any existing dates
        for account_data in existing_data.values() {
            for date in account_data.keys() {
                dates_set.insert(date.clone());
            }
        }

        let mut dates_vec: Vec<String> = dates_set.into_iter().collect();
        dates_vec.sort();
        dates_vec
    };

    // Build history entries
    let mut entries: Vec<HistoryEntry> = all_dates
        .iter()
        .map(|date| {
            let mut balances = HashMap::new();
            let mut total = 0.0;

            for name in &account_names {
                let balance = existing_data
                    .get(name)
                    .and_then(|h| h.get(date))
                    .copied()
                    .unwrap_or(0.0);
                balances.insert(name.clone(), balance);
                total += balance;
            }

            HistoryEntry {
                date: date.clone(),
                balances,
                total,
                diff: 0.0,
                diff_avg10: 0.0,
            }
        })
        .collect();

    // Calculate diffs
    calculate_diffs(&mut entries);

    // Save combined CSV
    save_combined_csv(&output_file, &account_names, &entries)?;
    println!("  CSV (Combined): {:?}", output_file);

    // Save individual CSVs
    let individual_dir = output_dir.join("individual");
    save_individual_csvs(&individual_dir, &account_names, &existing_data, &all_dates)?;
    println!(
        "  CSV (Individual): {} files in {:?}",
        account_names.len(),
        individual_dir
    );

    // Generate graphs
    if args.graph && !entries.is_empty() {
        println!("  Generating graphs...");

        // Convert existing_data to the format expected by plot_balances
        let graph_files = plot_balances(
            &output_file,
            &all_dates,
            &existing_data,
            &account_names,
            &source_name,
        )?;

        if !graph_files.is_empty() {
            println!("  Main graph: {:?}", graph_files[0]);
            println!(
                "  Individual graphs: {} files in {:?}",
                graph_files.len() - 1,
                individual_dir
            );
        }
    }

    // Print latest balance
    if let Some(latest) = entries.last() {
        println!("\n  Latest ({}): {:.1} CTC", latest.date, latest.total);
    }

    println!("\n{}", "=".repeat(60));
    println!("COMPLETED!");
    println!("{}", "=".repeat(60));

    Ok(())
}
