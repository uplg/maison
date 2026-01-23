#!/usr/bin/env python3
"""
Unified training script for Tempo prediction models.
Supports both XGBoost and MLX LSTM models.

IMPORTANT IMPROVEMENTS (Opus iteration):
- Fetches REAL historical temperature data from Open-Meteo
- Reconstructs stock levels by replaying history chronologically
- Data augmentation for RED days to handle class imbalance
"""

import argparse
import random
from datetime import date, timedelta
from pathlib import Path

import numpy as np
import pandas as pd

from .data_collector import TempoDataCollector
from .predictor import TempoPredictor
from .backtest import TempoBacktester
from .constants import (
    STOCK_RED_DAYS,
    STOCK_WHITE_DAYS,
    get_tempo_year,
    get_tempo_day_number,
)


def fetch_historical_temperatures(
    collector: TempoDataCollector,
    start_date: date,
    end_date: date
) -> pd.DataFrame:
    """
    Fetch real historical temperature data from Open-Meteo.
    Handles date ranges > 1 year by splitting into chunks.
    """
    print("   Fetching historical temperatures from Open-Meteo...")
    
    all_temps = []
    current_start = start_date
    
    # Open-Meteo allows ~2 years per request, chunk by 1 year to be safe
    while current_start < end_date:
        current_end = min(current_start + timedelta(days=365), end_date)
        
        temp_df = collector.fetch_temperature_history(current_start, current_end)
        if not temp_df.empty:
            all_temps.append(temp_df)
        
        current_start = current_end + timedelta(days=1)
    
    if all_temps:
        result = pd.concat(all_temps, ignore_index=True)
        result = result.drop_duplicates(subset=["date"])
        print(f"   Got {len(result)} temperature records")
        return result
    
    return pd.DataFrame()


def reconstruct_stock_levels(df: pd.DataFrame) -> pd.DataFrame:
    """
    Reconstruct stock levels by replaying the history chronologically.
    
    For each day, calculates:
    - stock_red_remaining: How many RED days left in this season
    - stock_white_remaining: How many WHITE days left in this season
    
    This is CRITICAL for the model to learn the relationship between
    stock levels and color decisions.
    """
    print("   Reconstructing stock levels from history...")
    
    df = df.copy()
    df = df.sort_values("date").reset_index(drop=True)
    
    # Ensure date is datetime
    df["date"] = pd.to_datetime(df["date"])
    
    # Track stocks per season
    stock_red = STOCK_RED_DAYS  # 22
    stock_white = STOCK_WHITE_DAYS  # 43
    current_season = None
    
    stock_red_list = []
    stock_white_list = []
    
    for idx, row in df.iterrows():
        d = row["date"].date() if hasattr(row["date"], "date") else row["date"]
        color = row["color"]
        
        # Get current season
        season_start, season_end = get_tempo_year(d)
        season_key = f"{season_start}-{season_end}"
        
        # Reset stocks at season change
        if season_key != current_season:
            stock_red = STOCK_RED_DAYS
            stock_white = STOCK_WHITE_DAYS
            current_season = season_key
        
        # Record current stock levels BEFORE this day's color is applied
        stock_red_list.append(stock_red)
        stock_white_list.append(stock_white)
        
        # Update stocks based on this day's color
        if color == "RED":
            stock_red = max(0, stock_red - 1)
        elif color == "WHITE":
            stock_white = max(0, stock_white - 1)
    
    df["stock_red_remaining"] = stock_red_list
    df["stock_white_remaining"] = stock_white_list
    
    print(f"   Stock levels reconstructed for {len(df)} days")
    return df


