"""
Subscan API client for Creditcoin3.
"""

import requests
from datetime import date
from dataclasses import dataclass


SUBSCAN_API_URL = "https://creditcoin.api.subscan.io"
CTC_DECIMALS = 18


@dataclass
class DailyBalance:
    """Daily balance data."""
    date: str
    balance: float  # CTC
    raw_balance: str  # Original value


class SubscanClient:
    """Subscan API client for balance queries."""

    def __init__(self, api_key: str | None = None):
        self.base_url = SUBSCAN_API_URL
        self.headers = {"Content-Type": "application/json"}
        if api_key:
            self.headers["X-API-Key"] = api_key

    def _post(self, endpoint: str, data: dict) -> dict:
        """Make POST request to Subscan API."""
        url = f"{self.base_url}{endpoint}"
        response = requests.post(url, json=data, headers=self.headers)
        response.raise_for_status()
        result = response.json()
        
        if result.get("code") != 0:
            raise Exception(f"Subscan API error: {result.get('message')}")
        
        return result.get("data", {})

    def get_balance_history(
        self, 
        address: str, 
        start_date: date, 
        end_date: date
    ) -> list[DailyBalance]:
        """
        Get daily balance history for an address.
        
        Args:
            address: SS58 wallet address
            start_date: Start date (inclusive)
            end_date: End date (inclusive)
            
        Returns:
            List of DailyBalance objects
        """
        data = self._post(
            "/api/scan/account/balance_history",
            {
                "address": address,
                "start": start_date.isoformat(),
                "end": end_date.isoformat(),
            }
        )
        
        history = data.get("history") or []  # Handle None
        divisor = 10 ** CTC_DECIMALS
        
        return [
            DailyBalance(
                date=item["date"],
                balance=int(item["balance"]) / divisor,
                raw_balance=item["balance"],
            )
            for item in history
        ]

    def get_current_balance(self, address: str) -> dict:
        """
        Get current balance for an address.
        
        Returns dict with keys: balance, lock, reserved, bonded, unbonding
        """
        data = self._post(
            "/api/scan/account/tokens",
            {"address": address}
        )
        
        native = data.get("native", [])
        if not native:
            return {"balance": 0.0}
        
        token = native[0]
        divisor = 10 ** CTC_DECIMALS
        
        return {
            "balance": int(token.get("balance", 0)) / divisor,
            "lock": int(token.get("lock", 0)) / divisor,
            "reserved": int(token.get("reserved", 0)) / divisor,
            "bonded": int(token.get("bonded", 0)) / divisor,
            "unbonding": int(token.get("unbonding", 0)) / divisor,
            "price": float(token.get("price", 0)),
        }
