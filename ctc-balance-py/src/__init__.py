# CTC Balance Tracker - Source Package

# Shared constants
CTC_DECIMALS = 18
CTC_DIVISOR = 10**CTC_DECIMALS
EPSILON = 1e-10  # For safe float comparison

# Block timing
BLOCK_TIME_SECONDS = 15
BLOCKS_PER_DAY = 24 * 60 * 60 // BLOCK_TIME_SECONDS  # 5760 blocks
