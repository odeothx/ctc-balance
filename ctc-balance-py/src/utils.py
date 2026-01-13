"""
Utility functions for CTC Balance Tracker.
"""

import csv
import json
from pathlib import Path
from datetime import datetime, date, timezone
from typing import Any


OUTPUT_DIR = Path(__file__).parent.parent / "output"
PROGRESS_FILE = OUTPUT_DIR / "progress.json"


def ensure_output_dir():
    """Ensure output directory exists."""
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)


def get_utc_midnight(d: date) -> datetime:
    """Get UTC midnight datetime for a date."""
    return datetime(d.year, d.month, d.day, 0, 0, 0, tzinfo=timezone.utc)


def get_utc_midnight_timestamp(d: date) -> int:
    """Get Unix timestamp for UTC midnight of a date."""
    return int(get_utc_midnight(d).timestamp())


def date_range(start_date: date, end_date: date):
    """Generate dates from start_date to end_date (inclusive)."""
    from datetime import timedelta

    current = start_date
    while current <= end_date:
        yield current
        current += timedelta(days=1)


def save_progress(last_date: date, output_file: str):
    """Save progress to resume later."""
    ensure_output_dir()
    progress = {
        "last_date": last_date.isoformat(),
        "output_file": output_file,
        "updated_at": datetime.now(timezone.utc).isoformat(),
    }
    with open(PROGRESS_FILE, "w") as f:
        json.dump(progress, f, indent=2)


def load_progress() -> dict[str, Any] | None:
    """Load progress from file."""
    if not PROGRESS_FILE.exists():
        return None
    with open(PROGRESS_FILE) as f:
        progress = json.load(f)
    progress["last_date"] = date.fromisoformat(progress["last_date"])
    return progress


def clear_progress():
    """Clear progress file."""
    if PROGRESS_FILE.exists():
        PROGRESS_FILE.unlink()


def create_csv_header(account_names: list[str]) -> list[str]:
    """Create CSV header row."""
    header = ["date", "block_number", "block_hash"]
    for name in sorted(account_names):
        header.extend([f"{name}_free", f"{name}_reserved", f"{name}_total"])
    header.append("total_free")
    return header


def create_csv_row(
    d: date,
    block_number: int,
    block_hash: str,
    balances: dict[str, Any],
    account_names: list[str],
) -> list[Any]:
    """Create CSV data row."""
    row = [d.isoformat(), block_number, block_hash]
    total_free = 0.0
    for name in sorted(account_names):
        balance = balances.get(name)
        if balance:
            row.extend([balance.free, balance.reserved, balance.total])
            total_free += balance.free
        else:
            row.extend([0.0, 0.0, 0.0])
    row.append(total_free)
    return row


def save_csv(
    output_file: Path,
    header: list[str],
    rows: list[list[Any]],
    append: bool = False,
):
    """Save data to CSV file."""
    ensure_output_dir()
    mode = "a" if append else "w"
    write_header = not append or not output_file.exists()

    with open(output_file, mode, newline="") as f:
        writer = csv.writer(f)
        if write_header:
            writer.writerow(header)
        writer.writerows(rows)


def format_ctc(amount: float) -> str:
    """Format CTC amount with commas."""
    return f"{amount:,.2f}"
