#!/usr/bin/env python3
"""
Calibration script for the Hybrid Tempo predictor.

Calibrates the temperature-to-consumption relationship using historical data
from RTE and Open-Meteo, then persists the calibration parameters.
"""

import argparse
from datetime import date

from .hybrid_predictor import HybridTempoPredictor


def main():
    parser = argparse.ArgumentParser(description="Calibrate the Hybrid Tempo predictor")
    parser.add_argument(
        "--start-year",
        type=int,
        default=2015,
        help="First season year to use for calibration (default: 2015)",
    )
    parser.add_argument(
        "--backtest", action="store_true", help="Run multi-season backtest after calibration"
    )
    parser.add_argument(
        "--backtest-from", type=int, default=2020, help="First season to backtest (default: 2020)"
    )

    args = parser.parse_args()

    print("=" * 60)
    print("TEMPO HYBRID PREDICTOR - Calibration")
    print("=" * 60)

    predictor = HybridTempoPredictor(auto_load=False)

    # Calibrate
    calibration = predictor.calibrate(start_year=args.start_year)

    if not calibration:
        print("\nCalibration failed - no data available")
        return

    # Optionally run backtest
    if args.backtest:
        print("\n" + "=" * 60)
        print("MULTI-SEASON VALIDATION")
        print("=" * 60)

        current_year = date.today().year
        for year in range(args.backtest_from, current_year + 1):
            season = f"{year}-{year + 1}"
            try:
                results = predictor.backtest(season)

                if "error" in results:
                    print(f"\n{season}: Error - {results['error']}")
                    continue

                print(f"\n{season}:")
                print(
                    f"  Accuracy: {results['accuracy']:.1%}"
                    f" ({results['correct']}/{results['total']})"
                )
                red = results["red_metrics"]
                print(f"  RED: P={red['precision']:.1%} R={red['recall']:.1%} F1={red['f1']:.2f}")
                white = results["white_metrics"]
                print(
                    f"  WHITE: P={white['precision']:.1%}"
                    f" R={white['recall']:.1%} F1={white['f1']:.2f}"
                )
            except Exception as e:
                print(f"\n{season}: Error - {e}")

    print("\n" + "=" * 60)
    print("Calibration complete!")
    print("=" * 60)
    print("\nTo start the prediction server:")
    print("  python -m tempo_prediction.server")


if __name__ == "__main__":
    main()
