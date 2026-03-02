"""
Implementation of the RTE Tempo algorithm.
This is the official algorithm used to determine Tempo colors.
"""

from dataclasses import dataclass
from datetime import date

from .constants import (
    NORMALIZATION_MEAN,
    NORMALIZATION_STD,
    STOCK_RED_DAYS,
    STOCK_WHITE_DAYS,
    TEMP_QUANTILE_30_MEAN,
    TEMP_SENSITIVITY_GAMMA,
    THRESHOLD_RED,
    THRESHOLD_WHITE_RED,
    TempoColor,
    can_be_red,
    can_be_white,
    get_tempo_day_number,
    get_tempo_year,
    is_in_red_period,
)


@dataclass
class TempoState:
    """Current state of the Tempo algorithm."""

    stock_red: int = STOCK_RED_DAYS  # Remaining red days
    stock_white: int = STOCK_WHITE_DAYS  # Remaining white days
    consecutive_red: int = 0  # Consecutive red days count
    last_color: TempoColor | None = None

    def copy(self) -> "TempoState":
        return TempoState(
            stock_red=self.stock_red,
            stock_white=self.stock_white,
            consecutive_red=self.consecutive_red,
            last_color=self.last_color,
        )


class TempoAlgorithm:
    """
    Implementation of the RTE Tempo color selection algorithm.

    Based on the official RTE documentation:
    - Threshold_Red = 3.15 - 0.010 * day - 0.031 * stock_red
    - Threshold_White+Red = 4.00 - 0.015 * day - 0.026 * (stock_red + stock_white)
    """

    def __init__(
        self,
        normalization_mean: float = NORMALIZATION_MEAN,
        normalization_std: float = NORMALIZATION_STD,
    ):
        self.normalization_mean = normalization_mean
        self.normalization_std = normalization_std

    def normalize_consumption(
        self,
        net_consumption: float,
        temperature: float | None = None,
    ) -> float:
        """
        Normalize net consumption using the simplified RTE formula.

        Args:
            net_consumption: Net consumption in MW (consumption - wind - solar)
            temperature: Optional temperature for advanced correction

        Returns:
            Normalized consumption value
        """
        # Simplified normalization (from RTE documentation)
        normalized = (net_consumption - self.normalization_mean) / self.normalization_std

        # Optional temperature correction
        if temperature is not None:
            temp_correction = TEMP_SENSITIVITY_GAMMA * (temperature - TEMP_QUANTILE_30_MEAN)
            normalized = normalized * (1 + temp_correction)

        return normalized

    def calculate_threshold_red(self, tempo_day: int, stock_red: int) -> float:
        """Calculate the threshold for red day selection."""
        return THRESHOLD_RED["A"] - THRESHOLD_RED["B"] * tempo_day - THRESHOLD_RED["C"] * stock_red

    def calculate_threshold_white_red(
        self,
        tempo_day: int,
        stock_red: int,
        stock_white: int,
    ) -> float:
        """Calculate the threshold for white+red day selection."""
        return (
            THRESHOLD_WHITE_RED["A"]
            - THRESHOLD_WHITE_RED["B"] * tempo_day
            - THRESHOLD_WHITE_RED["C"] * (stock_red + stock_white)
        )

    def determine_color(
        self,
        d: date,
        normalized_consumption: float,
        state: TempoState,
        force_stock_depletion: bool = False,
    ) -> tuple[TempoColor, TempoState]:
        """
        Determine the Tempo color for a given day.

        Args:
            d: The date to evaluate
            normalized_consumption: Normalized net consumption
            state: Current algorithm state (stocks, consecutive days, etc.)
            force_stock_depletion: If True, force color placement to deplete stocks

        Returns:
            Tuple of (color, new_state)
        """
        new_state = state.copy()
        tempo_day = get_tempo_day_number(d)

        # Calculate thresholds
        threshold_red = self.calculate_threshold_red(tempo_day, state.stock_red)
        threshold_white_red = self.calculate_threshold_white_red(
            tempo_day, state.stock_red, state.stock_white
        )

        # Check if we need to force stock depletion (end of period)
        days_remaining = self._days_remaining_in_period(d)

        # Determine color based on thresholds and constraints
        color: TempoColor = "BLUE"

        # Check for RED
        if (
            (
                normalized_consumption > threshold_red
                or (
                    force_stock_depletion
                    and state.stock_red > 0
                    and days_remaining <= state.stock_red
                )
            )
            and can_be_red(d, state.consecutive_red)
            and state.stock_red > 0
        ):
            color = "RED"
            new_state.stock_red -= 1
            new_state.consecutive_red += 1
            new_state.last_color = "RED"
            return color, new_state

        # Reset consecutive red count if not red
        new_state.consecutive_red = 0

        # Check for WHITE
        if (
            (
                normalized_consumption > threshold_white_red
                or (
                    force_stock_depletion
                    and state.stock_white > 0
                    and days_remaining <= state.stock_white
                )
            )
            and can_be_white(d)
            and state.stock_white > 0
        ):
            color = "WHITE"
            new_state.stock_white -= 1
            new_state.last_color = "WHITE"
            return color, new_state

        # Default to BLUE
        new_state.last_color = "BLUE"
        return "BLUE", new_state

    def _days_remaining_in_period(self, d: date) -> int:
        """Calculate days remaining in the current Tempo year."""
        _start_year, end_year = get_tempo_year(d)
        tempo_end = date(end_year, 8, 31)
        return (tempo_end - d).days

    def _days_remaining_red_period(self, d: date) -> int:
        """Calculate days remaining in the red period (ends March 31)."""
        if not is_in_red_period(d):
            return 0

        year = d.year
        red_end = date(year + 1, 3, 31) if d.month >= 11 else date(year, 3, 31)

        return (red_end - d).days

    def simulate_season(
        self,
        consumptions: dict[date, float],
        start_date: date | None = None,
    ) -> dict[date, TempoColor]:
        """
        Simulate a full Tempo season using the algorithm.

        Args:
            consumptions: Dict mapping dates to normalized net consumption
            start_date: Optional start date (defaults to Sept 1)

        Returns:
            Dict mapping dates to predicted colors
        """
        if not consumptions:
            return {}

        sorted_dates = sorted(consumptions.keys())
        if start_date is None:
            start_date = sorted_dates[0]

        state = TempoState()
        results = {}

        for d in sorted_dates:
            if d < start_date:
                continue

            normalized = consumptions[d]
            color, state = self.determine_color(d, normalized, state)
            results[d] = color

        return results

    def predict_with_thresholds(
        self,
        d: date,
        state: TempoState,
    ) -> dict:
        """
        Get prediction information including thresholds.

        Returns:
            Dict with thresholds and required consumption for each color
        """
        tempo_day = get_tempo_day_number(d)

        threshold_red = self.calculate_threshold_red(tempo_day, state.stock_red)
        threshold_white_red = self.calculate_threshold_white_red(
            tempo_day, state.stock_red, state.stock_white
        )

        return {
            "date": d.isoformat(),
            "tempo_day": tempo_day,
            "threshold_red": threshold_red,
            "threshold_white_red": threshold_white_red,
            "can_be_red": can_be_red(d, state.consecutive_red),
            "can_be_white": can_be_white(d),
            "stock_red": state.stock_red,
            "stock_white": state.stock_white,
            # Convert thresholds back to MW for interpretability
            "consumption_for_red_mw": threshold_red * self.normalization_std
            + self.normalization_mean,
            "consumption_for_white_mw": threshold_white_red * self.normalization_std
            + self.normalization_mean,
        }


