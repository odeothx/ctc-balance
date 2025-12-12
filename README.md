# CTC Balance Tracker

A high-performance tool to track and visualize Creditcoin3 (CTC) wallet balances history using Substrate RPC.

## Features

- **Historical Data**: Tracks daily balances from Genesis (2024-08-29) or specified date range.
- **Optimized Performance**: 
  - **Parallel Execution**: Uses multi-processing for both block finding and balance fetching.
  - **Smart Caching**: Caches daily block numbers and genesis info to minimize RPC calls.
  - **Smart Search**: Uses interpolation search to quickly locate blocks by timestamp.
- **Incremental Updates**: Intelligently appends new data to existing CSV history files.
- **Visualization**: Automatically generates beautiful balance history graphs.
- **Robustness**: Includes retry logic and connection pooling for reliable data fetching.

## Prerequisites

- **Python 3.12+**
- **uv** (recommended for package management)

## Installation

This project uses `uv` for dependency management.

```bash
# Clone the repository
git clone <repo-url>
cd ctc-balance

# Install dependencies
uv sync
```

Alternatively, using pip:
```bash
pip install -r requirements.txt
```

## Usage

### 1. Prepare Accounts File
Create a text file (e.g., `my_accounts.txt`) with your accounts.
Format: `AccountName Address` (space separated).

Example `my_accounts.txt`:
```text
WalletA 5...
WalletB 5...
```

### 2. Run Tracker

**Basic Run (Track entire history):**
```bash
uv run main.py -f my_accounts.txt
```

**Generate Graph:**
```bash
uv run main.py -f my_accounts.txt --graph
```

**Specify Date Range:**
```bash
uv run main.py -f my_accounts.txt --start 2024-10-01 --end 2024-12-31
```

**Track Single Address (without file):**
```bash
uv run main.py -a 5XXXX... -n MyMainWallet
```

## Output

Results are saved in the `output/` directory:

- **`{name}_history.csv`**: Daily balance records (appended on each run).
- **`{name}_history.png`**: Visualization of balance history (if `--graph` is used).
- **`block_cache.json`**: Cache of Date-to-Block mappings (do not delete to speed up future runs).

## Configuration

You can adjust internal parameters in `main.py` if needed (e.g., worker counts), though defaults are optimized for general use.
