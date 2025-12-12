#!/usr/bin/env python3
"""
CTC Balance Tracker - Main Script (RPC Version with Optimization)

Tracks Creditcoin3 wallet balances from genesis to present,
using Substrate RPC with caching for speed optimization.

Usage:
    python main.py -f my_accounts.txt
    python main.py -f my_accounts.txt --graph
    python main.py -a 5ERZF3... -n MyWallet --start 2024-10-01
"""

import argparse
import csv
import json
import os
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from concurrent.futures import ProcessPoolExecutor, as_completed

import matplotlib
matplotlib.use('Agg')  # Non-interactive backend
import matplotlib.pyplot as plt
import matplotlib.dates as mdates

from accounts import load_accounts
from src.chain import ChainConnector
from src.balance import BalanceTracker


# Creditcoin3 메인넷 시작일 (Genesis: 2024-08-28)
GENESIS_DATE = date(2024, 8, 29)  # 블록 1부터 시작
OUTPUT_DIR = Path(__file__).parent / "output"
CACHE_FILE = OUTPUT_DIR / "block_cache.json"


def format_ctc(amount: float) -> str:
    """Format CTC amount with commas."""
    return f"{amount:,.1f}"


def load_block_cache() -> dict:
    """Load cached date->block mappings."""
    if CACHE_FILE.exists():
        with open(CACHE_FILE) as f:
            return json.load(f)
    return {}


def save_block_cache(cache: dict):
    """Save date->block mappings to cache."""
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    with open(CACHE_FILE, "w") as f:
        json.dump(cache, f)


def get_utc_midnight_timestamp(d: date) -> int:
    """Get Unix timestamp for UTC midnight of a date."""
    dt = datetime(d.year, d.month, d.day, 0, 0, 0, tzinfo=timezone.utc)
    return int(dt.timestamp())


# Global variable for worker process
worker_chain: ChainConnector | None = None


def init_worker():
    """Initialize worker process with a shared connection."""
    global worker_chain
    worker_chain = ChainConnector()


def _find_block_worker(d: date) -> tuple[date, int, str]:
    """Worker for finding block in parallel."""
    # use global connection
    if worker_chain is None:
        raise RuntimeError("Worker chain not initialized")
        
    # Retry loop
    for attempt in range(3):
        try:
            ts = get_utc_midnight_timestamp(d)
            block_num, block_hash = worker_chain.find_block_at_timestamp(ts)
            return d, block_num, block_hash
        except Exception as e:
            if attempt < 2:
                # Reconnect and retry
                try:
                    worker_chain.reconnect()
                except:
                    pass
            else:
                print(f"Error in _find_block_worker for {d}: {e}")
                raise


def _fetch_balance_worker(date_key: str, block_hash: str, accounts: dict) -> tuple[str, dict]:
    """Worker for fetching balances in parallel."""
    if worker_chain is None:
         raise RuntimeError("Worker chain not initialized")

    tracker = BalanceTracker(worker_chain)
    results = {}
    
    try:
        # returns {name: Balance}
        balances = tracker.get_all_balances(accounts, block_hash)
        results = {name: round(b.free, 1) for name, b in balances.items()}
        return date_key, results
    except Exception:
        # Return empty/zeros on fatal error, or re-raise
        return date_key, {name: 0.0 for name in accounts}