def estimate_consumption_from_temperature(
    temperature: float,
    day_of_week: int,
    month: int,
    base_consumption: float = 46050,  # RTE normalization mean
) -> float:
    """
    Estimate NET consumption from temperature (simplified model).

    NET consumption = Gross consumption - Wind - Solar

    IMPORTANT: Without wind/solar forecast data, we cannot accurately
    predict net consumption. This function provides a conservative estimate
    that will tend toward BLUE days (the most common outcome).

    For accurate RED/WHITE predictions, we would need:
    - Wind production forecast (can be 5-15 GW in winter)
    - Solar production forecast (1-4 GW depending on season)

    Args:
        temperature: Mean daily temperature in °C
        day_of_week: 0=Monday, 6=Sunday
        month: 1-12
        base_consumption: Base NET consumption in MW (RTE mean = 46050)

    Returns:
        Estimated NET consumption in MW (conservative estimate)
    """
    # For a day to be RED, normalized consumption must exceed ~3.15 - adjustments
    # For a day to be WHITE, it must exceed ~4.00 - adjustments (or be above white threshold)
    # Most days are BLUE, so our baseline should predict BLUE

    # Temperature reference (French yearly average)
    temp_ref = 12.0  # °C

    # Conservative thermosensitivity for NET consumption
    # Real gross sensitivity is ~1300 MW/°C, but wind often increases when cold
    # so net sensitivity is much lower
    net_thermo_sensitivity = 350  # MW/°C (very conservative)

    temp_effect = (temp_ref - temperature) * net_thermo_sensitivity

    # Weekly pattern: lower on weekends
    weekly_factor = 0.98 if day_of_week >= 5 else 1.0

    estimated = (base_consumption + temp_effect) * weekly_factor

    # Keep close to mean - without wind data, we can't predict extremes
    return max(44000, min(50000, estimated))


if __name__ == "__main__":
    # Test the algorithm
    algo = TempoAlgorithm()

    # Test normalization
    test_consumption = 50000  # MW
    normalized = algo.normalize_consumption(test_consumption)
    print(f"Consumption: {test_consumption} MW -> Normalized: {normalized:.2f}")

    # Test threshold calculation
    state = TempoState()
    info = algo.predict_with_thresholds(date.today(), state)
    print("\nThreshold info for today:")
    for k, v in info.items():
        print(f"  {k}: {v}")
