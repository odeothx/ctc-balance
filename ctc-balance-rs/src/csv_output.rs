//! CSV output module.
//!
//! Generates combined and individual CSV files for balance history.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

/// Balance history entry
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub date: String,
    pub balances: HashMap<String, f64>,
    pub total: f64,
    pub diff: f64,
    pub diff_avg10: f64,
    // Reward fields
    pub rewards: HashMap<String, f64>,
    pub total_reward: f64,
    pub reward_avg10: f64,
    pub total_reward_cumulative: f64,
}

/// Save combined CSV with all accounts
pub fn save_combined_csv<P: AsRef<Path>>(
    output_file: P,
    account_names: &[String],
    entries: &[HistoryEntry],
    include_rewards: bool,
) -> Result<()> {
    let path = output_file.as_ref();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create output directory")?;
    }

    let mut file = File::create(path).context("Failed to create CSV file")?;

    // Write header
    let mut header = vec!["date".to_string()];
    for name in account_names {
        header.push(name.clone());
    }
    header.extend([
        "total".to_string(),
        "diff".to_string(),
        "diff_avg10".to_string(),
    ]);

    // Add reward columns if enabled
    if include_rewards {
        for name in account_names {
            header.push(format!("{}_reward", name));
        }
        header.extend([
            "total_reward".to_string(),
            "reward_avg10".to_string(),
            "total_reward_cumulative".to_string(),
        ]);
    }
    writeln!(file, "{}", header.join(","))?;

    // Write data rows
    for entry in entries {
        let mut row = vec![entry.date.clone()];

        for name in account_names {
            let balance = entry.balances.get(name).unwrap_or(&0.0);
            row.push(format!("{:.1}", balance));
        }

        row.push(format!("{:.1}", entry.total));
        row.push(format!("{:.1}", entry.diff));
        row.push(format!("{:.1}", entry.diff_avg10));

        // Add reward data if enabled
        if include_rewards {
            for name in account_names {
                let reward = entry.rewards.get(name).unwrap_or(&0.0);
                row.push(format!("{:.4}", reward));
            }
            row.push(format!("{:.4}", entry.total_reward));
            row.push(format!("{:.4}", entry.reward_avg10));
            row.push(format!("{:.4}", entry.total_reward_cumulative));
        }

        writeln!(file, "{}", row.join(","))?;
    }

    Ok(())
}

/// Save individual CSV files for each account
pub fn save_individual_csvs<P: AsRef<Path>>(
    output_dir: P,
    account_names: &[String],
    all_history: &HashMap<String, HashMap<String, f64>>,
    sorted_dates: &[String],
    reward_history: Option<&HashMap<String, HashMap<String, f64>>>, // account_name -> date -> reward
) -> Result<()> {
    let dir = output_dir.as_ref();
    fs::create_dir_all(dir).context("Failed to create individual directory")?;

    let include_rewards = reward_history.is_some();

    for name in account_names {
        let csv_path = dir.join(format!("{}.csv", name));
        let mut file =
            File::create(&csv_path).context(format!("Failed to create {:?}", csv_path))?;

        // Write header
        if include_rewards {
            writeln!(
                file,
                "date,balance,diff,diff_avg10,reward,reward_avg10,reward_cumulative"
            )?;
        } else {
            writeln!(file, "date,balance,diff,diff_avg10")?;
        }

        let account_history = all_history.get(name);
        let account_rewards = reward_history.and_then(|r| r.get(name));

        let mut prev_balance: Option<f64> = None;
        let mut diffs: Vec<f64> = Vec::new();
        let mut rewards_for_avg: Vec<f64> = Vec::new();
        let mut reward_cumulative: f64 = 0.0;

        for date in sorted_dates {
            let balance = account_history
                .and_then(|h| h.get(date))
                .copied()
                .unwrap_or(0.0);

            // Calculate balance diff
            let diff = match prev_balance {
                Some(prev) => balance - prev,
                None => 0.0,
            };
            diffs.push(diff);

            // Calculate 10-day average for balance
            let diff_avg10 = if diffs.len() >= 10 {
                let last_10: f64 = diffs.iter().rev().take(10).sum();
                last_10 / 10.0
            } else if !diffs.is_empty() {
                diffs.iter().sum::<f64>() / diffs.len() as f64
            } else {
                0.0
            };

            if include_rewards {
                let reward = account_rewards
                    .and_then(|r| r.get(date))
                    .copied()
                    .unwrap_or(0.0);

                reward_cumulative += reward;

                rewards_for_avg.push(reward);

                // Calculate 10-day average for reward
                let reward_avg10 = if rewards_for_avg.len() >= 10 {
                    let last_10: f64 = rewards_for_avg.iter().rev().take(10).sum();
                    last_10 / 10.0
                } else if !rewards_for_avg.is_empty() {
                    rewards_for_avg.iter().sum::<f64>() / rewards_for_avg.len() as f64
                } else {
                    0.0
                };

                writeln!(
                    file,
                    "{},{:.1},{:.1},{:.1},{:.4},{:.4},{:.4}",
                    date, balance, diff, diff_avg10, reward, reward_avg10, reward_cumulative
                )?;
            } else {
                writeln!(
                    file,
                    "{},{:.1},{:.1},{:.1}",
                    date, balance, diff, diff_avg10
                )?;
            }

            prev_balance = Some(balance);
        }
    }

    Ok(())
}

/// Load existing CSV data to merge with new data
pub fn load_existing_csv<P: AsRef<Path>>(
    csv_file: P,
) -> Result<HashMap<String, HashMap<String, f64>>> {
    let path = csv_file.as_ref();

    if !path.exists() {
        return Ok(HashMap::new());
    }

    let mut reader = csv::Reader::from_path(path).context("Failed to open CSV")?;
    let headers: Vec<String> = reader
        .headers()
        .context("Failed to read headers")?
        .iter()
        .map(|s| s.to_string())
        .collect();

    let mut existing_data: HashMap<String, HashMap<String, f64>> = HashMap::new();

    for result in reader.records() {
        let record = result?;
        let date = record.get(0).unwrap_or("").to_string();

        if date.is_empty() {
            continue;
        }

        for (i, header) in headers.iter().enumerate().skip(1) {
            // Skip date, total, diff, diff_avg10
            if header == "total" || header == "diff" || header == "diff_avg10" {
                continue;
            }

            if let Some(value_str) = record.get(i) {
                if let Ok(value) = value_str.parse::<f64>() {
                    existing_data
                        .entry(header.clone())
                        .or_insert_with(HashMap::new)
                        .insert(date.clone(), value);
                }
            }
        }
    }

    Ok(existing_data)
}

/// Calculate diff and diff_avg10 for entries
pub fn calculate_diffs(entries: &mut Vec<HistoryEntry>) {
    let mut diffs: Vec<f64> = Vec::new();
    let mut prev_total: Option<f64> = None;

    for entry in entries.iter_mut() {
        // Calculate diff
        entry.diff = match prev_total {
            Some(prev) => entry.total - prev,
            None => 0.0,
        };
        diffs.push(entry.diff);

        // Calculate 10-day average
        entry.diff_avg10 = if diffs.len() >= 10 {
            let last_10: f64 = diffs.iter().rev().take(10).sum();
            last_10 / 10.0
        } else if !diffs.is_empty() {
            diffs.iter().sum::<f64>() / diffs.len() as f64
        } else {
            0.0
        };

        prev_total = Some(entry.total);
    }
}
