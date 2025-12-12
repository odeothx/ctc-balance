"""
Balance query module for Creditcoin3 accounts.
"""

from dataclasses import dataclass
from src.chain import ChainConnector


# CTC 단위
CTC_DECIMALS = 18


@dataclass
class Balance:
    """Account balance data."""

    free: float  # 사용 가능 잔고 (CTC)
    reserved: float  # 예약된 잔고 (CTC)
    frozen: float  # 동결된 잔고 (CTC)

    @property
    def total(self) -> float:
        """Total balance (free + reserved)."""
        return self.free + self.reserved

    def to_dict(self) -> dict:
        """Convert to dictionary."""
        return {
            "free": self.free,
            "reserved": self.reserved,
            "frozen": self.frozen,
            "total": self.total,
        }


class BalanceTracker:
    """Account balance tracker for Creditcoin3."""

    def __init__(self, chain: ChainConnector | None = None):
        self.chain = chain or ChainConnector()

    def get_balance(
        self, address: str, block_hash: str | None = None
    ) -> Balance:
        """
        Get account balance at a specific block.

        Args:
            address: Account SS58 address
            block_hash: Block hash (None for latest)

        Returns:
            Balance object
        """
        result = self.chain.substrate.query(
            module="System",
            storage_function="Account",
            params=[address],
            block_hash=block_hash,
        )

        data = result.value["data"]
        divisor = 10**CTC_DECIMALS

        return Balance(
            free=int(data["free"]) / divisor,
            reserved=int(data["reserved"]) / divisor,
            frozen=int(data.get("frozen", 0)) / divisor,
        )

    def get_balances_batch(
        self, accounts: dict[str, str], block_hash: str | None = None
    ) -> dict[str, Balance]:
        """
        Get balances for all accounts at a specific block.
        (Batching disabled due to library limitation, using fast sequential)
        """
        return self.get_all_balances(accounts, block_hash)

    def get_all_balances(
        self, accounts: dict[str, str], block_hash: str | None = None
    ) -> dict[str, Balance]:
        """
        Get balances for all accounts at a specific block.
        """
        balances = {}
        for name, address in accounts.items():
            try:
                balances[name] = self.get_balance(address, block_hash)
            except Exception as e:
                # print(f"  Warning: Failed to get balance for {name}: {e}")
                balances[name] = Balance(free=0.0, reserved=0.0, frozen=0.0)
        return balances
