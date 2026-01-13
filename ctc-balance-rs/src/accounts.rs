//! Account file loading module.
//!
//! Supports two formats:
//! - `Name = Address`
//! - `Name Address`

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Load accounts from a text file
///
/// Supports two formats:
/// - `Name = Address`
/// - `Name Address` (space-separated)
///
/// Lines starting with `#` are treated as comments.
pub fn load_accounts<P: AsRef<Path>>(file_path: P) -> Result<HashMap<String, String>> {
    let path = file_path.as_ref();
    let file = File::open(path).context(format!("Accounts file not found: {:?}", path))?;
    let reader = BufReader::new(file);

    let mut accounts = HashMap::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse "name = address" or "name address" format
        if line.contains('=') {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                let name = parts[0].trim().to_string();
                let address = parts[1].trim().to_string();
                accounts.insert(name, address);
            }
        } else {
            // Space-separated format
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0].to_string();
                let address = parts[1].to_string();
                accounts.insert(name, address);
            }
        }
    }

    Ok(accounts)
}
