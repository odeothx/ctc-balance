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
import logging
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from concurrent.futures import ProcessPoolExecutor, as_completed
from filelock import FileLock

import matplotlib
matplotlib.use('Agg')  # Non-interactive backend
import matplotlib.pyplot as plt
import matplotlib.dates as mdates

from accounts import load_accounts
from src.chain import ChainConnector
from src.balance import BalanceTracker
from src.reward import RewardTracker
from src import BLOCKS_PER_DAY


# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)

# Creditcoin3 메인넷 시작일 (Genesis: 2024-08-28)
GENESIS_DATE = date(2024, 8, 29)  # 블록 1부터 시작
OUTPUT_DIR = Path(__file__).parent / "output"
CACHE_FILE = OUTPUT_DIR / "block_cache.json"
REWARD_CACHE_FILE = OUTPUT_DIR / "reward_cache.json"

# Concurrency Constants (Matched with Rust version)
CONCURRENCY_DATES = 5
CONCURRENCY_BALANCES = 3


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
    """Save date->block mappings to cache with file locking."""
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    lock_file = CACHE_FILE.with_suffix('.lock')
    with FileLock(lock_file):
        with open(CACHE_FILE, "w") as f:
            json.dump(cache, f)


def load_reward_cache() -> dict:
    """Load cached rewards."""
    if REWARD_CACHE_FILE.exists():
        with open(REWARD_CACHE_FILE) as f:
            return json.load(f)
    return {}


def save_reward_cache(cache: dict):
    """Save rewards to cache with file locking."""
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    lock_file = REWARD_CACHE_FILE.with_suffix('.lock')
    with FileLock(lock_file):
        with open(REWARD_CACHE_FILE, "w") as f:
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
                except Exception:
                    pass
            else:
                logger.error(f"Error in _find_block_worker for {d}: {e}")
                raise


def _fetch_balance_worker(date_key: str, block_hash: str, accounts: dict, refetch_zero: bool = False) -> tuple[str, dict]:
    """Worker for fetching balances in parallel."""
    if worker_chain is None:
         raise RuntimeError("Worker chain not initialized")

    tracker = BalanceTracker(worker_chain)
    results = {}
    
    try:
        # returns {name: Balance}
        balances = tracker.get_all_balances(accounts, block_hash, force_refetch=refetch_zero)
        results = {name: b.free for name, b in balances.items()}
        return date_key, results
    except Exception as e:
        # Log the error instead of silently swallowing
        logger.error(f"Failed to fetch balances for {date_key}: {e}")
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
    parser.add_argument("--no-rewards", action="store_true", help="Skip fetching staking rewards")
    parser.add_argument("--refetch-zero", action="store_true", help="Re-fetch if balance is zero")
    
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
    
    with ProcessPoolExecutor(max_workers=CONCURRENCY_DATES, initializer=init_worker) as executor:
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


