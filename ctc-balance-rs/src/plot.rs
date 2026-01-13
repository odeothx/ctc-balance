//! Graph generation module using plotters.
//!
//! Generates PNG graphs for balance history visualization.

use anyhow::{Context, Result};
use chrono::NaiveDate;
use plotters::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Generate main balance graph (combined + total)
pub fn plot_balances<P: AsRef<Path>>(
    output_file: P,
    dates: &[String],
    all_history: &HashMap<String, HashMap<String, f64>>,
    account_names: &[String],
    source_name: &str,
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

    // Create the main graph (2-panel)
    let png_path = path.with_extension("png");
    {
        let root = BitMapBackend::new(&png_path, (1400, 1000)).into_drawing_area();
        root.fill(&WHITE)?;

        let (upper, lower) = root.split_vertically(500);

        // Title
        root.draw(&Text::new(
            format!("CTC Balance History - {}", source_name),
            (700, 20),
            ("sans-serif", 24).into_font().color(&BLACK),
        ))?;

        // Upper panel: Individual balances
        {
            let x_range =
                date_objects.first().unwrap().clone()..date_objects.last().unwrap().clone();
            let y_max = max_individual * 1.1;

            let mut chart = ChartBuilder::on(&upper)
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

        // Lower panel: Total balance
        {
            let x_range =
                date_objects.first().unwrap().clone()..date_objects.last().unwrap().clone();
            let y_max = max_total * 1.1;

            let mut chart = ChartBuilder::on(&lower)
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

        let root = BitMapBackend::new(&individual_path, (1200, 600)).into_drawing_area();
        root.fill(&WHITE)?;

        let x_range = date_objects.first().unwrap().clone()..date_objects.last().unwrap().clone();

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
