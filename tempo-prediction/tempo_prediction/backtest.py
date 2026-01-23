"""
Backtesting module for Tempo prediction model.
Evaluates model accuracy on historical data.
"""

import argparse
import json
from datetime import date, timedelta
from pathlib import Path
from typing import Optional

import numpy as np
import pandas as pd

from .algorithm import TempoAlgorithm, TempoState
from .constants import (
    STOCK_RED_DAYS,
    STOCK_WHITE_DAYS,
    get_tempo_year,
)
from .data_collector import TempoDataCollector
from .predictor import TempoPredictor

try:
    from .mlx_predictor import MLXTempoPredictor
    MLX_AVAILABLE = True
except ImportError:
    MLX_AVAILABLE = False


class TempoBacktester:
    """
    Backtesting framework for Tempo prediction models.
    
    Evaluates:
    - Overall accuracy
    - Per-color accuracy
    - Precision/Recall for RED days (most important)
    - Accuracy by prediction horizon (J+1, J+2, ..., J+7)
    """

    def __init__(self):
        self.collector = TempoDataCollector()
        self.algorithm = TempoAlgorithm()
        self.predictor = TempoPredictor()

    def run_backtest(
        self,
        start_date: date,
        end_date: date,
        prediction_horizon: int = 7,
    ) -> dict:
        """
        Run backtesting on historical data.
        
        Args:
            start_date: Start of backtest period
            end_date: End of backtest period
            prediction_horizon: Max days ahead to predict (1-7)
        
        Returns:
            Dict with detailed metrics
        """
        print(f"Running backtest from {start_date} to {end_date}")
        print(f"Prediction horizon: J+1 to J+{prediction_horizon}")
        
        # Load historical Tempo colors
        all_history = {}
        current_year = end_date.year
        for year in range(start_date.year - 1, current_year + 1):
            season = f"{year}-{year + 1}"
            history = self.collector.fetch_tempo_history(season)
            all_history.update(history)
        
        print(f"Loaded {len(all_history)} historical days")
        
        # Load temperature history
        temp_df = self.collector.fetch_temperature_history(start_date, end_date)
        temp_dict = {}
        if not temp_df.empty:
            for _, row in temp_df.iterrows():
                temp_dict[row["date"].strftime("%Y-%m-%d")] = row["temperature_mean"]
        
        print(f"Loaded {len(temp_dict)} temperature records")
        
        # Ensure model is loaded
        if not self.predictor.model:
            if not self.predictor.load_model():
                print("Training model for backtest...")
                self.predictor.train(start_year=2015, save_model=True)
        
        # Run predictions for each day
        results = []
        current = start_date
        
        while current <= end_date - timedelta(days=prediction_horizon):
            for horizon in range(1, prediction_horizon + 1):
                target_date = current + timedelta(days=horizon)
                target_str = target_date.isoformat()
                
                if target_str not in all_history:
                    continue
                
                actual_color = all_history[target_str]
                if actual_color not in ["BLUE", "WHITE", "RED"]:
                    continue
                
                # Get temperature (use historical data as if it were forecast)
                temp = temp_dict.get(target_str, 10.0)
                
                # Make prediction
                try:
                    predictions = self.predictor.predict(
                        [target_date],
                        [temp],
                    )
                    if predictions:
                        pred = predictions[0]
                        results.append({
                            "prediction_date": current.isoformat(),
                            "target_date": target_str,
                            "horizon": horizon,
                            "actual": actual_color,
                            "predicted": pred["predicted_color"],
                            "confidence": pred["confidence"],
                            "prob_blue": pred["probabilities"]["BLUE"],
                            "prob_white": pred["probabilities"]["WHITE"],
                            "prob_red": pred["probabilities"]["RED"],
                        })
                except Exception as e:
                    print(f"Error predicting {target_date}: {e}")
                    continue
            
            current += timedelta(days=7)  # Move by week for efficiency
        
        if not results:
            return {"error": "No predictions made"}
        
        # Analyze results
        df = pd.DataFrame(results)
        
        metrics = self._calculate_metrics(df)
        
        return metrics

    def _calculate_metrics(self, df: pd.DataFrame) -> dict:
        """Calculate comprehensive metrics from backtest results."""
        
        # Overall accuracy
        df["correct"] = df["actual"] == df["predicted"]
        overall_accuracy = df["correct"].mean()
        
        # Accuracy by horizon
        accuracy_by_horizon = df.groupby("horizon")["correct"].mean().to_dict()
        
        # Accuracy by color
        accuracy_by_color = {}
        for color in ["BLUE", "WHITE", "RED"]:
            color_df = df[df["actual"] == color]
            if len(color_df) > 0:
                accuracy_by_color[color] = {
                    "accuracy": color_df["correct"].mean(),
                    "count": len(color_df),
                }
        
        # Precision/Recall for RED (most important)
        red_precision = len(df[(df["predicted"] == "RED") & (df["actual"] == "RED")]) / max(1, len(df[df["predicted"] == "RED"]))
        red_recall = len(df[(df["predicted"] == "RED") & (df["actual"] == "RED")]) / max(1, len(df[df["actual"] == "RED"]))
        red_f1 = 2 * (red_precision * red_recall) / max(0.001, red_precision + red_recall)
        
        # Confusion matrix
        confusion = pd.crosstab(
            df["actual"],
            df["predicted"],
            rownames=["Actual"],
            colnames=["Predicted"],
        ).to_dict()
        
        # Average confidence by correctness
        avg_confidence_correct = df[df["correct"]]["confidence"].mean()
        avg_confidence_incorrect = df[~df["correct"]]["confidence"].mean()
        
        # High confidence predictions
        high_conf_df = df[df["confidence"] > 0.7]
        high_conf_accuracy = high_conf_df["correct"].mean() if len(high_conf_df) > 0 else 0
        
        return {
            "total_predictions": len(df),
            "overall_accuracy": float(overall_accuracy),
            "accuracy_by_horizon": {str(k): float(v) for k, v in accuracy_by_horizon.items()},
            "accuracy_by_color": accuracy_by_color,
            "red_day_metrics": {
                "precision": float(red_precision),
                "recall": float(red_recall),
                "f1_score": float(red_f1),
            },
            "confusion_matrix": confusion,
            "confidence_analysis": {
                "avg_confidence_correct": float(avg_confidence_correct) if not np.isnan(avg_confidence_correct) else 0,
                "avg_confidence_incorrect": float(avg_confidence_incorrect) if not np.isnan(avg_confidence_incorrect) else 0,
                "high_confidence_accuracy": float(high_conf_accuracy),
                "high_confidence_count": len(high_conf_df),
            },
        }

    def compare_with_baseline(
        self,
        start_date: date,
        end_date: date,
    ) -> dict:
        """
        Compare ML model with the baseline RTE algorithm.
        
        The baseline uses temperature to estimate consumption,
        then applies the RTE algorithm.
        """
        print("Comparing ML model with RTE algorithm baseline...")
        
        # This would require implementing a full baseline comparison
        # For now, return placeholder
        return {
            "message": "Baseline comparison not yet implemented",
            "description": "Would compare ML predictions vs RTE algorithm using weather forecasts",
        }