def plot_balances(dates: list[str], all_history: dict, reward_history: dict, account_names: list[str], 
                  output_file: Path, source_name: str):
    """Generate and save balance graphs (combined and individual)."""
    date_objects = [datetime.strptime(d, "%Y-%m-%d") for d in dates]
    colors = plt.cm.tab10.colors
    graph_files = []
    
    # === 메인 그래프 (전체 계정 + 총합) ===
    # Add a third subplot for rewards
    fig, (ax1, ax2, ax3) = plt.subplots(3, 1, figsize=(14, 15))
    fig.suptitle(f"CTC Balance & Rewards History - {source_name}", fontsize=14, fontweight='bold')
    
    # 1. Individual Account Balances
    for i, name in enumerate(account_names):
        balances = [all_history.get(name, {}).get(d, 0.0) for d in dates]
        ax1.plot(date_objects, balances, label=name, color=colors[i % len(colors)], linewidth=1.5)
    
    ax1.set_ylabel("Balance (CTC)")
    ax1.set_title("Individual Account Balances")
    ax1.legend(loc='upper left', fontsize=9, ncol=2)
    ax1.grid(True, alpha=0.3)
    ax1.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m'))
    ax1.xaxis.set_major_locator(mdates.MonthLocator() if len(dates) > 60 else mdates.DayLocator(interval=max(1, len(dates)//10)))
    plt.setp(ax1.xaxis.get_majorticklabels(), rotation=45, ha='right')
    ax1.yaxis.set_major_formatter(plt.FuncFormatter(lambda x, p: f'{x:,.0f}'))
    
    # 2. Total Balance Over Time
    totals = [sum(all_history.get(n, {}).get(d, 0.0) for n in account_names) for d in dates]
    ax2.fill_between(date_objects, totals, alpha=0.3, color='blue')
    ax2.plot(date_objects, totals, color='blue', linewidth=2)
    ax2.set_ylabel("Total Balance (CTC)")
    ax2.set_title("Total Balance Over Time")
    ax2.grid(True, alpha=0.3)
    ax2.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m'))
    ax2.xaxis.set_major_locator(mdates.MonthLocator() if len(dates) > 60 else mdates.DayLocator(interval=max(1, len(dates)//10)))
    plt.setp(ax2.xaxis.get_majorticklabels(), rotation=45, ha='right')
    ax2.yaxis.set_major_formatter(plt.FuncFormatter(lambda x, p: f'{x:,.0f}'))
    
    if totals:
        ax2.annotate(f'{totals[0]:,.0f}', xy=(date_objects[0], totals[0]), fontsize=9)
        ax2.annotate(f'{totals[-1]:,.0f}', xy=(date_objects[-1], totals[-1]), fontsize=9, ha='right')

    # 3. Daily Staking Rewards
    reward_totals = [sum(reward_history.get(n, {}).get(d, 0.0) for n in account_names) for d in dates]
    # Use bar chart for rewards
    ax3.bar(date_objects, reward_totals, color='orange', alpha=0.7, width=0.8)
    ax3.set_xlabel("Date")
    ax3.set_ylabel("Daily Rewards (CTC)")
    ax3.set_title("Daily Staking Rewards")
    ax3.grid(True, linestyle='--', alpha=0.3)
    ax3.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m-%d'))
    ax3.xaxis.set_major_locator(mdates.AutoDateLocator())
    plt.setp(ax3.xaxis.get_majorticklabels(), rotation=45, ha='right')
    
    plt.tight_layout()
    graph_file = output_file.with_suffix('.png')
    plt.savefig(graph_file, dpi=150, bbox_inches='tight')
    plt.close()
    graph_files.append(graph_file)
    
    # === 각 계정별 개별 그래프 ===
    individual_dir = output_file.parent / "individual"
    individual_dir.mkdir(parents=True, exist_ok=True)
    
    for i, name in enumerate(account_names):
        balances = [all_history.get(name, {}).get(d, 0.0) for d in dates]
        rewards = [reward_history.get(name, {}).get(d, 0.0) for d in dates]
        
        fig, (iax1, iax2) = plt.subplots(2, 1, figsize=(12, 10))
        fig.suptitle(f"CTC Balance & Rewards - {name}", fontsize=14, fontweight='bold')
        
        color = colors[i % len(colors)]
        iax1.fill_between(date_objects, balances, alpha=0.3, color=color)
        iax1.plot(date_objects, balances, color=color, linewidth=2)
        iax1.set_ylabel("Balance (CTC)")
        iax1.set_title("Balance History")
        iax1.grid(True, alpha=0.3)
        iax1.xaxis.set_major_formatter(mdates.DateFormatter('%Y-%m'))
        iax1.xaxis.set_major_locator(mdates.AutoDateLocator())
        plt.setp(iax1.xaxis.get_majorticklabels(), rotation=45, ha='right')
        iax1.yaxis.set_major_formatter(plt.FuncFormatter(lambda x, p: f'{x:,.0f}'))
        
        iax2.bar(date_objects, rewards, color='orange', alpha=0.7, width=0.8)
        iax2.set_xlabel("Date")
        iax2.set_ylabel("Daily Reward (CTC)")
        iax2.set_title("Daily Staking Rewards")
        iax2.grid(True, linestyle='--', alpha=0.3)
        iax2.xaxis.set_major_formatter(mdates.DateFormatter('%m-%d'))
        iax2.xaxis.set_major_locator(mdates.AutoDateLocator())
        plt.setp(iax2.xaxis.get_majorticklabels(), rotation=45, ha='right')
        
        plt.tight_layout()
        individual_file = individual_dir / f"{name}.png"
        plt.savefig(individual_file, dpi=150, bbox_inches='tight')
        plt.close()
        graph_files.append(individual_file)
    
    return graph_files


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
    
    output_file = Path(args.output) if args.output else OUTPUT_DIR / f"{source_name}_history.csv"
    
    # Load existing data if file exists (to skip already fetched dates)
    existing_data = {}
    if output_file.exists():
        print(f"  Loading existing data from {output_file.name}...")
        try:
            with open(output_file, "r") as f:
                reader = csv.DictReader(f)
                for row in reader:
                    d_key = row["date"]
                    # Validate date format to skip corrupt rows from previous runs
                    try:
                        datetime.strptime(d_key, "%Y-%m-%d")
                    except ValueError:
                        continue
                        
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

    tasks = []
    # 처리할 작업 목록 생성
    for d in dates:
        key = d.isoformat()
        
        # Check if we already have data for this date and all accounts
        if key in existing_data:
            # Check if all requested accounts are present in the existing data
            all_accounts_exist = all(acc in existing_data[key] for acc in accounts)
            if all_accounts_exist:
                continue
        
        block_info = block_map.get(key)
        
        if block_info:
            tasks.append((key, block_info["hash"]))

    with ProcessPoolExecutor(max_workers=CONCURRENCY_BALANCES, initializer=init_worker) as executor:
        future_to_task = {
            executor.submit(_fetch_balance_worker, key, block_hash, accounts, args.refetch_zero): key 
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

    # 4.5. Staking Rewards 조회
    reward_history = {name: {} for name in accounts}
    if not args.no_rewards:
        print("\n[4.5/5] Fetching staking rewards...")
        reward_cache = {} if args.no_cache else load_reward_cache()
        
        reward_tracker = RewardTracker()
        
        # Determine uncached dates
        uncached_dates = []
        for i, d in enumerate(dates):
            date_str = d.isoformat()
            
            all_present = True
            for name in accounts:
                # Check if present in cache (allow 0.0)
                if name not in reward_cache or date_str not in reward_cache[name]:
                    all_present = False
                    break
            
            if not all_present:
                uncached_dates.append((i, date_str))
        
        if uncached_dates:
            print(f"  Fetching rewards for {len(uncached_dates)} uncached dates...")
            for idx, date_str in uncached_dates:
                block_info = block_map.get(date_str)
                if not block_info:
                    continue
                
                # Get block range for this date
                start_block = block_info["block"]
                start_hash = block_info["hash"]
                
                # Next day's block or +BLOCKS_PER_DAY (1 day of blocks)
                if idx + 1 < len(dates):
                    next_date_str = dates[idx+1].isoformat()
                    next_block_info = block_map.get(next_date_str)
                    if next_block_info:
                        end_block = next_block_info["block"]
                        end_hash = next_block_info["hash"]
                    else:
                        end_block = start_block + BLOCKS_PER_DAY
                        end_hash = chain.get_block_hash(end_block)
                else:
                    end_block = start_block + BLOCKS_PER_DAY
                    end_hash = chain.get_block_hash(end_block)
                
                try:
                    # Fetch rewards for this era/block range
                    rewards = reward_tracker.get_rewards_via_eras(accounts, start_hash, end_hash)
                    
                    # Logic match with Rust: if era-based fetching returns nothing, check if events exist
                    total_amt = sum(r.claimed for r in rewards.values())
                    if total_amt == 0:
                        # Fallback to scanning events
                        print(f"    No era rewards for {date_str}, scanning events...")
                        rewards = reward_tracker.get_all_rewards_in_range(start_block, end_block, accounts)
                    
                    for name, reward in rewards.items():
                        if name not in reward_cache:
                            reward_cache[name] = {}
                        reward_cache[name][date_str] = round(reward.claimed, 2)
                        
                    print(f"    {date_str} rewards fetched")
                    save_reward_cache(reward_cache)
                except Exception as e:
                    print(f"    Warning: Failed to fetch rewards for {date_str}: {e}")
        else:
            print("  All rewards found in cache!")
            
        reward_history = reward_cache

    # 5. 결과 저장
    print(f"\n[5/5] Saving results...")
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    
    account_names = sorted(accounts.keys())

    # Merge new data into existing data
    # all_history is {name: {date: balance}}, convert to {date: {name: balance}}
    for name, date_balances in all_history.items():
        for date_key, balance in date_balances.items():
            if date_key not in existing_data:
                existing_data[date_key] = {}
            existing_data[date_key][name] = balance

    header = ["date"] + account_names + ["total", "reward_total", "diff", "diff_avg10"]
    
    rows = []
    diffs = []
    all_date_keys = sorted(existing_data.keys())
    
    prev_total = None
    for key in all_date_keys:
        row = [key]
        total = 0.0
        row_data = existing_data[key]
        for name in account_names:
            bal = row_data.get(name, 0.0)
            row.append(bal)
            total += bal
        total = round(total, 1)
        row.append(total)
        
        # Calculate daily total reward
        day_reward = 0.0
        for name in account_names:
            day_reward += reward_history.get(name, {}).get(key, 0.0)
        row.append(round(day_reward, 1))
        
        # 차이 계산
        if prev_total is not None:
            diff = round(total - prev_total, 1)
        else:
            diff = 0.0
        row.append(diff)
        diffs.append(diff)
        prev_total = total
        
        # 10일 평균 계산
        if len(diffs) >= 10:
            diff_avg10 = round(sum(diffs[-10:]) / 10, 1)
        else:
            diff_avg10 = round(sum(diffs) / len(diffs), 1) if diffs else 0.0
        row.append(diff_avg10)
        
        rows.append(row)
    
    with open(output_file, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(header)
        writer.writerows(rows)
    
    print(f"  CSV (Combined): {output_file}")
    
    # === 각 계정별 개별 CSV 저장 ===
    individual_dir = output_file.parent / "individual"
    individual_dir.mkdir(parents=True, exist_ok=True)
    
    for name in account_names:
        indiv_csv = individual_dir / f"{name}.csv"
        indiv_rows = []
        indiv_diffs = []
        prev_bal = None
        
        for key in all_date_keys:
            bal = existing_data[key].get(name, 0.0)
            reward = reward_history.get(name, {}).get(key, 0.0)
            
            # 차이 계산
            if prev_bal is not None:
                d = round(bal - prev_bal, 1)
            else:
                d = 0.0
            
            indiv_diffs.append(d)
            
            # 10일 평균 계산
            if len(indiv_diffs) >= 10:
                d_avg10 = round(sum(indiv_diffs[-10:]) / 10, 1)
            else:
                d_avg10 = round(sum(indiv_diffs) / len(indiv_diffs), 1) if indiv_diffs else 0.0
                
            indiv_rows.append([key, bal, reward, d, d_avg10])
            prev_bal = bal
            
        with open(indiv_csv, "w", newline="") as f:
            writer = csv.writer(f)
            writer.writerow(["date", "balance", "reward", "diff", "diff_avg10"])
            writer.writerows(indiv_rows)
            
    print(f"  CSV (Individual): {len(account_names)} files in {individual_dir}")
    
    if args.graph and rows:
        print("  Generating graphs...")
        # Reconstruct full history for plotting
        full_history = {name: {} for name in account_names}
        for d_key in all_date_keys:
            row_data = existing_data[d_key]
            for acc_name in account_names:
                if acc_name in row_data:
                    full_history[acc_name][d_key] = row_data[acc_name]
                    
        graph_files = plot_balances(all_date_keys, full_history, reward_history, account_names, output_file, source_name)
        print(f"  Main graph: {graph_files[0]}")
        print(f"  Individual graphs: {len(graph_files) - 1} files in {output_file.parent / 'individual'}")
    
    if rows:
        latest = rows[-1]
        print(f"\n  Latest ({latest[0]}): {format_ctc(latest[-1])} CTC")
    
    print("\n" + "=" * 60)
    print("COMPLETED!")
    print("=" * 60)
    return 0


if __name__ == "__main__":
    exit(main())
