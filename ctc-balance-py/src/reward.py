"""
Staking reward tracking module for Creditcoin3.
"""

from typing import Dict, List, Optional, Tuple
from datetime import date
from substrateinterface import SubstrateInterface
from src.chain import NODE_URL

CTC_DECIMALS = 18

class StakingReward:
    """Staking reward data."""
    def __init__(self, claimed: float = 0.0):
        self.claimed = claimed

    def to_dict(self):
        return {"claimed": self.claimed}

class RewardTracker:
    """Reward tracker for Creditcoin3 accounts."""

    def __init__(self, url: str = NODE_URL):
        self.url = url
        self._substrate = None

    @property
    def substrate(self) -> SubstrateInterface:
        """Lazy connection to substrate node."""
        if self._substrate is None:
            self._substrate = SubstrateInterface(url=self.url)
        return self._substrate

    def get_active_era(self, block_hash: str) -> Optional[int]:
        """Get active era at a specific block hash."""
        result = self.substrate.query(
            module="Staking",
            storage_function="ActiveEra",
            block_hash=block_hash
        )
        if result and result.value:
            return result.value.get("index")
        return None

    def get_rewards_via_eras(
        self, 
        accounts: Dict[str, str], 
        start_block_hash: str, 
        end_block_hash: str
    ) -> Dict[str, StakingReward]:
        """
        Get rewards for accounts in a block range using Eras storage.
        """
        start_era = self.get_active_era(start_block_hash)
        end_era = self.get_active_era(end_block_hash)

        if start_era is None or end_era is None:
            return {}

        results = {name: 0.0 for name in accounts}
        divisor = 10**CTC_DECIMALS

        # Account ID mapping (SS58 to bytes and name)
        # substrateinterface handles SS58, so we can use addresses directly
        # but for internal mapping we might use bytes.
        
        for era in range(start_era, end_era + 1):
            # 1. Get total era reward
            total_reward_result = self.substrate.query(
                module="Staking",
                storage_function="ErasValidatorReward",
                params=[era],
                block_hash=end_block_hash
            )
            if not total_reward_result or not total_reward_result.value:
                continue
            total_reward_val = float(total_reward_result.value)

            # 2. Get era reward points
            points_result = self.substrate.query(
                module="Staking",
                storage_function="ErasRewardPoints",
                params=[era],
                block_hash=end_block_hash
            )
            if not points_result or not points_result.value:
                continue
            
            total_points = float(points_result.value.get("total", 0))
            individual_points = points_result.value.get("individual", {})
            
            if total_points == 0 or total_reward_val == 0:
                continue

            # 3. For each validator who earned points
            # individual_points can be dict or list of [address, points]
            if isinstance(individual_points, dict):
                items = individual_points.items()
            else:
                items = individual_points

            for item in items:
                if isinstance(item, (list, tuple)):
                    v_address, p_v = item
                else:
                    # If it's a dict entry (though unlikely if individual_points wasn't a dict)
                    continue
                
                p_v = float(p_v)
                if p_v == 0:
                    continue

                # Validator's total reward share for this era
                r_v_total = (total_reward_val * p_v) / total_points

                # 4. Get validator commission
                prefs_result = self.substrate.query(
                    module="Staking",
                    storage_function="ErasValidatorPrefs",
                    params=[era, v_address],
                    block_hash=end_block_hash
                )
                commission_ratio = 0.0
                if prefs_result and prefs_result.value:
                    commission_ratio = float(prefs_result.value.get("commission", 0)) / 1_000_000_000.0

                # 5. Get validator exposure (Overview or Clipped)
                exposure = None
                # Try ErasStakersOverview (Paged Staking)
                exposure_result = self.substrate.query(
                    module="Staking",
                    storage_function="ErasStakersOverview",
                    params=[era, v_address],
                    block_hash=end_block_hash
                )
                
                is_paged = False
                if exposure_result and exposure_result.value:
                    exposure = exposure_result.value
                    is_paged = True
                else:
                    # Fallback to ErasStakersClipped (Legacy)
                    exposure_result = self.substrate.query(
                        module="Staking",
                        storage_function="ErasStakersClipped",
                        params=[era, v_address],
                        block_hash=end_block_hash
                    )
                    if exposure_result and exposure_result.value:
                        exposure = exposure_result.value

                if not exposure:
                    continue

                e_total = float(exposure.get("total", 0))
                e_own = float(exposure.get("own", 0))
                if e_total == 0:
                    continue

                # Get nominators
                others = exposure.get("others", [])
                
                # If paged, fetch from ErasStakersPaged
                if is_paged and (page_count := exposure.get("page_count", 0)):
                    for page_idx in range(page_count):
                        paged_result = self.substrate.query(
                            module="Staking",
                            storage_function="ErasStakersPaged",
                            params=[era, v_address, page_idx],
                            block_hash=end_block_hash
                        )
                        if paged_result and paged_result.value:
                            others.extend(paged_result.value.get("others", []))

                # 6. Calculate rewards for our tracked accounts
                
                # Case 1: Tracked account is the validator
                for name, address in accounts.items():
                    if address == v_address:
                        validator_reward = (r_v_total * commission_ratio) + \
                                           (r_v_total * (1.0 - commission_ratio) * (e_own / e_total))
                        results[name] += validator_reward

                # Case 2: Tracked account is a nominator
                for nominator in others:
                    n_address = nominator.get("who")
                    n_value = float(nominator.get("value", 0))
                    
                    for name, address in accounts.items():
                        if address == n_address:
                            nominator_reward = r_v_total * (1.0 - commission_ratio) * (n_value / e_total)
                            results[name] += nominator_reward

        return {name: StakingReward(claimed=amt / divisor) for name, amt in results.items()}
