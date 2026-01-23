#!/usr/bin/env python3
"""
Hybrid predictor combining RTE algorithm with ML-based confidence.

This approach:
1. Uses the deterministic RTE algorithm as the base
2. Calibrates temperature -> consumption relationship from historical data
3. Integrates renewable production data (wind/solar) for better estimation
4. Persists calibration parameters between runs
"""

import json
from dataclasses import dataclass, asdict, field
from datetime import date, timedelta
from pathlib import Path
from typing import Optional

import pandas as pd
import numpy as np

from .algorithm import TempoAlgorithm, TempoState
from .constants import (
    CACHE_DIR,
    STOCK_RED_DAYS,
    STOCK_WHITE_DAYS,
    TempoColor,
    can_be_red,
    can_be_white,
    get_tempo_day_number,
    get_tempo_year,
    is_in_red_period,
)
from .data_collector import TempoDataCollector

# Path for calibration parameters persistence
CALIBRATION_FILE = Path(CACHE_DIR) / "calibration_params.json"


@dataclass
class CalibrationParams:
    """Calibrated parameters for consumption estimation."""
    base_consumption: float = 46050  # MW
    thermosensitivity: float = 1900  # MW per degree below reference (calibrated)
    temp_reference: float = 12.0  # Reference temperature
    weekend_factor: float = 0.92  # Weekend reduction
    
    # Renewable production adjustment
    renewable_factor: float = 0.12  # How much renewables reduce effective consumption
    
    # Calibration metadata
    calibration_date: Optional[str] = None
    calibration_accuracy: float = 0.0
    calibration_red_recall: float = 0.0
    
    # Monthly adjustments (relative to base) - Conservative estimates
    # These are tuned to produce fewer false RED predictions
    monthly_adjustments: dict = field(default_factory=lambda: {
        1: 0.98,   # January - slightly reduced (wind often high)
        2: 0.97,   # February
        3: 0.93,   # March
        4: 0.88,   # April - spring
        5: 0.83,   # May
        6: 0.80,   # June
        7: 0.78,   # July - summer minimum
        8: 0.80,   # August
        9: 0.85,   # September
        10: 0.90,  # October
        11: 0.95,  # November - start of heating
        12: 0.97,  # December
    })
    
    def to_dict(self) -> dict:
        """Convert to JSON-serializable dict."""
        return {
            "base_consumption": self.base_consumption,
            "thermosensitivity": self.thermosensitivity,
            "temp_reference": self.temp_reference,
            "weekend_factor": self.weekend_factor,
            "renewable_factor": self.renewable_factor,
            "calibration_date": self.calibration_date,
            "calibration_accuracy": self.calibration_accuracy,
            "calibration_red_recall": self.calibration_red_recall,
            "monthly_adjustments": {str(k): v for k, v in self.monthly_adjustments.items()},
        }
    
    @classmethod
    def from_dict(cls, data: dict) -> "CalibrationParams":
        """Create from JSON dict."""
        monthly = data.get("monthly_adjustments", {})
        # Convert string keys back to int
        monthly_int = {int(k): v for k, v in monthly.items()}
        
        return cls(
            base_consumption=data.get("base_consumption", 46050),
            thermosensitivity=data.get("thermosensitivity", 1900),
            temp_reference=data.get("temp_reference", 12.0),
            weekend_factor=data.get("weekend_factor", 0.92),
            renewable_factor=data.get("renewable_factor", 0.12),
            calibration_date=data.get("calibration_date"),
            calibration_accuracy=data.get("calibration_accuracy", 0.0),
            calibration_red_recall=data.get("calibration_red_recall", 0.0),
            monthly_adjustments=monthly_int if monthly_int else None,
        )


