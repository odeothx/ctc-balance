import logging
from typing import Dict, Optional, Tuple
from substrateinterface import SubstrateInterface
from src.chain import NODE_URL
from src.utils import retry
from src import CTC_DIVISOR

logger = logging.getLogger(__name__)



class StakingReward:
    """Staking reward data."""
    def __init__(self, claimed: float = 0.0):
        self.claimed = claimed

    @classmethod
    def zero(cls):
        return cls(0.0)

    def to_dict(self):
        return {"claimed": self.claimed}


class RewardTracker:
    """Reward tracker for Creditcoin3 accounts."""

    def __init__(self, url: str = NODE_URL, substrate: SubstrateInterface | None = None):
        self.url = url
        self._substrate = substrate

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.close()
        return False

    def close(self):
        """Close the substrate connection."""
        if self._substrate is not None:
            try:
                self._substrate.close()
            except Exception:
                pass
            self._substrate = None

    @property
    def substrate(self) -> SubstrateInterface:
        """Lazy connection to substrate node."""
        if self._substrate is None:
            self._substrate = SubstrateInterface(url=self.url)
        return self._substrate

    @retry(max_retries=3)
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

    @retry(max_retries=3)
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
        
        # Build lookup for fast address matching
        # address -> name
        address_to_name = {addr: name for name, addr in accounts.items()}

        for era in range(start_era, end_era + 1):
            self._process_era_rewards(era, end_block_hash, address_to_name, results)

        return {name: StakingReward(claimed=amt / CTC_DIVISOR) for name, amt in results.items()}

    @retry(max_retries=3)
    def _process_era_rewards(self, era: int, block_hash: str, address_to_name: Dict[str, str], results: Dict[str, float]):
        """Process a single era's rewards."""
        # 1. Get total era reward
        total_reward_result = self.substrate.query(
            module="Staking",
            storage_function="ErasValidatorReward",
            params=[era],
            block_hash=block_hash
        )
        if not total_reward_result or not total_reward_result.value:
            return
        total_reward_val = float(total_reward_result.value)
        if total_reward_val == 0:
            return

        # 2. Get era reward points
        points_result = self.substrate.query(
            module="Staking",
            storage_function="ErasRewardPoints",
            params=[era],
            block_hash=block_hash
        )
        if not points_result or not points_result.value:
            return
        
        total_points = float(points_result.value.get("total", 0))
        individual_points = points_result.value.get("individual", {})
        
        if total_points == 0:
            return

        # 3. Process each validator
        if isinstance(individual_points, dict):
            items = individual_points.items()
        else:
            items = individual_points

        for v_address, p_v in items:
            p_v = float(p_v)
            if p_v == 0:
                continue

            # Validator's total reward share for this era
            r_v_total = (total_reward_val * p_v) / total_points
            
            self._process_validator_reward(era, block_hash, v_address, r_v_total, address_to_name, results)

    def _process_validator_reward(
        self, 
        era: int, 
        block_hash: str, 
        v_address: str, 
        r_v_total: float, 
        address_to_name: Dict[str, str], 
        results: Dict[str, float]
    ):
        """Process a single validator's rewards."""
        # Get validator commission
        prefs_result = self.substrate.query(
            module="Staking",
            storage_function="ErasValidatorPrefs",
            params=[era, v_address],
            block_hash=block_hash
        )
        commission_ratio = 0.0
        if prefs_result and prefs_result.value:
            commission_ratio = float(prefs_result.value.get("commission", 0)) / 1_000_000_000.0

        # Get validator exposure
        exposure, is_paged = self._fetch_validator_exposure(era, v_address, block_hash)
        if not exposure:
            return

        e_total = float(exposure.get("total", 0))
        e_own = float(exposure.get("own", 0))
        if e_total == 0:
            return

        # Case 1: Tracked account is the validator
        if v_address in address_to_name:
            name = address_to_name[v_address]
            validator_reward = (r_v_total * commission_ratio) + \
                               (r_v_total * (1.0 - commission_ratio) * (e_own / e_total))
            results[name] += validator_reward

        # Case 2: Tracked account is a nominator
        others = exposure.get("others", [])
        if is_paged and (page_count := exposure.get("page_count", 0)):
            for page_idx in range(page_count):
                paged_result = self.substrate.query(
                    module="Staking",
                    storage_function="ErasStakersPaged",
                    params=[era, v_address, page_idx],
                    block_hash=block_hash
                )
                if paged_result and paged_result.value:
                    others.extend(paged_result.value.get("others", []))

        for nominator in others:
            n_address = nominator.get("who")
            n_value = float(nominator.get("value", 0))
            
            if n_address in address_to_name:
                name = address_to_name[n_address]
                n_reward = r_v_total * (1.0 - commission_ratio) * (n_value / e_total)
                results[name] += n_reward

    def _fetch_validator_exposure(self, era: int, v_address: str, block_hash: str) -> Tuple[Optional[Dict], bool]:
        """Fetch validator exposure (Overview/Paged or Clipped).
        
        Note: ErasStakersOverview was introduced with paged staking.
        For older eras/blocks, this storage function doesn't exist,
        so we catch the exception and fallback to ErasStakersClipped.
        """
        # Try ErasStakersOverview (Paged Staking) - may not exist in older runtimes
        try:
            res = self.substrate.query("Staking", "ErasStakersOverview", [era, v_address], block_hash=block_hash)
            if res and res.value:
                return res.value, True
        except Exception:
            # Storage function not found in this runtime version - expected for older blocks
            pass
        
        # Fallback to ErasStakersClipped (legacy, always available)
        try:
            res = self.substrate.query("Staking", "ErasStakersClipped", [era, v_address], block_hash=block_hash)
            if res and res.value:
                return res.value, False
        except Exception as e:
            logger.debug(f"Failed to fetch ErasStakersClipped for era {era}, validator {v_address}: {e}")
            
        return None, False

    def get_all_rewards_in_range(
        self, 
        start_block: int, 
        end_block: int, 
        accounts: Dict[str, str]
    ) -> Dict[str, StakingReward]:
        """
        Scan all blocks in range for Staking(Rewarded) events.
        This is the fallback for historical rewards.
        Uses parallel processing for speed.
        """
        from concurrent.futures import ThreadPoolExecutor, as_completed, TimeoutError
        import threading
        
        # Reduced concurrency to avoid overwhelming RPC node
        # Too many connections cause rate limiting or hangs
        CONCURRENCY_EVENTS = 10
        
        results = {name: 0.0 for name in accounts}
        address_to_name = {addr: name for name, addr in accounts.items()}
        
        total_blocks = end_block - start_block + 1
        block_nums = list(range(start_block, end_block + 1))
        
        # Thread-safe result aggregation
        results_lock = threading.Lock()
        processed_count = [0]
        
        # Pre-create a pool of connections (one per worker)
        url = self.url
        
        def scan_block(block_num: int) -> Dict[str, int]:
            """Scan single block and return rewards found."""
            local_results = {}
            try:
                from substrateinterface import SubstrateInterface
                substrate = SubstrateInterface(url=url)
                try:
                    block_hash = substrate.get_block_hash(block_num)
                    if block_hash is None:
                        return local_results
                    events = substrate.get_events(block_hash)
                    
                    for event in events:
                        if event.value['module_id'] == 'Staking' and event.value['event_id'] in ('Rewarded', 'Reward'):
                            params = event.value['params']
                            if len(params) >= 2:
                                stash = params[0]['value']
                                amount = int(params[1]['value'])
                                
                                if stash in address_to_name:
                                    name = address_to_name[stash]
                                    local_results[name] = local_results.get(name, 0) + amount
                finally:
                    try:
                        substrate.close()
                    except Exception:
                        pass
            except Exception as e:
                logger.debug(f"Error scanning block {block_num}: {e}")
            return local_results
        
        print(f"    Scanning {total_blocks} blocks (parallel, {CONCURRENCY_EVENTS} workers)...")
        
        # Execute in parallel with timeout protection
        with ThreadPoolExecutor(max_workers=CONCURRENCY_EVENTS) as executor:
            future_to_block = {executor.submit(scan_block, bn): bn for bn in block_nums}
            
            for future in as_completed(future_to_block, timeout=300):  # 5 minute timeout
                try:
                    block_rewards = future.result(timeout=30)  # 30 second per-block timeout
                    
                    with results_lock:
                        for name, amount in block_rewards.items():
                            results[name] += amount
                        
                        processed_count[0] += 1
                        if processed_count[0] % 500 == 0 or processed_count[0] == total_blocks:
                            print(f"    Scanning blocks: {processed_count[0] * 100 // total_blocks}% ({processed_count[0]}/{total_blocks})")
                except TimeoutError:
                    block_num = future_to_block[future]
                    logger.warning(f"Timeout scanning block {block_num}")
                except Exception as e:
                    logger.debug(f"Error in future: {e}")

        return {name: StakingReward(claimed=amt / CTC_DIVISOR) for name, amt in results.items()}

    @retry(max_retries=3)
    def _process_block_events(self, block_num: int, address_to_name: Dict[str, str], results: Dict[str, float]):
        """Fetch and process events for a single block."""
        block_hash = self.substrate.get_block_hash(block_num)
        events = self.substrate.get_events(block_hash)
        
        for event in events:
            # event is an EventRecord-like object from substrateinterface
            if event.value['module_id'] == 'Staking' and event.value['event_id'] in ('Rewarded', 'Reward'):
                # Params are usually (stash_address, amount)
                params = event.value['params']
                if len(params) >= 2:
                    stash = params[0]['value']
                    amount = int(params[1]['value'])
                    
                    if stash in address_to_name:
                        name = address_to_name[stash]
                        results[name] += amount

    def has_staking_events(self, block_num: int) -> bool:
        """Check if a block contains any Staking(Rewarded/Reward) events."""
        try:
            block_hash = self.substrate.get_block_hash(block_num)
            events = self.substrate.get_events(block_hash)
            for event in events:
                if event.value['module_id'] == 'Staking' and event.value['event_id'] in ('Rewarded', 'Reward'):
                    return True
        except Exception:
            pass
        return False