def main():
    """Run backtesting."""
    parser = argparse.ArgumentParser(description="Backtest Tempo prediction model")
    parser.add_argument(
        "--model",
        type=str,
        choices=["xgboost", "mlx"],
        default="mlx",
        help="Model to backtest"
    )
    parser.add_argument(
        "--season",
        type=str,
        default=None,
        help="Specific season to test (e.g., 2024-2025)"
    )
    args = parser.parse_args()
    
    print("=" * 60)
    print("🔮 TEMPO PREDICTION BACKTEST")
    print("=" * 60)
    
    if args.model == "mlx" and MLX_AVAILABLE:
        print(f"\n📂 Loading MLX LSTM model...")
        mlx_predictor = MLXTempoPredictor()
        if not mlx_predictor.load():
            print("❌ No MLX model found. Train first with:")
            print("   python -m tempo_prediction.train --model mlx")
            return
        print("✅ MLX model loaded")
        
        # Run MLX backtest
        run_mlx_backtest(mlx_predictor, args.season)
    else:
        # Original XGBoost backtest
        backtester = TempoBacktester()
        end_date = date.today() - timedelta(days=1)
        start_date = end_date - timedelta(days=365)
        
        metrics = backtester.run_backtest(start_date, end_date)
        print_metrics(metrics)