class HybridTempoPredictor:
    """
    Hybrid predictor using calibrated RTE algorithm.
    
    The key insight: RTE uses a deterministic algorithm based on consumption.
    If we can accurately estimate consumption from temperature (and optionally
    renewable production), we can predict colors with high accuracy.
    """
    
    def __init__(self, collector: Optional[TempoDataCollector] = None, auto_load: bool = True):
        self.collector = collector or TempoDataCollector()
        self.algorithm = TempoAlgorithm()
        self.params = CalibrationParams()
        self._calibrated = False
        
        # Try to load saved calibration
        if auto_load:
            self.load_calibration()
    
    def save_calibration(self) -> bool:
        """Save calibration parameters to disk."""
        try:
            CALIBRATION_FILE.parent.mkdir(parents=True, exist_ok=True)
            with open(CALIBRATION_FILE, "w") as f:
                json.dump(self.params.to_dict(), f, indent=2)
            print(f"Calibration saved to {CALIBRATION_FILE}")
            return True
        except Exception as e:
            print(f"Failed to save calibration: {e}")
            return False
    
    def load_calibration(self) -> bool:
        """Load calibration parameters from disk."""
        if not CALIBRATION_FILE.exists():
            return False
        
        try:
            with open(CALIBRATION_FILE, "r") as f:
                data = json.load(f)
            self.params = CalibrationParams.from_dict(data)
            self._calibrated = True
            print(f"Loaded calibration from {CALIBRATION_FILE}")
            print(f"  Thermosensitivity: {self.params.thermosensitivity} MW/C")
            print(f"  Accuracy: {self.params.calibration_accuracy:.1%}")
            print(f"  RED recall: {self.params.calibration_red_recall:.1%}")
            return True
        except Exception as e:
            print(f"Failed to load calibration: {e}")
            return False
    
    def calibrate(self, start_year: int = 2015, save: bool = True) -> dict:
        """
        Calibrate parameters from historical data.
        
        Finds the thermosensitivity that best matches historical colors.
        """
        print("Calibrating predictor from historical data...")
        
        # Fetch all historical data
        tempo_df = self.collector.fetch_tempo_history_all_seasons(start_year)
        if tempo_df.empty:
            print("No historical data available")
            return {}
        
        tempo_df["date"] = pd.to_datetime(tempo_df["date"])
        start_date = tempo_df["date"].min().date()
        end_date = tempo_df["date"].max().date()
        
        # Fetch temperature in chunks (Open-Meteo has date range limits)
        print("Fetching temperature data in chunks...")
        all_temps = []
        current_start = start_date
        
        while current_start < end_date:
            current_end = min(current_start + timedelta(days=365), end_date)
            chunk_df = self.collector.fetch_temperature_history(current_start, current_end)
            if not chunk_df.empty:
                all_temps.append(chunk_df)
            current_start = current_end + timedelta(days=1)
        
        if not all_temps:
            print("No temperature data available")
            return {}
        
        temp_df = pd.concat(all_temps, ignore_index=True)
        temp_df = temp_df.drop_duplicates(subset=["date"])
        temp_df["date"] = pd.to_datetime(temp_df["date"])
        
        print(f"Got {len(temp_df)} temperature records")
        
        # Merge
        df = tempo_df.merge(temp_df, on="date", how="left")
        df = df.dropna(subset=["temperature_mean", "color"])
        
        print(f"Calibrating on {len(df)} samples...")
        
        # Grid search for best thermosensitivity
        best_score = 0
        best_thermo = 1100  # More reasonable default
        best_accuracy = 0
        best_red_recall = 0
        best_red_precision = 0
        
        for thermo in range(600, 1800, 25):
            self.params.thermosensitivity = thermo
            accuracy, red_recall, red_precision, red_predicted = self._evaluate_on_history(df)
            
            # Use F1-score for RED class as main metric
            # This balances recall (don't miss RED) with precision (don't cry wolf)
            if red_recall + red_precision > 0:
                red_f1 = 2 * red_recall * red_precision / (red_recall + red_precision)
            else:
                red_f1 = 0
            
            # Combined score: prioritize F1 but also consider overall accuracy
            score = red_f1 * 0.7 + accuracy * 0.3
            
            if score > best_score:
                best_score = score
                best_accuracy = accuracy
                best_red_recall = red_recall
                best_red_precision = red_precision
                best_thermo = thermo
            
            # Debug output for interesting values
            if thermo % 100 == 0:
                print(f"  thermo={thermo}: acc={accuracy:.1%}, RED P={red_precision:.1%} R={red_recall:.1%} F1={red_f1:.2f}, predicted={red_predicted}")
        
        self.params.thermosensitivity = best_thermo
        self.params.calibration_date = date.today().isoformat()
        self.params.calibration_accuracy = best_accuracy
        self.params.calibration_red_recall = best_red_recall
        self._calibrated = True
        
        print(f"\nCalibration complete:")
        print(f"  Best thermosensitivity: {best_thermo} MW/C")
        print(f"  Overall accuracy: {best_accuracy:.1%}")
        print(f"  RED recall: {best_red_recall:.1%}")
        print(f"  RED precision: {best_red_precision:.1%}")
        
        # Save calibration
        if save:
            self.save_calibration()
        
        return {
            "thermosensitivity": best_thermo,
            "accuracy": best_accuracy,
            "red_recall": best_red_recall,
            "red_precision": best_red_precision,
        }
    
    def _evaluate_on_history(self, df: pd.DataFrame) -> tuple[float, float, float, int]:
        """Evaluate current parameters on historical data.
        
        Returns:
            Tuple of (accuracy, red_recall, red_precision, red_predicted_count)
        """
        correct = 0
        total = 0
        red_correct = 0
        red_total = 0
        red_predicted = 0
        
        # Group by season
        df = df.copy()
        df["season"] = df["date"].apply(lambda d: get_tempo_year(d.date()))
        
        for season, season_df in df.groupby("season"):
            season_df = season_df.sort_values("date")
            state = TempoState()
            
            for _, row in season_df.iterrows():
                d = row["date"].date() if hasattr(row["date"], "date") else row["date"]
                temp = row["temperature_mean"]
                actual_color = row["color"]
                
                # Estimate consumption
                consumption = self._estimate_consumption(temp, d)
                
                # Normalize
                normalized = self.algorithm.normalize_consumption(consumption)
                
                # Predict
                predicted_color, new_state = self.algorithm.determine_color(
                    d, normalized, state
                )
                
                # Track accuracy
                if predicted_color == actual_color:
                    correct += 1
                total += 1
                
                # Track RED metrics
                if predicted_color == "RED":
                    red_predicted += 1
                    if actual_color == "RED":
                        red_correct += 1
                
                if actual_color == "RED":
                    red_total += 1
                
                # Update state with ACTUAL color (not predicted)
                if actual_color == "RED":
                    state.stock_red = max(0, state.stock_red - 1)
                    state.consecutive_red += 1
                else:
                    state.consecutive_red = 0
                    if actual_color == "WHITE":
                        state.stock_white = max(0, state.stock_white - 1)
        
        accuracy = correct / total if total > 0 else 0
        red_recall = red_correct / red_total if red_total > 0 else 0
        red_precision = red_correct / red_predicted if red_predicted > 0 else 0
        
        return accuracy, red_recall, red_precision, red_predicted
    
    def _estimate_consumption(
        self, 
        temperature: float, 
        d: date,
        wind_production: Optional[float] = None,
        solar_production: Optional[float] = None,
    ) -> float:
        """
        Estimate consumption from temperature using calibrated parameters.
        
        Optionally integrates renewable production data for better accuracy.
        """
        # Base consumption
        base = self.params.base_consumption
        
        # Temperature effect (heating demand)
        temp_effect = (self.params.temp_reference - temperature) * self.params.thermosensitivity
        
        # Weekend factor
        if d.weekday() >= 5:
            weekend_factor = self.params.weekend_factor
        else:
            weekend_factor = 1.0
        
        # Monthly adjustment
        monthly_factor = self.params.monthly_adjustments.get(d.month, 1.0)
        
        # Calculate gross consumption
        gross_consumption = (base + temp_effect) * weekend_factor * monthly_factor
        
        # Renewable adjustment (if data available)
        renewable_reduction = 0
        if wind_production is not None or solar_production is not None:
            total_renewable = (wind_production or 0) + (solar_production or 0)
            renewable_reduction = total_renewable * self.params.renewable_factor
        
        consumption = gross_consumption - renewable_reduction
        
        # Clip to reasonable range
        return max(35000, min(75000, consumption))
    
    def predict(
        self,
        dates: list[date],
        temperatures: list[float],
        stock_red: int = STOCK_RED_DAYS,
        stock_white: int = STOCK_WHITE_DAYS,
        wind_production: Optional[list[float]] = None,
        solar_production: Optional[list[float]] = None,
    ) -> list[dict]:
        """
        Predict Tempo colors using calibrated RTE algorithm.
        
        Args:
            dates: List of dates to predict
            temperatures: Mean temperatures for each date
            stock_red: Remaining RED days in stock
            stock_white: Remaining WHITE days in stock
            wind_production: Optional wind production forecast (MW)
            solar_production: Optional solar production forecast (MW)
        """
        if not self._calibrated:
            print("Warning: Predictor not calibrated, using default parameters")
        
        state = TempoState(stock_red=stock_red, stock_white=stock_white)
        predictions = []
        
        for i, (d, temp) in enumerate(zip(dates, temperatures)):
            # Get renewable data if available
            wind = wind_production[i] if wind_production and i < len(wind_production) else None
            solar = solar_production[i] if solar_production and i < len(solar_production) else None
            
            # Estimate consumption
            consumption = self._estimate_consumption(temp, d, wind, solar)
            normalized = self.algorithm.normalize_consumption(consumption)
            
            # Get thresholds
            tempo_day = get_tempo_day_number(d)
            threshold_red = self.algorithm.calculate_threshold_red(tempo_day, state.stock_red)
            threshold_white = self.algorithm.calculate_threshold_white_red(
                tempo_day, state.stock_red, state.stock_white
            )
            
            # Calculate pseudo-probabilities based on distance to thresholds
            dist_to_red = normalized - threshold_red
            dist_to_white = normalized - threshold_white
            
            # Sigmoid-based probabilities
            def sigmoid(x, scale=1.5):
                return 1 / (1 + np.exp(-x * scale))
            
            if can_be_red(d, state.consecutive_red) and state.stock_red > 0:
                prob_red = float(sigmoid(dist_to_red))
            else:
                prob_red = 0.0
            
            if can_be_white(d) and state.stock_white > 0:
                prob_white = float(sigmoid(dist_to_white) * (1 - prob_red))
            else:
                prob_white = 0.0
            
            prob_blue = 1.0 - prob_red - prob_white
            
            # Predicted color is the one with highest probability
            probs = {"BLUE": prob_blue, "WHITE": prob_white, "RED": prob_red}
            color = max(probs, key=lambda k: probs[k])
            
            # Update state based on predicted color
            new_state = state.copy()
            if color == "RED":
                new_state.stock_red -= 1
                new_state.consecutive_red += 1
            else:
                new_state.consecutive_red = 0
                if color == "WHITE":
                    new_state.stock_white -= 1
            new_state.last_color = color
            
            # Build prediction object
            prediction = {
                "date": d.isoformat(),
                "predicted_color": color,
                "probabilities": {
                    "BLUE": max(0, prob_blue),
                    "WHITE": max(0, prob_white),
                    "RED": max(0, prob_red),
                },
                "confidence": max(prob_blue, prob_white, prob_red),
                "constraints": {
                    "can_be_red": can_be_red(d, state.consecutive_red),
                    "can_be_white": can_be_white(d),
                    "is_in_red_period": is_in_red_period(d),
                },
                "details": {
                    "temperature": temp,
                    "estimated_consumption": round(consumption, 0),
                    "normalized_consumption": round(normalized, 3),
                    "threshold_red": round(threshold_red, 3),
                    "threshold_white": round(threshold_white, 3),
                    "stock_red": state.stock_red,
                    "stock_white": state.stock_white,
                }
            }
            
            # Add renewable info if available
            if wind is not None or solar is not None:
                prediction["details"]["wind_production"] = wind
                prediction["details"]["solar_production"] = solar
            
            predictions.append(prediction)
            state = new_state
        
        return predictions
    
    def predict_week(
        self, 
        stock_red: int = STOCK_RED_DAYS, 
        stock_white: int = STOCK_WHITE_DAYS,
    ) -> list[dict]:
        """
        Predict next 7 days using weather forecast.
        
        This is the main entry point for the server.
        """
        # Fetch weather forecast
        weather_df = self.collector.fetch_temperature_forecast(days=7)
        
        if weather_df.empty:
            print("Warning: No weather forecast available")
            return []
        
        # Extract dates and temperatures - ensure dates are date objects
        dates = []
        for d in weather_df["date"]:
            if isinstance(d, str):
                # Handle ISO format with or without time
                dates.append(date.fromisoformat(d[:10]))
            elif hasattr(d, 'date') and callable(getattr(d, 'date')):
                dates.append(d.date())
            elif isinstance(d, date):
                dates.append(d)
            else:
                dates.append(date.fromisoformat(str(d)[:10]))
        
        temperatures = weather_df["temperature_mean"].tolist()
        
        return self.predict(dates, temperatures, stock_red, stock_white)
    
    def backtest(self, test_season: str = "2024-2025") -> dict:
        """Run backtest on a specific season."""
        print(f"\nBacktesting on season {test_season}...")
        
        # Parse season
        start_year = int(test_season.split("-")[0])
        end_year = int(test_season.split("-")[1])
        season_start = date(start_year, 9, 1)
        season_end = min(date(end_year, 8, 31), date.today() - timedelta(days=1))
        
        # Fetch data
        history = self.collector.fetch_tempo_history(test_season)
        temp_df = self.collector.fetch_temperature_history(season_start, season_end)
        
        if not history or temp_df.empty:
            return {"error": "Missing data"}
        
        temp_df["date"] = pd.to_datetime(temp_df["date"]).dt.date
        temp_dict = dict(zip(temp_df["date"], temp_df["temperature_mean"]))
        
        # Initialize state
        state = TempoState()
        
        # Track results
        confusion = {
            "BLUE": {"BLUE": 0, "WHITE": 0, "RED": 0},
            "WHITE": {"BLUE": 0, "WHITE": 0, "RED": 0},
            "RED": {"BLUE": 0, "WHITE": 0, "RED": 0},
        }
        
        results = []
        current = season_start
        
        while current <= season_end:
            date_str = current.isoformat()
            if date_str not in history:
                current += timedelta(days=1)
                continue
            
            actual_color = history[date_str]
            if actual_color not in ["BLUE", "WHITE", "RED"]:
                current += timedelta(days=1)
                continue
            
            # Get temperature
            temp = temp_dict.get(current, 8.0)
            
            # Predict
            consumption = self._estimate_consumption(temp, current)
            normalized = self.algorithm.normalize_consumption(consumption)
            predicted_color, _ = self.algorithm.determine_color(current, normalized, state)
            
            # Record
            confusion[actual_color][predicted_color] += 1
            results.append({
                "date": date_str,
                "actual": actual_color,
                "predicted": predicted_color,
                "correct": actual_color == predicted_color,
                "temperature": temp,
            })
            
            # Update state with ACTUAL color
            if actual_color == "RED":
                state.stock_red = max(0, state.stock_red - 1)
                state.consecutive_red += 1
            else:
                state.consecutive_red = 0
                if actual_color == "WHITE":
                    state.stock_white = max(0, state.stock_white - 1)
            
            current += timedelta(days=1)
        
        # Calculate metrics
        total = len(results)
        correct = sum(1 for r in results if r["correct"])
        accuracy = correct / total if total > 0 else 0
        
        red_predicted = confusion["BLUE"]["RED"] + confusion["WHITE"]["RED"] + confusion["RED"]["RED"]
        red_actual = confusion["RED"]["BLUE"] + confusion["RED"]["WHITE"] + confusion["RED"]["RED"]
        red_correct = confusion["RED"]["RED"]
        
        white_predicted = confusion["BLUE"]["WHITE"] + confusion["WHITE"]["WHITE"] + confusion["RED"]["WHITE"]
        white_actual = confusion["BLUE"]["WHITE"] + confusion["WHITE"]["WHITE"] + confusion["RED"]["WHITE"]
        white_correct = confusion["WHITE"]["WHITE"]
        
        red_precision = red_correct / red_predicted if red_predicted > 0 else 0
        red_recall = red_correct / red_actual if red_actual > 0 else 0
        red_f1 = 2 * red_precision * red_recall / (red_precision + red_recall) if (red_precision + red_recall) > 0 else 0
        
        white_precision = white_correct / white_predicted if white_predicted > 0 else 0
        white_actual_count = confusion["WHITE"]["BLUE"] + confusion["WHITE"]["WHITE"] + confusion["WHITE"]["RED"]
        white_recall = white_correct / white_actual_count if white_actual_count > 0 else 0
        white_f1 = 2 * white_precision * white_recall / (white_precision + white_recall) if (white_precision + white_recall) > 0 else 0
        
        return {
            "season": test_season,
            "total": total,
            "correct": correct,
            "accuracy": accuracy,
            "confusion": confusion,
            "red_metrics": {
                "precision": red_precision,
                "recall": red_recall,
                "f1": red_f1,
            },
            "white_metrics": {
                "precision": white_precision,
                "recall": white_recall,
                "f1": white_f1,
            },
            "details": results,
        }
    
    def backtest_all_seasons(self, start_year: int = 2020) -> dict:
        """Run backtest on all seasons from start_year to current."""
        current_year = date.today().year
        all_results = {}
        
        for year in range(start_year, current_year + 1):
            season = f"{year}-{year + 1}"
            try:
                result = self.backtest(season)
                if "error" not in result:
                    all_results[season] = result
            except Exception as e:
                print(f"Error backtesting {season}: {e}")
        
        # Calculate aggregate metrics
        total_days = sum(r["total"] for r in all_results.values())
        total_correct = sum(r["correct"] for r in all_results.values())
        total_red_recall = sum(r["red_metrics"]["recall"] * r["total"] for r in all_results.values())
        
        return {
            "seasons": all_results,
            "aggregate": {
                "total_days": total_days,
                "total_correct": total_correct,
                "overall_accuracy": total_correct / total_days if total_days > 0 else 0,
                "avg_red_recall": total_red_recall / total_days if total_days > 0 else 0,
            }
        }
    
    def get_season_history(self, season: str = None) -> list[dict]:
        """
        Get complete history for a season with actual colors.
        
        Returns a list suitable for calendar display.
        """
        if season is None:
            start_year, end_year = get_tempo_year(date.today())
            season = f"{start_year}-{end_year}"
        
        history = self.collector.fetch_tempo_history(season)
        
        # Convert to list format
        result = []
        for date_str, color in sorted(history.items()):
            if color in ["BLUE", "WHITE", "RED"]:
                result.append({
                    "date": date_str,
                    "color": color,
                    "is_actual": True,
                })
        
        return result
    
    def get_calibration_info(self) -> dict:
        """Get current calibration parameters and status."""
        return {
            "calibrated": self._calibrated,
            "params": self.params.to_dict(),
        }