def augment_minority_classes(
    df: pd.DataFrame,
    target_samples_per_class: int = 150,
    temperature_noise_std: float = 1.5
) -> pd.DataFrame:
    """
    Augment RED and WHITE samples to address class imbalance.
    
    Strategy:
    - Duplicate minority class samples with small temperature variations
    - This simulates similar weather conditions that would produce same color
    
    Args:
        df: Training dataframe with temperature column
        target_samples_per_class: Minimum samples per class after augmentation
        temperature_noise_std: Standard deviation for temperature noise
    
    Returns:
        Augmented dataframe
    """
    print("   Performing data augmentation for minority classes...")
    
    df = df.copy()
    
    # Count current samples per class
    class_counts = df["color"].value_counts()
    print(f"   Before augmentation: {dict(class_counts)}")
    
    augmented_rows = []
    
    for color in ["RED", "WHITE"]:
        current_count = class_counts.get(color, 0)
        if current_count == 0:
            continue
        
        samples_needed = target_samples_per_class - current_count
        if samples_needed <= 0:
            continue
        
        # Get all samples of this color
        color_samples = df[df["color"] == color]
        
        # How many times to duplicate each sample (on average)
        multiplier = max(1, samples_needed // current_count + 1)
        
        for _ in range(multiplier):
            for idx, row in color_samples.iterrows():
                if len(augmented_rows) >= samples_needed:
                    break
                
                # Create augmented sample
                new_row = row.copy()
                
                # Add noise to temperature
                if "temperature" in new_row and pd.notna(new_row["temperature"]):
                    noise = np.random.normal(0, temperature_noise_std)
                    new_row["temperature"] = new_row["temperature"] + noise
                
                # Slight variation in stock levels (within reason)
                if "stock_red_remaining" in new_row:
                    new_row["stock_red_remaining"] = max(0, min(
                        STOCK_RED_DAYS,
                        int(new_row["stock_red_remaining"]) + random.randint(-1, 1)
                    ))
                if "stock_white_remaining" in new_row:
                    new_row["stock_white_remaining"] = max(0, min(
                        STOCK_WHITE_DAYS,
                        int(new_row["stock_white_remaining"]) + random.randint(-1, 1)
                    ))
                
                augmented_rows.append(new_row)
            
            if len(augmented_rows) >= samples_needed:
                break
    
    if augmented_rows:
        augmented_df = pd.DataFrame(augmented_rows)
        df = pd.concat([df, augmented_df], ignore_index=True)
        
        # Shuffle to mix augmented samples
        df = df.sample(frac=1, random_state=42).reset_index(drop=True)
    
    new_counts = df["color"].value_counts()
    print(f"   After augmentation: {dict(new_counts)}")
    
    return df


def train_xgboost(collector: TempoDataCollector, output_dir: Path) -> dict:
    """Train XGBoost model."""
    print("\n" + "=" * 50)
    print("Training XGBoost Model")
    print("=" * 50)
    
    predictor = TempoPredictor(model_dir=str(output_dir))
    
    # Fetch training data
    print("\nFetching historical data...")
    tempo_history = collector.fetch_tempo_history_all_seasons(start_year=2015)
    
    if tempo_history.empty:
        print("No Tempo history data available")
        return {}
    
    # Get date range
    tempo_history["date"] = pd.to_datetime(tempo_history["date"])
    start_date = tempo_history["date"].min().date()
    end_date = tempo_history["date"].max().date()
    
    # Fetch real temperatures
    temp_df = fetch_historical_temperatures(collector, start_date, end_date)
    
    # Merge
    df = tempo_history.copy()
    if not temp_df.empty:
        temp_df["date"] = pd.to_datetime(temp_df["date"])
        df = df.merge(temp_df, on="date", how="left")
        df["temperature"] = df["temperature_mean"]
    
    # Fill missing temperatures with monthly averages
    month_temp = {1: 4, 2: 5, 3: 8, 4: 11, 5: 15, 6: 18, 
                  7: 20, 8: 20, 9: 17, 10: 12, 11: 7, 12: 4}
    df["month_temp"] = df["date"].dt.month.map(month_temp)
    df["temperature"] = df["temperature"].fillna(df["month_temp"])
    
    # Reconstruct stocks
    df = reconstruct_stock_levels(df)
    
    print(f"Training data: {len(df)} samples")
    
    # Split into train/val
    split_date = date.today() - timedelta(days=365)
    train_df = df[df["date"].dt.date < split_date].copy()
    val_df = df[df["date"].dt.date >= split_date].copy()
    
    # Augment training data
    train_df = augment_minority_classes(train_df)
    
    print(f"   Train: {len(train_df)} samples")
    print(f"   Val: {len(val_df)} samples")
    
    # Train
    metrics = predictor.train(train_df, val_df)
    
    print(f"\nXGBoost model saved to {output_dir}")
    return metrics


def train_mlx(collector: TempoDataCollector, output_dir: Path, epochs: int = 100) -> dict:
    """Train MLX LSTM model with proper data preparation."""
    print("\n" + "=" * 50)
    print("Training MLX LSTM Model (Apple Silicon)")
    print("=" * 50)
    
    try:
        from .mlx_predictor import MLXTempoPredictor, MLX_AVAILABLE
        if not MLX_AVAILABLE:
            print("MLX is not available. Install with: pip install mlx")
            return {}
    except ImportError:
        print("MLX is not installed. Install with: pip install mlx")
        return {}
    
    predictor = MLXTempoPredictor(
        model_dir=str(output_dir),
        epochs=epochs,
        hidden_size=64,
        num_layers=2
    )
    
    # Fetch training data
    print("\nFetching historical data...")
    tempo_history = collector.fetch_tempo_history_all_seasons(start_year=2015)
    
    if tempo_history.empty:
        print("No Tempo history data available")
        return {}
    
    df = tempo_history.copy()
    df["date"] = pd.to_datetime(df["date"])
    df = df.sort_values("date").reset_index(drop=True)
    
    # Get date range
    start_date = df["date"].min().date()
    end_date = df["date"].max().date()
    
    print(f"   Date range: {start_date} to {end_date}")
    
    # ========================================
    # CRITICAL FIX 1: Fetch REAL temperatures
    # ========================================
    temp_df = fetch_historical_temperatures(collector, start_date, end_date)
    
    if not temp_df.empty:
        temp_df["date"] = pd.to_datetime(temp_df["date"])
        df = df.merge(temp_df, on="date", how="left")
        df["temperature"] = df["temperature_mean"]
        print(f"   Merged {len(temp_df)} temperature records")
    else:
        print("   WARNING: No temperature data, using monthly estimates")
    
    # Fill missing temperatures with monthly averages
    month_temp = {1: 4, 2: 5, 3: 8, 4: 11, 5: 15, 6: 18, 
                  7: 20, 8: 20, 9: 17, 10: 12, 11: 7, 12: 4}
    df["month_temp"] = df["date"].dt.month.map(month_temp)
    df["temperature"] = df["temperature"].fillna(df["month_temp"])
    
    # Estimate consumption from temperature
    # French grid: ~1500 MW per degree below 15C
    MEAN_CONSO = 46050
    TEMP_SENSITIVITY = 1500
    df["consumption"] = MEAN_CONSO + (15.0 - df["temperature"]) * TEMP_SENSITIVITY
    df["consumption"] = df["consumption"].clip(35000, 70000)
    
    # ========================================
    # CRITICAL FIX 2: Reconstruct stock levels
    # ========================================
    df = reconstruct_stock_levels(df)
    
    # Remove temporary columns
    df = df.drop(columns=["month_temp", "temperature_mean"], errors="ignore")
    
    # Fill any remaining NaN
    df = df.ffill().bfill()
    
    print(f"Training data: {len(df)} samples")
    
    # Print class distribution
    class_counts = df["color"].value_counts()
    print(f"   Class distribution: {dict(class_counts)}")
    
    # Split into train/val (use last season for validation)
    # This ensures temporal ordering
    split_date = date.today() - timedelta(days=365)
    train_df = df[df["date"].dt.date < split_date].copy()
    val_df = df[df["date"].dt.date >= split_date].copy()
    
    # ========================================
    # CRITICAL FIX 3: Data augmentation for RED
    # ========================================
    # Target: at least 500 samples per minority class
    train_df = augment_minority_classes(train_df, target_samples_per_class=500)
    
    print(f"   Train: {len(train_df)} samples (after augmentation)")
    print(f"   Val: {len(val_df)} samples")
    
    # Train
    history = predictor.train(train_df, val_df)
    
    print(f"\nMLX model saved to {output_dir}")
    return history


def run_backtest(model_type: str, output_dir: Path):
    """Run backtest on the trained model."""
    print("\n" + "=" * 50)
    print(f"Backtesting {model_type.upper()} Model")
    print("=" * 50)
    
    if model_type == "mlx":
        try:
            from .mlx_predictor import MLXTempoPredictor
            from .backtest import run_mlx_backtest
            
            predictor = MLXTempoPredictor(model_dir=str(output_dir))
            if predictor.load():
                run_mlx_backtest(predictor, test_season="2024-2025")
            else:
                print("No MLX model found to backtest")
        except ImportError as e:
            print(f"Error loading MLX: {e}")
    else:
        backtester = TempoBacktester()
        end_date = date.today() - timedelta(days=1)
        start_date = date(end_date.year - 2, 9, 1)
        
        results = backtester.run_backtest(start_date, end_date)
        
        if "error" in results:
            print(f"Backtest error: {results['error']}")
            return
        
        print("\nBacktest Results:")
        print(f"   Total predictions: {results.get('total_predictions', 0)}")
        print(f"   Overall Accuracy: {results.get('accuracy', 0):.2%}")


def main():
    parser = argparse.ArgumentParser(description="Train Tempo prediction models")
    parser.add_argument(
        "--model", 
        choices=["xgboost", "mlx", "all"], 
        default="mlx",
        help="Model type to train (default: mlx)"
    )
    parser.add_argument(
        "--epochs",
        type=int,
        default=100,
        help="Number of epochs for MLX training (default: 100)"
    )
    parser.add_argument(
        "--output-dir",
        type=str,
        default="models",
        help="Output directory for models (default: models)"
    )
    parser.add_argument(
        "--backtest",
        action="store_true",
        help="Run backtest after training"
    )
    
    args = parser.parse_args()
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    
    print("=" * 50)
    print("TEMPO PREDICTION MODEL TRAINING")
    print("=" * 50)
    
    collector = TempoDataCollector()
    
    if args.model in ["xgboost", "all"]:
        train_xgboost(collector, output_dir)
        if args.backtest:
            run_backtest("xgboost", output_dir)
    
    if args.model in ["mlx", "all"]:
        train_mlx(collector, output_dir, epochs=args.epochs)
        if args.backtest:
            run_backtest("mlx", output_dir)
    
    print("\n" + "=" * 50)
    print("Training complete!")
    print("=" * 50)
    print("\nTo start the prediction server:")
    print("  python -m tempo_prediction.server")


if __name__ == "__main__":
    main()
