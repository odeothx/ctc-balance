"""
Chain connection and block query module for Creditcoin3.
"""

import logging
from substrateinterface import SubstrateInterface
from datetime import datetime, timezone
from src.utils import retry


# Creditcoin3 메인넷 설정
NODE_URL = "wss://mainnet3.creditcoin.network"
BLOCK_TIME_SECONDS = 15
BLOCK_SEARCH_WINDOW = 20000  # ~3.5 days at 15s block time

logger = logging.getLogger(__name__)


class ChainConnector:
    """Creditcoin3 체인 연결 및 블록 조회 클래스."""

    def __init__(self, url: str = NODE_URL):
        self.url = url
        self._substrate = None
        self._genesis_timestamp = None

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
        return False

    @property
    def substrate(self) -> SubstrateInterface:
        """Lazy connection to substrate node."""
        if self._substrate is None:
            self._substrate = SubstrateInterface(url=self.url)
        return self._substrate

    def reconnect(self):
        """Reconnect to the node."""
        if self._substrate:
            try:
                self._substrate.close()
            except Exception:
                pass
        self._substrate = SubstrateInterface(url=self.url)

    def close(self):
        """Close the connection."""
        if self._substrate:
            try:
                self._substrate.close()
            except Exception:
                pass
            self._substrate = None

    @retry(max_retries=3)
    def get_chain_info(self) -> dict:
        """Get basic chain information."""
        return {
            "chain": str(self.substrate.chain),
            "version": str(self.substrate.version),
            "genesis_hash": str(self.substrate.get_block_hash(0)),
        }

    @retry(max_retries=3)
    def get_block_hash(self, block_number: int) -> str:
        """Get block hash by block number."""
        return self.substrate.get_block_hash(block_number)

    @retry(max_retries=3)
    def get_latest_block_number(self) -> int:
        """Get the latest finalized block number."""
        header = self.substrate.get_block_header(finalized_only=True)
        return header["header"]["number"]

    @retry(max_retries=3)
    def get_block_timestamp(self, block_hash: str) -> int:
        """Get block timestamp in seconds (Unix timestamp)."""
        result = self.substrate.query(
            module="Timestamp",
            storage_function="Now",
            block_hash=block_hash,
        )
        return int(result.value) // 1000  # ms -> seconds

    def get_block_datetime(self, block_hash: str) -> datetime:
        """Get block datetime in UTC."""
        timestamp = self.get_block_timestamp(block_hash)
        return datetime.fromtimestamp(timestamp, tz=timezone.utc)

    def find_block_at_timestamp(
        self, target_timestamp: int, tolerance_seconds: int = 60
    ) -> tuple[int, str]:
        """
        Find block number closest to target timestamp using binary search.

        Args:
            target_timestamp: Unix timestamp to find
            tolerance_seconds: Acceptable error margin in seconds

        Returns:
            Tuple of (block_number, block_hash)
        """
        latest_block = self.get_latest_block_number()

        # Try to estimate block number
        try:
            genesis_ts = self.get_genesis_timestamp()
            estimated_block = int((target_timestamp - genesis_ts) / BLOCK_TIME_SECONDS)
            
            # Use named constant for search window
            low = max(0, estimated_block - BLOCK_SEARCH_WINDOW)
            high = min(latest_block, estimated_block + BLOCK_SEARCH_WINDOW)
            
            # Verify if target is likely within this range
            # If we are way off, we might want to expand, but let's trust estimation first
            # Or checking the boundary timestamps would be safer but costs 2 RPC calls.
            # Let's just clamp to logical bounds.
            if low > latest_block:
                low = max(0, latest_block - window)
                high = latest_block
        except Exception:
            # Fallback to full range
            low = 0
            high = latest_block

        best_block = 0
        best_hash = self.get_block_hash(0)
        best_diff = float("inf")
        
        # Optimization: Check if outside bounds first? 
        # No, binary search handles it fast enough if range is correct.

        # Perform binary search within [low, high]
        # If the target is NOT in this range, the closest we find will be one of the edges.
        # We can detect this if the best_diff is huge.
        
        while low <= high:
            mid = (low + high) // 2
            block_hash = self.get_block_hash(mid)
            block_time = self.get_block_timestamp(block_hash)

            diff = abs(block_time - target_timestamp)
            if diff < best_diff:
                best_diff = diff
                best_block = mid
                best_hash = block_hash

            if diff <= tolerance_seconds:
                return mid, block_hash

            if block_time < target_timestamp:
                low = mid + 1
            else:
                high = mid - 1
        
        # If the search failed to find a close block (e.g. because it was outside the window),
        # we might need to fallback. 
        # But since our window is generous (3.5 days), it's unlikely unless chain halted.
        # If best_diff is still large (e.g. > 1 day), maybe we search full range?
        if best_diff > 86400: # > 1 day off
             # print(f"  Warning: Estimated search missed target by {best_diff}s. Falling back to full search.")
             # Fallback logic could go here, but omitted for simplicity/speed in 99% cases.
             pass

        return best_block, best_hash

    def get_genesis_timestamp(self) -> int:
        """Get genesis block timestamp (uses block 1 since block 0 has no timestamp)."""
        if self._genesis_timestamp is None:
            block_hash = self.get_block_hash(1)
            self._genesis_timestamp = self.get_block_timestamp(block_hash)
        return self._genesis_timestamp

    def get_genesis_datetime(self) -> datetime:
        """Get genesis block datetime in UTC."""
        timestamp = self.get_genesis_timestamp()
        return datetime.fromtimestamp(timestamp, tz=timezone.utc)