def main():
    """Test the hybrid predictor with multi-season validation."""
    print("=" * 60)
    print("HYBRID TEMPO PREDICTOR - Calibration & Multi-Season Validation")
    print("=" * 60)
    
    predictor = HybridTempoPredictor(auto_load=False)
    
    # Calibrate
    calibration = predictor.calibrate(start_year=2015)
    
    # Backtest on multiple seasons
    print("\n" + "=" * 60)
    print("MULTI-SEASON VALIDATION")
    print("=" * 60)
    
    seasons_to_test = ["2022-2023", "2023-2024", "2024-2025"]
    
    for season in seasons_to_test:
        results = predictor.backtest(season)
        
        if "error" in results:
            print(f"\n{season}: Error - {results['error']}")
            continue
        
        print(f"\n{season}:")
        print(f"  Accuracy: {results['accuracy']:.1%} ({results['correct']}/{results['total']})")
        print(f"  RED: P={results['red_metrics']['precision']:.1%} R={results['red_metrics']['recall']:.1%} F1={results['red_metrics']['f1']:.2f}")
        print(f"  WHITE: P={results['white_metrics']['precision']:.1%} R={results['white_metrics']['recall']:.1%} F1={results['white_metrics']['f1']:.2f}")
        
        print(f"\n  Confusion Matrix:")
        confusion = results["confusion"]
        print(f"             BLUE   WHITE    RED  (Predicted)")
        for actual in ["BLUE", "WHITE", "RED"]:
            row = f"    {actual:>5}"
            for pred in ["BLUE", "WHITE", "RED"]:
                row += f"  {confusion[actual][pred]:>5}"
            print(row)


if __name__ == "__main__":
    main()
