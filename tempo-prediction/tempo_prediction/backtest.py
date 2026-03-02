"""
Backtesting module for the Hybrid Tempo predictor.
Evaluates prediction accuracy on historical seasons.
"""

import argparse
from datetime import date

from .hybrid_predictor import HybridTempoPredictor


def main():
    """Run backtesting on historical seasons."""
    parser = argparse.ArgumentParser(description="Backtest the Hybrid Tempo predictor")
    parser.add_argument(
        "--season",
        type=str,
        default=None,
        help="Specific season to test (e.g., 2024-2025). Default: test multiple seasons.",
    )
    parser.add_argument(
        "--from-year",
        type=int,
        default=2020,
        help="First season year for multi-season backtest (default: 2020)",
    )
    args = parser.parse_args()

    print("=" * 60)
    print("TEMPO HYBRID PREDICTOR - Backtest")
    print("=" * 60)

    predictor = HybridTempoPredictor(auto_load=True)

    if not predictor._calibrated:
        print("Predictor not calibrated. Running calibration first...")
        predictor.calibrate(start_year=2015, save=True)

    if args.season:
        # Single season backtest
        results = predictor.backtest(args.season)
        _print_season_results(args.season, results)
    else:
        # Multi-season backtest
        current_year = date.today().year
        for year in range(args.from_year, current_year + 1):
            season = f"{year}-{year + 1}"
            try:
                results = predictor.backtest(season)
                _print_season_results(season, results)
            except Exception as e:
                print(f"\n{season}: Error - {e}")


def _print_season_results(season: str, results: dict):
    """Print backtest results for a season."""
    if "error" in results:
        print(f"\n{season}: Error - {results['error']}")
        return

    print(f"\n{season}:")
    print(f"  Accuracy: {results['accuracy']:.1%} ({results['correct']}/{results['total']})")
    red = results["red_metrics"]
    print(f"  RED: P={red['precision']:.1%} R={red['recall']:.1%} F1={red['f1']:.2f}")
    white = results["white_metrics"]
    print(f"  WHITE: P={white['precision']:.1%} R={white['recall']:.1%} F1={white['f1']:.2f}")

    print("\n  Confusion Matrix:")
    confusion = results["confusion"]
    print("             BLUE   WHITE    RED  (Predicted)")
    for actual in ["BLUE", "WHITE", "RED"]:
        row = f"    {actual:>5}"
        for pred in ["BLUE", "WHITE", "RED"]:
            row += f"  {confusion[actual][pred]:>5}"
        print(row)


if __name__ == "__main__":
    main()
