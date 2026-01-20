//! Graph generation module using plotters.
//!
//! Generates PNG graphs for balance history visualization.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use plotters::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Generate main balance graph (combined + total + rewards if available)
pub fn plot_balances<P: AsRef<Path>>(
    output_file: P,
    dates: &[String],
    all_history: &HashMap<String, HashMap<String, f64>>,
    account_names: &[String],
    source_name: &str,
    reward_history: Option<&HashMap<String, f64>>, // date -> total_reward
) -> Result<Vec<std::path::PathBuf>> {
    let path = output_file.as_ref();
    let mut generated_files = Vec::new();

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("Failed to create output directory")?;
    }

    // Parse dates
    let date_objects: Vec<NaiveDate> = dates
        .iter()
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();

    if date_objects.is_empty() {
        return Ok(generated_files);
    }

    // Colors for accounts
    let colors = [
        RGBColor(31, 119, 180),  // Blue
        RGBColor(255, 127, 14),  // Orange
        RGBColor(44, 160, 44),   // Green
        RGBColor(214, 39, 40),   // Red
        RGBColor(148, 103, 189), // Purple
        RGBColor(140, 86, 75),   // Brown
        RGBColor(227, 119, 194), // Pink
        RGBColor(127, 127, 127), // Gray
        RGBColor(188, 189, 34),  // Olive
        RGBColor(23, 190, 207),  // Cyan
    ];

    // Calculate totals
    let totals: Vec<f64> = dates
        .iter()
        .map(|d| {
            account_names
                .iter()
                .map(|name| {
                    all_history
                        .get(name)
                        .and_then(|h| h.get(d))
                        .copied()
                        .unwrap_or(0.0)
                })
                .sum()
        })
        .collect();

    // Find max values for Y axis
    let max_individual: f64 = account_names
        .iter()
        .flat_map(|name| {
            all_history
                .get(name)
                .map(|h| h.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .fold(0.0f64, |a, b| a.max(b));

    let max_total: f64 = totals.iter().cloned().fold(0.0f64, |a, b| a.max(b));

    // Determine if we have reward data
    let has_rewards = reward_history.is_some();
    let graph_height = if has_rewards { 1400 } else { 1000 };

    // Create the main graph (2 or 3-panel)
    let png_path = path.with_extension("png");
    {
        let root = BitMapBackend::new(&png_path, (1400, graph_height)).into_drawing_area();
        root.fill(&WHITE)?;

        let panels = if has_rewards {
            // 3-panel layout: top (400), middle (400), bottom (500)
            let (top_mid, bottom) = root.split_vertically((graph_height as u32 * 6) / 10);
            let (upper, lower) = top_mid.split_vertically((graph_height as u32 * 3) / 10);
            (upper, lower, Some(bottom))
        } else {
            let (upper, lower) = root.split_vertically(500);
            (upper, lower, None)
        };

        // Title
        root.draw(&Text::new(
            format!("CTC Balance History - {}", source_name),
            (700, 20),
            ("sans-serif", 24).into_font().color(&BLACK),
        ))?;

        // Upper panel: Individual balances
        {
            let x_range = if date_objects.len() > 1 {
                date_objects.first().unwrap().clone()..date_objects.last().unwrap().clone()
            } else {
                let d = date_objects[0];
                d.pred_opt().unwrap_or(d)..d.succ_opt().unwrap_or(d)
            };
            let y_max = max_individual * 1.1;

            let mut chart = ChartBuilder::on(&panels.0)
                .margin(40)
                .x_label_area_size(30)
                .y_label_area_size(80)
                .caption("Individual Account Balances", ("sans-serif", 18))
                .build_cartesian_2d(x_range.clone(), 0.0..y_max)?;

            chart
                .configure_mesh()
                .x_labels(12)
                .y_labels(10)
                .y_label_formatter(&|v| format_ctc(*v))
                .draw()?;

            // Draw each account
            for (i, name) in account_names.iter().enumerate() {
                let color = colors[i % colors.len()];

                let data: Vec<(NaiveDate, f64)> = date_objects
                    .iter()
                    .zip(dates.iter())
                    .filter_map(|(date_obj, date_str)| {
                        all_history
                            .get(name)
                            .and_then(|h| h.get(date_str))
                            .map(|&v| (date_obj.clone(), v))
                    })
                    .collect();

                chart
                    .draw_series(LineSeries::new(data, color.stroke_width(2)))?
                    .label(name)
                    .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
            }

            chart
                .configure_series_labels()
                .position(SeriesLabelPosition::UpperLeft)
                .background_style(&WHITE.mix(0.8))
                .border_style(&BLACK)
                .draw()?;
        }

        // Middle panel: Total balance
        {
            let x_range =
                date_objects.first().unwrap().clone()..date_objects.last().unwrap().clone();
            let y_max = max_total * 1.1;

            let mut chart = ChartBuilder::on(&panels.1)
                .margin(40)
                .x_label_area_size(30)
                .y_label_area_size(80)
                .caption("Total Balance Over Time", ("sans-serif", 18))
                .build_cartesian_2d(x_range, 0.0..y_max)?;

            chart
                .configure_mesh()
                .x_labels(12)
                .y_labels(10)
                .y_label_formatter(&|v| format_ctc(*v))
                .draw()?;

            let total_data: Vec<(NaiveDate, f64)> = date_objects
                .iter()
                .cloned()
                .zip(totals.iter().cloned())
                .collect();

            // Area fill
            chart.draw_series(AreaSeries::new(total_data.clone(), 0.0, BLUE.mix(0.3)))?;

            // Line
            chart.draw_series(LineSeries::new(total_data, BLUE.stroke_width(2)))?;
        }

        // Bottom panel: Daily rewards (if available)
        if let (Some(reward_data), Some(bottom_panel)) = (reward_history, panels.2) {
            let rewards: Vec<f64> = dates
                .iter()
                .map(|d| reward_data.get(d).copied().unwrap_or(0.0))
                .collect();

            let max_reward = rewards.iter().cloned().fold(0.0f64, |a, b| a.max(b)) * 1.2;
            let max_reward = if max_reward <= 0.0 { 1.0 } else { max_reward };

            let x_range = if date_objects.len() > 1 {
                date_objects.first().unwrap().clone()..date_objects.last().unwrap().clone()
            } else {
                let d = date_objects[0];
                d.pred_opt().unwrap_or(d)..d.succ_opt().unwrap_or(d)
            };

            let mut chart = ChartBuilder::on(&bottom_panel)
                .margin(40)
                .x_label_area_size(30)
                .y_label_area_size(80)
                .caption("Daily Staking Rewards", ("sans-serif", 18))
                .build_cartesian_2d(x_range, 0.0..max_reward)?;

            chart
                .configure_mesh()
                .x_labels(12)
                .y_labels(10)
                .y_label_formatter(&|v| format!("{:.2}", v))
                .draw()?;

            // Draw bars for each day
            let bar_color = RGBColor(76, 175, 80); // Green

            chart.draw_series(
                date_objects
                    .iter()
                    .zip(rewards.iter())
                    .filter(|(_, r)| **r > 0.0)
                    .map(|(date, reward)| {
                        let x0 = *date;
                        let x1 = date.succ_opt().unwrap_or(*date);
                        Rectangle::new([(x0, 0.0), (x1, *reward)], bar_color.filled())
                    }),
            )?;
        }

        root.present()?;
    }
    generated_files.push(png_path);

    // Create individual graphs
    let individual_dir = path.parent().unwrap_or(Path::new(".")).join("individual");
    fs::create_dir_all(&individual_dir)?;

    for (i, name) in account_names.iter().enumerate() {
        let individual_path = individual_dir.join(format!("{}.png", name));
        let individual_path_clone = individual_path.clone();
        let color = colors[i % colors.len()];

        let balances: Vec<f64> = dates
            .iter()
            .map(|d| {
                all_history
                    .get(name)
                    .and_then(|h| h.get(d))
                    .copied()
                    .unwrap_or(0.0)
            })
            .collect();

        let max_balance = balances.iter().cloned().fold(0.0f64, |a, b| a.max(b)) * 1.1;
        if max_balance <= 0.0 {
            continue;
        }

        // Check if we have reward data for this account
        let has_account_rewards = reward_history.is_some();
        let graph_height = if has_account_rewards { 900 } else { 600 };

        let root = BitMapBackend::new(&individual_path, (1200, graph_height)).into_drawing_area();
        root.fill(&WHITE)?;

        let x_range = if date_objects.len() > 1 {
            date_objects.first().unwrap().clone()..date_objects.last().unwrap().clone()
        } else {
            let d = date_objects[0];
            d.pred_opt().unwrap_or(d)..d.succ_opt().unwrap_or(d)
        };

        if has_account_rewards {
            // 2-panel layout: balance on top, reward on bottom
            let (upper, lower) = root.split_vertically(500);

            // Upper panel: Balance
            {
                let mut chart = ChartBuilder::on(&upper)
                    .margin(40)
                    .x_label_area_size(40)
                    .y_label_area_size(80)
                    .caption(
                        format!("CTC Balance History - {}", name),
                        ("sans-serif", 20),
                    )
                    .build_cartesian_2d(x_range.clone(), 0.0..max_balance)?;

                chart
                    .configure_mesh()
                    .x_labels(12)
                    .y_labels(10)
                    .y_label_formatter(&|v| format_ctc(*v))
                    .draw()?;

                let data: Vec<(NaiveDate, f64)> = date_objects
                    .iter()
                    .cloned()
                    .zip(balances.iter().cloned())
                    .collect();

                // Area fill
                chart.draw_series(AreaSeries::new(data.clone(), 0.0, color.mix(0.3)))?;

                // Line
                chart.draw_series(LineSeries::new(data, color.stroke_width(2)))?;
            }

            // Lower panel: Rewards (using total rewards for this date)
            if let Some(reward_data) = reward_history {
                let rewards: Vec<f64> = dates
                    .iter()
                    .map(|d| reward_data.get(d).copied().unwrap_or(0.0))
                    .collect();

                let max_reward = rewards.iter().cloned().fold(0.0f64, |a, b| a.max(b)) * 1.2;
                let max_reward = if max_reward <= 0.0 { 1.0 } else { max_reward };

                let mut chart = ChartBuilder::on(&lower)
                    .margin(40)
                    .x_label_area_size(40)
                    .y_label_area_size(80)
                    .caption(
                        format!("Daily Staking Rewards - {}", name),
                        ("sans-serif", 18),
                    )
                    .build_cartesian_2d(x_range.clone(), 0.0..max_reward)?;

                chart
                    .configure_mesh()
                    .x_labels(12)
                    .y_labels(8)
                    .y_label_formatter(&|v| format!("{:.2}", v))
                    .draw()?;

                // Draw bars for each day
                let bar_color = RGBColor(76, 175, 80); // Green

                chart.draw_series(
                    date_objects
                        .iter()
                        .zip(rewards.iter())
                        .filter(|(_, r)| **r > 0.0)
                        .map(|(date, reward)| {
                            let x0 = *date;
                            let x1 = date.succ_opt().unwrap_or(*date);
                            Rectangle::new([(x0, 0.0), (x1, *reward)], bar_color.filled())
                        }),
                )?;
            }
        } else {
            // Single panel: Balance only
            let mut chart = ChartBuilder::on(&root)
                .margin(40)
                .x_label_area_size(40)
                .y_label_area_size(80)
                .caption(
                    format!("CTC Balance History - {}", name),
                    ("sans-serif", 20),
                )
                .build_cartesian_2d(x_range, 0.0..max_balance)?;

            chart
                .configure_mesh()
                .x_labels(12)
                .y_labels(10)
                .y_label_formatter(&|v| format_ctc(*v))
                .draw()?;

            let data: Vec<(NaiveDate, f64)> = date_objects
                .iter()
                .cloned()
                .zip(balances.iter().cloned())
                .collect();

            // Area fill
            chart.draw_series(AreaSeries::new(data.clone(), 0.0, color.mix(0.3)))?;

            // Line
            chart.draw_series(LineSeries::new(data, color.stroke_width(2)))?;
        }

        root.present()?;
        generated_files.push(individual_path_clone);
    }

    Ok(generated_files)
}

/// Format CTC amount with commas
fn format_ctc(amount: f64) -> String {
    let formatted = format!("{:.0}", amount);
    let chars: Vec<char> = formatted.chars().collect();
    let mut result = String::new();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }

    result
}
