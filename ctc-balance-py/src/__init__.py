# CTC Balance Tracker - Source Package
from decimal import Decimal

# Shared constants
CTC_DECIMALS = 18
CTC_DIVISOR = 10**CTC_DECIMALS
CTC_DIVISOR_DEC = Decimal(10) ** CTC_DECIMALS  # For precise financial calculations
EPSILON = 1e-10  # For safe float comparison

# Block timing
BLOCK_TIME_SECONDS = 15
BLOCKS_PER_DAY = 24 * 60 * 60 // BLOCK_TIME_SECONDS  # 5760 blocks
# Concurrency Constants
CONCURRENCY_EXPOSURES = 20
CONCURRENCY_EVENTS = 50
