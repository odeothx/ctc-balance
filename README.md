# CTC Balance Tracker

A high-performance tool to track and visualize Creditcoin3 (CTC) wallet balances history using Substrate RPC.

## Features

- **Historical Data**: Tracks daily balances from Genesis (2024-08-29) or a specified date range.
- **Optimized Performance**: 
  - **Parallel Execution**: Uses multi-processing for both block finding (multi-worker) and balance fetching (up to 20 workers).
  - **Smart Caching**: Caches daily block numbers and hashes to `block_cache.json` to minimize RPC calls.
  - **Efficient RPC Interaction**: Uses a dedicated `ChainConnector` with connection pooling and retry logic.
- **Incremental Updates**: Intelligently merges new data with existing CSV history files without duplicates.
- **Advanced Visualization**: 
  - Generates a combined total balance graph.
  - Generates individual balance graphs for every tracked account.
- **Detailed Analytics**: Calculates daily differences (`diff`) and 10-day moving averages (`diff_avg10`).

## Prerequisites

- **Python 3.12+**
- **uv** (recommended for high-performance package management)

## Installation

This project is optimized for use with `uv`.

```bash
# Clone the repository
git clone <repo-url>
cd ctc-balance

# Install dependencies and setup venv
uv sync
```

Alternatively, using standard pip:
```bash
pip install -r requirements.txt
```

## Usage

### 1. Prepare Accounts File
Create a text file (e.g., `my_accounts.txt`) listing your accounts.
Two formats are supported:
- `AccountName Address` (space separated)
- `AccountName = Address` (equals separated)

Example `my_accounts.txt`:
```text
# Staking Wallet
MyWallet 5ERZF3...
# Exchange Wallet
Exchange = 5GXXXX...
```

### 2. Run Tracker

**Track entire history (incremental by default):**
```bash
uv run main.py -f my_accounts.txt
```

**Generate Graphs (Combined & Individual):**
```bash
uv run main.py -f my_accounts.txt --graph
```

**Specify Date Range:**
```bash
uv run main.py -f my_accounts.txt --start 2024-10-01 --end 2024-12-31
```

**Track Single Address (without file):**
```bash
uv run main.py -a 5XXXX... -n MyMainWallet --graph
```

## Output

Results are saved in the `output/` directory:

- **`{name}_history.csv`**: Daily balance records.
  - Columns: `date`, `[Account Names...]`, `total`, `diff`, `diff_avg10`
- **`{name}_history.png`**: Combined visualization of all accounts and total balance.
- **`individual/`**: Directory containing per-account balance graphs (e.g., `WalletA.png`).
- **`block_cache.json`**: Cache of Date-to-Block mappings. Do not delete this to maintain fast performance on future runs.

## Technical Details

The tool uses `substrate-interface` to communicate with Creditcoin3 RPC nodes. It employs a two-stage parallel process:
1. **Block Finding**: Locate UTC midnight blocks for each date in the range.
2. **Balance Fetching**: Query the state at those specific block hashes across multiple worker processes.

Configuration such as `max_workers` and `GENESIS_DATE` can be adjusted in `main.py`.
