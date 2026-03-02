"""
Constants and configuration for Tempo prediction
"""

from datetime import date
from typing import Literal

# Tempo color types
TempoColor = Literal["BLUE", "WHITE", "RED"]

# Tempo year configuration
TEMPO_YEAR_START_MONTH = 9  # September
TEMPO_YEAR_START_DAY = 1

# Stock limits per Tempo year
STOCK_RED_DAYS = 22
STOCK_WHITE_DAYS = 43
STOCK_BLUE_DAYS = 300  # Remaining days (365 - 22 - 43)

# Red day constraints
RED_PERIOD_START_MONTH = 11  # November
RED_PERIOD_START_DAY = 1
RED_PERIOD_END_MONTH = 3  # March
RED_PERIOD_END_DAY = 31
MAX_CONSECUTIVE_RED_DAYS = 5

# Algorithm thresholds (from RTE documentation)
# Threshold = A - B * day - C * stock
THRESHOLD_RED = {
    "A": 3.15,
    "B": 0.010,
    "C": 0.031,
}

THRESHOLD_WHITE_RED = {
    "A": 4.00,
    "B": 0.015,
    "C": 0.026,
}

# Normalization constants (from RTE simplified model)
NORMALIZATION_MEAN = 46050  # MW
NORMALIZATION_STD = 2160  # MW

# Temperature correction parameters
TEMP_SENSITIVITY_GAMMA = -0.1176
TEMP_QUANTILE_30_MEAN = 8.3042  # °C

# API URLs
RTE_TEMPO_API = "https://www.services-rte.com/cms/open_data/v1/tempo"
RTE_ECO2MIX_API = (
    "https://odre.opendatasoft.com/api/explore/v2.1/catalog/datasets/eco2mix-national-tr/records"
)
OPEN_METEO_API = "https://api.open-meteo.com/v1/forecast"
OPEN_METEO_HISTORICAL_API = "https://archive-api.open-meteo.com/v1/archive"

# France average coordinates (for weather)
FRANCE_LAT = 46.603354
FRANCE_LON = 1.888334

# Cache directory
CACHE_DIR = "cache"
MODEL_DIR = "models"


def get_tempo_year(d: date) -> tuple[int, int]:
    """
    Get the Tempo year for a given date.
    Tempo year runs from September 1 to August 31.

    Returns:
        Tuple of (start_year, end_year) e.g., (2024, 2025)
    """
    if d.month >= TEMPO_YEAR_START_MONTH:
        return (d.year, d.year + 1)
    return (d.year - 1, d.year)


def get_tempo_day_number(d: date) -> int:
    """
    Get the day number within the Tempo year (0-indexed).
    Day 0 = September 1st.
    """
    start_year, _ = get_tempo_year(d)
    tempo_start = date(start_year, TEMPO_YEAR_START_MONTH, TEMPO_YEAR_START_DAY)
    return (d - tempo_start).days


def is_in_red_period(d: date) -> bool:
    """Check if the date is within the red day period (Nov 1 - Mar 31)."""
    month = d.month
    return month >= RED_PERIOD_START_MONTH or month <= RED_PERIOD_END_MONTH


def is_weekend(d: date) -> bool:
    """Check if the date is a weekend (Saturday=5, Sunday=6)."""
    return d.weekday() >= 5


def is_sunday(d: date) -> bool:
    """Check if the date is a Sunday."""
    return d.weekday() == 6


def can_be_red(d: date, consecutive_red_count: int = 0) -> bool:
    """
    Check if a date can be marked as red.

    Constraints:
    - Must be in red period (Nov 1 - Mar 31)
    - Cannot be weekend
    - Cannot exceed 5 consecutive red days
    """
    if not is_in_red_period(d):
        return False
    if is_weekend(d):
        return False
    return not consecutive_red_count >= MAX_CONSECUTIVE_RED_DAYS


def can_be_white(d: date) -> bool:
    """
    Check if a date can be marked as white.

    Constraints:
    - Cannot be Sunday
    """
    return not is_sunday(d)