def run_mlx_backtest(predictor: "MLXTempoPredictor", test_season: Optional[str] = None):
    """Run backtest specifically for MLX LSTM model."""
    collector = TempoDataCollector()
    
    print("\n📥 Fetching historical data...")
    history_df = collector.fetch_tempo_history_all_seasons(start_year=2015)
    print(f"   Total samples: {len(history_df)}")
    
    # Determine test season
    if test_season is None:
        test_season = "2024-2025"
    
    start_year = int(test_season.split("-")[0])
    end_year = int(test_season.split("-")[1])
    
    season_start = date(start_year, 9, 1)
    season_end = min(date(end_year, 8, 31), date.today() - timedelta(days=1))
    
    # Filter test data
    history_df = history_df.copy()
    history_df["date"] = pd.to_datetime(history_df["date"]).dt.date
    
    test_df = history_df[
        (history_df["date"] >= season_start) & 
        (history_df["date"] <= season_end)
    ].copy()
    
    # Training data (everything before test season)
    train_df = history_df[history_df["date"] < season_start].copy()
    
    print(f"\n📊 Backtesting on season {test_season}")
    print(f"   Test period: {season_start} to {season_end}")
    print(f"   Test samples: {len(test_df)}")
    print(f"   Training context: {len(train_df)} samples")
    
    if len(test_df) == 0:
        print("❌ No test data available")
        return
    
    # Track predictions
    results = []
    confusion = {"BLUE": {"BLUE": 0, "WHITE": 0, "RED": 0},
                 "WHITE": {"BLUE": 0, "WHITE": 0, "RED": 0},
                 "RED": {"BLUE": 0, "WHITE": 0, "RED": 0}}
    
    # Initial stocks
    stock_red = 22
    stock_white = 43
    
    # Build rolling context
    context_df = train_df.tail(predictor.sequence_length * 2).copy()
    
    print("\n⏳ Running predictions...")
    
    for idx, row in test_df.iterrows():
        d = row["date"]
        actual_color = row["color"]
        
        if actual_color not in ["BLUE", "WHITE", "RED"]:
            continue
        
        # Create prediction dataframe
        predict_df = pd.concat([
            context_df.tail(predictor.sequence_length),
            pd.DataFrame([{"date": d, "color": "BLUE"}])  # Placeholder color
        ], ignore_index=True)
        
        # Predict
        try:
            predictions = predictor.predict(
                predict_df,
                stock_red_remaining=stock_red,
                stock_white_remaining=stock_white
            )
            pred = predictions[-1]
            predicted_color = pred["predicted_color"]
            confidence = pred["confidence"]
            probs = pred["probabilities"]
        except Exception as e:
            print(f"   ⚠️ Error predicting {d}: {e}")
            predicted_color = "BLUE"
            confidence = 0.33
            probs = {"BLUE": 0.33, "WHITE": 0.33, "RED": 0.33}
        
        # Record result
        correct = predicted_color == actual_color
        results.append({
            "date": d,
            "actual": actual_color,
            "predicted": predicted_color,
            "correct": correct,
            "confidence": confidence,
            "prob_blue": probs["BLUE"],
            "prob_white": probs["WHITE"],
            "prob_red": probs["RED"],
        })
        
        confusion[actual_color][predicted_color] += 1
        
        # Update stocks based on actual color
        if actual_color == "RED":
            stock_red = max(0, stock_red - 1)
        elif actual_color == "WHITE":
            stock_white = max(0, stock_white - 1)
        
        # Add actual data to context for next prediction
        context_df = pd.concat([
            context_df,
            pd.DataFrame([row])
        ], ignore_index=True)
    
    # Calculate and print metrics
    results_df = pd.DataFrame(results)
    
    total = len(results_df)
    correct = results_df["correct"].sum()
    accuracy = correct / total if total > 0 else 0
    
    print(f"\n" + "=" * 60)
    print("📈 RESULTS")
    print("=" * 60)
    
    print(f"\n📊 Overall Accuracy: {accuracy:.1%} ({correct}/{total})")
    
    # Per-color metrics
    print("\n🎨 Accuracy by Color:")
    for color in ["BLUE", "WHITE", "RED"]:
        emoji = {"BLUE": "🔵", "WHITE": "⚪", "RED": "🔴"}[color]
        color_rows = results_df[results_df["actual"] == color]
        if len(color_rows) > 0:
            color_acc = color_rows["correct"].mean()
            print(f"   {emoji} {color}: {color_acc:.1%} ({color_rows['correct'].sum()}/{len(color_rows)})")
    
    # RED day metrics (precision/recall)
    red_predicted = len(results_df[results_df["predicted"] == "RED"])
    red_actual = len(results_df[results_df["actual"] == "RED"])
    red_correct = confusion["RED"]["RED"]
    
    if red_predicted > 0:
        red_precision = red_correct / red_predicted
    else:
        red_precision = 0
    
    if red_actual > 0:
        red_recall = red_correct / red_actual
    else:
        red_recall = 0
    
    if red_precision + red_recall > 0:
        red_f1 = 2 * red_precision * red_recall / (red_precision + red_recall)
    else:
        red_f1 = 0
    
    print("\n🔴 RED Day Metrics:")
    print(f"   Precision: {red_precision:.1%} (of predicted RED, how many were correct)")
    print(f"   Recall: {red_recall:.1%} (of actual RED, how many did we catch)")
    print(f"   F1 Score: {red_f1:.2f}")
    
    # Confusion matrix
    print("\n📋 Confusion Matrix:")
    print(f"   {'':>12} {'BLUE':>8} {'WHITE':>8} {'RED':>8}  (Predicted)")
    print(f"   {'Actual':>12} {'----':>8} {'-----':>8} {'---':>8}")
    for actual in ["BLUE", "WHITE", "RED"]:
        row = f"   {actual:>12}"
        for pred in ["BLUE", "WHITE", "RED"]:
            row += f" {confusion[actual][pred]:>8}"
        print(row)
    
    # Confidence analysis
    correct_conf = results_df[results_df["correct"]]["confidence"].mean()
    incorrect_conf = results_df[~results_df["correct"]]["confidence"].mean()
    
    print("\n💡 Confidence Analysis:")
    print(f"   Avg confidence (correct): {correct_conf:.1%}")
    print(f"   Avg confidence (incorrect): {incorrect_conf:.1%}")
    
    # Save results
    output_path = Path("backtest_results.json")
    metrics = {
        "season": test_season,
        "total": total,
        "correct": int(correct),
        "accuracy": float(accuracy),
        "confusion": confusion,
        "red_metrics": {
            "precision": float(red_precision),
            "recall": float(red_recall),
            "f1": float(red_f1)
        }
    }
    with open(output_path, "w") as f:
        json.dump(metrics, f, indent=2)
    print(f"\n📁 Results saved to {output_path}")


def print_metrics(metrics: dict):
    """Print metrics from XGBoost backtest."""
    print(f"\n📊 Overall Accuracy: {metrics.get('overall_accuracy', 0):.1%}")
    
    print("\n📈 Accuracy by Horizon:")
    for horizon, acc in sorted(metrics.get("accuracy_by_horizon", {}).items()):
        print(f"   J+{horizon}: {acc:.1%}")
    
    print("\n🎨 Accuracy by Color:")
    for color, data in metrics.get("accuracy_by_color", {}).items():
        emoji = {"BLUE": "🔵", "WHITE": "⚪", "RED": "🔴"}[color]
        print(f"   {emoji} {color}: {data['accuracy']:.1%} ({data['count']} samples)")
    
    print("\n🔴 RED Day Metrics:")
    red = metrics.get("red_day_metrics", {})
    print(f"   Precision: {red.get('precision', 0):.1%}")
    print(f"   Recall: {red.get('recall', 0):.1%}")
    print(f"   F1 Score: {red.get('f1_score', 0):.2f}")


if __name__ == "__main__":
    main()
