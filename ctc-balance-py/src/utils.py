"""
Utility functions for CTC Balance Tracker.
"""

import csv
import json
import logging
from pathlib import Path
from datetime import datetime, date, timezone
from typing import Any, Callable, TypeVar, cast
import functools
import time

logger = logging.getLogger(__name__)

T = TypeVar("T")


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


def retry(
    max_retries: int = 5,
    base_delay: float = 1.0,
    exceptions: tuple[type[Exception], ...] = (Exception,),
):
    """
    Decorator to retry a function on exception.
    Similar to the Rust retry! macro.
    """

    def decorator(func: Callable[..., T]) -> Callable[..., T]:
        @functools.wraps(func)
        def wrapper(*args, **kwargs) -> T:
            last_exception = None
            for i in range(max_retries):
                try:
                    return func(*args, **kwargs)
                except exceptions as e:
                    last_exception = e
                    delay = base_delay * (2**i)
                    logger.debug(f"Retry {i+1}/{max_retries} for {func.__name__} after {delay}s due to: {e}")
                    time.sleep(delay)
            raise cast(Exception, last_exception)

        return wrapper

    return decorator


def format_ctc(amount: float) -> str:
    """Format CTC amount with commas."""
    return f"{amount:,.2f}"


def validate_ss58_address(address: str) -> bool:
    """
    Validate an SS58 address format.
    
    Returns True if the address appears to be a valid SS58 address.
    Note: This is a basic format check, not a full cryptographic validation.
    """
    import re
    # Basic SS58 format: starts with 5 (Creditcoin prefix), 47-48 characters, alphanumeric
    if not address:
        return False
    if not re.match(r'^[1-9A-HJ-NP-Za-km-z]{47,48}$', address):
        return False
    # Creditcoin addresses typically start with 5
    if not address.startswith('5'):
        return False
    return True