def parse_args():
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(
        description="CTC Balance Tracker - Track Creditcoin3 wallet balances (RPC)",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument("-f", "--file", help="Wallet addresses file")
    source.add_argument("-a", "--address", help="Single wallet address")
    
    parser.add_argument("-n", "--name", default="wallet", help="Name for single wallet")
    parser.add_argument("--start", type=lambda s: date.fromisoformat(s), default=GENESIS_DATE)
    parser.add_argument("--end", type=lambda s: date.fromisoformat(s), default=date.today())
    parser.add_argument("-o", "--output", help="Output CSV file")
    parser.add_argument("--graph", "-g", action="store_true", help="Generate graph")
    parser.add_argument("--no-cache", action="store_true", help="Ignore block cache")
    
    return parser.parse_args()


def find_blocks_for_dates(chain: ChainConnector, dates: list[date], cache: dict) -> dict:
    """Find block numbers for each date (with caching)."""
    results = {}
    dates_to_find = []
    
    for d in dates:
        key = d.isoformat()
        if key in cache:
            results[key] = cache[key]
        else:
            dates_to_find.append(d)
    
    if not dates_to_find:
        return results
    
    print(f"  Finding blocks for {len(dates_to_find)} uncached dates (Parallel)...")
    
    with ProcessPoolExecutor(max_workers=5, initializer=init_worker) as executor:
        future_to_date = {executor.submit(_find_block_worker, d): d for d in dates_to_find}
        
        completed_count = 0
        for future in as_completed(future_to_date):
            try:
                d, block_num, block_hash = future.result()
                results[d.isoformat()] = {"block": block_num, "hash": block_hash}
                cache[d.isoformat()] = {"block": block_num, "hash": block_hash}
            except Exception as e:
                d = future_to_date[future]
                print(f"    Error finding block for {d}: {e}")
            
            completed_count += 1
            if completed_count % 10 == 0 or completed_count == len(dates_to_find):
                print(f"    {completed_count}/{len(dates_to_find)} blocks found...")
                save_block_cache(cache)
    
    save_block_cache(cache)
    return results


def plot_balances(dates: list[str], all_history: dict, account_names: list[str], 
                  output_file: Path, source_name: str):
    """Generate and save balance graph."""
    date_objects = [datetime.strptime(d, "%Y-%m-%d") for d in dates]
    
    fig, (ax1, ax2) = plt.subplots(2, 1, figsize=(14, 10))
    fig.suptitle(f"CTC Balance History - {source_name}", fontsize=14, fontweight='bold')
    
    colors = plt.cm.tab10.colors
    
    # 상단: 개별 계정
    for i, name in enumerate(account_names):
        balances = [all_history.get(name, {}).get(d, 0.0) for d in dates]
        ax1.plot(date_objects, balances, label=name, color=colors[i % len(colors)], linewidth=1.5)
    
    ax1.set_ylabel("Balance (CTC)")
    ax1.set_title("Individual Account Balances")
    ax1.legend(loc='upper left', fontsize=9, ncol=2)
    ax1.grid(True, alpha=0.3)
    ax1.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m'))
    ax1.xaxis.set_major_locator(mdates.MonthLocator())
    plt.setp(ax1.xaxis.get_majorticklabels(), rotation=45, ha='right')
    ax1.yaxis.set_major_formatter(plt.FuncFormatter(lambda x, p: f'{x:,.0f}'))
    
    # 하단: 총 잔고
    totals = [sum(all_history.get(n, {}).get(d, 0.0) for n in account_names) for d in dates]
    ax2.fill_between(date_objects, totals, alpha=0.3, color='blue')
    ax2.plot(date_objects, totals, color='blue', linewidth=2)
    ax2.set_xlabel("Date")
    ax2.set_ylabel("Total Balance (CTC)")
    ax2.set_title("Total Balance Over Time")
    ax2.grid(True, alpha=0.3)
    ax2.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m'))
    ax2.xaxis.set_major_locator(mdates.MonthLocator())
    plt.setp(ax2.xaxis.get_majorticklabels(), rotation=45, ha='right')
    ax2.yaxis.set_major_formatter(plt.FuncFormatter(lambda x, p: f'{x:,.0f}'))
    
    if totals:
        ax2.annotate(f'{totals[0]:,.0f}', xy=(date_objects[0], totals[0]), fontsize=9)
        ax2.annotate(f'{totals[-1]:,.0f}', xy=(date_objects[-1], totals[-1]), fontsize=9, ha='right')
    
    plt.tight_layout()
    graph_file = output_file.with_suffix('.png')
    plt.savefig(graph_file, dpi=150, bbox_inches='tight')
    plt.close()
    return graph_file


def main():
    args = parse_args()
    
    print("=" * 60)
    print("CTC Balance Tracker - RPC Version")
    print("=" * 60)

    # 1. 계정 로드
    print("\n[1/5] Loading accounts...")
    if args.file:
        file_path = Path(args.file)
        if not file_path.exists():
            file_path = Path(__file__).parent / args.file
        accounts = load_accounts(file_path)
        source_name = file_path.stem
        print(f"  Loaded: {len(accounts)} accounts from {file_path.name}")
    else:
        accounts = {args.name: args.address}
        source_name = args.name
        print(f"  Single wallet: {args.name}")

    # 2. 체인 연결
    print("\n[2/5] Connecting to RPC...")
    chain = ChainConnector()
    tracker = BalanceTracker(chain)
    
    try:
        info = chain.get_chain_info()
        print(f"  Chain: {info['chain']} v{info['version']}")
    except Exception as e:
        print(f"  ERROR: {e}")
        return 1

    # 3. 날짜 범위 및 블록 캐시
    print("\n[3/5] Finding blocks for dates...")
    start_date = args.start
    end_date = args.end
    
    dates = []
    current = start_date
    while current <= end_date:
        dates.append(current)
        current += timedelta(days=1)
    
    print(f"  Date range: {start_date} ~ {end_date} ({len(dates)} days)")
    
    cache = {} if args.no_cache else load_block_cache()
    block_map = find_blocks_for_dates(chain, dates, cache)

    # 4. 잔고 조회 (병렬 처리)
    print(f"\n[4/5] Fetching balances (Parallel)...")
    all_history = {name: {} for name in accounts}
    
    tasks = []
    # 처리할 작업 목록 생성
    for d in dates:
        key = d.isoformat()
        block_info = block_map.get(key)
        
        if block_info:
            tasks.append((key, block_info["hash"]))

    with ProcessPoolExecutor(max_workers=20, initializer=init_worker) as executor:
        future_to_task = {
            executor.submit(_fetch_balance_worker, key, block_hash, accounts): key 
            for key, block_hash in tasks
        }
        
        completed = 0
        total_tasks = len(tasks)
        
        for future in as_completed(future_to_task):
            key, balances = future.result()
            
            # 결과 병합
            for name, bal in balances.items():
                all_history[name][key] = bal
                
            completed += 1
            if completed % 10 == 0 or completed == total_tasks:
                # 진행 상황 표시 (첫 번째 계정의 합계로 대략적인 표시)
                total = sum(balances.values())
                print(f"  [{completed}/{total_tasks}] {key} completed")

    # 5. 결과 저장
    print(f"\n[5/5] Saving results...")
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    
    output_file = Path(args.output) if args.output else OUTPUT_DIR / f"{source_name}_history.csv"
    
    account_names = sorted(accounts.keys())
    
    # Load existing data if file exists
    existing_data = {}
    if output_file.exists():
        print(f"  Loading existing data from {output_file.name}...")
        try:
            with open(output_file, "r") as f:
                reader = csv.DictReader(f)
                for row in reader:
                    d_key = row["date"]
                    # Convert values to float
                    data = {}
                    for k, v in row.items():
                        if k not in ["date", "total"]:
                            try:
                                data[k] = float(v)
                            except ValueError:
                                data[k] = 0.0
                    existing_data[d_key] = data
        except Exception as e:
            print(f"    Warning: Could not read existing file: {e}")

    # Merge new data into existing data
    for date_key, balances in all_history.items():
        existing_data[date_key] = balances

    header = ["date"] + account_names + ["total"]
    
    rows = []
    # Sort by date
    all_date_keys = sorted(existing_data.keys())
    
    for key in all_date_keys:
        row = [key]
        total = 0.0
        row_data = existing_data[key]
        for name in account_names:
            bal = row_data.get(name, 0.0)
            row.append(bal)
            total += bal
        row.append(round(total, 1))
        rows.append(row)
    
    with open(output_file, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(header)
        writer.writerows(rows)
    
    print(f"  CSV: {output_file}")
    
    if args.graph and rows:
        print("  Generating graph...")
        graph_file = plot_balances(date_keys, all_history, account_names, output_file, source_name)
        print(f"  Graph: {graph_file}")
    
    if rows:
        latest = rows[-1]
        print(f"\n  Latest ({latest[0]}): {format_ctc(latest[-1])} CTC")
    
    print("\n" + "=" * 60)
    print("COMPLETED!")
    print("=" * 60)
    return 0


if __name__ == "__main__":
    exit(main())
