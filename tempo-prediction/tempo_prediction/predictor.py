"""
ML-based Tempo predictor using XGBoost.
Optimized for macOS with Apple Silicon support.
"""

import json
import os
from datetime import date, timedelta
from pathlib import Path
from typing import Optional

import joblib
import numpy as np
import pandas as pd
from sklearn.preprocessing import LabelEncoder

from .algorithm import TempoAlgorithm, TempoState, estimate_consumption_from_temperature
from .constants import (
    MODEL_DIR,
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


class TempoPredictor:
    """
    ML-based Tempo color predictor.
    
    Uses XGBoost for classification with features:
    - Temperature forecast
    - Day of week, month
    - Tempo day number
    - Stock levels (estimated)
    - Historical patterns
    """

    def __init__(self, model_dir: Optional[str] = None):
        self.model_dir = Path(model_dir or MODEL_DIR)
        self.model_dir.mkdir(parents=True, exist_ok=True)
        
        self.model = None
        self.label_encoder = LabelEncoder()
        self.label_encoder.fit(["BLUE", "WHITE", "RED"])
        
        self.algorithm = TempoAlgorithm()
        self.collector = TempoDataCollector()
        
        # Feature columns used for training
        self.feature_columns = [
            "tempo_day",
            "day_of_week",
            "month",
            "is_weekend",
            "is_in_red_period",
            "can_be_red",
            "can_be_white",
            "temperature_mean",
            "temperature_7d_avg",
            "stock_red_estimate",
            "stock_white_estimate",
            "threshold_red",
            "threshold_white_red",
            "days_to_end_red_period",
        ]

    def _prepare_features(self, df: pd.DataFrame) -> pd.DataFrame:
        """Prepare features for ML model."""
        features = df.copy()
        
        # Ensure date column is datetime
        if "date" in features.columns:
            features["date"] = pd.to_datetime(features["date"])
        
        # Basic date features
        if "day_of_week" not in features.columns:
            features["day_of_week"] = features["date"].dt.dayofweek
        if "month" not in features.columns:
            features["month"] = features["date"].dt.month
        if "is_weekend" not in features.columns:
            features["is_weekend"] = features["day_of_week"] >= 5
        
        # Tempo-specific features
        if "tempo_day" not in features.columns:
            features["tempo_day"] = features["date"].apply(
                lambda d: get_tempo_day_number(d.date() if hasattr(d, "date") else d)
            )
        
        # Red period check
        features["is_in_red_period"] = features["date"].apply(
            lambda d: is_in_red_period(d.date() if hasattr(d, "date") else d)
        )
        
        # Constraint checks
        features["can_be_red"] = features["date"].apply(
            lambda d: can_be_red(d.date() if hasattr(d, "date") else d)
        )
        features["can_be_white"] = features["date"].apply(
            lambda d: can_be_white(d.date() if hasattr(d, "date") else d)
        )
        
        # Temperature features
        if "temperature_mean" not in features.columns:
            features["temperature_mean"] = 10.0  # Default
        
        features["temperature_7d_avg"] = features["temperature_mean"].rolling(
            window=7, min_periods=1
        ).mean()
        
        # Estimate stock levels based on tempo_day
        # Simplified: assume linear depletion
        features["stock_red_estimate"] = features["tempo_day"].apply(
            lambda d: max(0, STOCK_RED_DAYS - int(d * STOCK_RED_DAYS / 200))
        )
        features["stock_white_estimate"] = features["tempo_day"].apply(
            lambda d: max(0, STOCK_WHITE_DAYS - int(d * STOCK_WHITE_DAYS / 365))
        )
        
        # Calculate thresholds
        features["threshold_red"] = features.apply(
            lambda row: self.algorithm.calculate_threshold_red(
                row["tempo_day"], row["stock_red_estimate"]
            ),
            axis=1,
        )
        features["threshold_white_red"] = features.apply(
            lambda row: self.algorithm.calculate_threshold_white_red(
                row["tempo_day"], row["stock_red_estimate"], row["stock_white_estimate"]
            ),
            axis=1,
        )
        
        # Days until end of red period
        def days_to_end_red(d):
            if hasattr(d, "date"):
                d = d.date()
            if not is_in_red_period(d):
                return 0
            year = d.year
            if d.month >= 11:
                end = date(year + 1, 3, 31)
            else:
                end = date(year, 3, 31)
            return (end - d).days
        
        features["days_to_end_red_period"] = features["date"].apply(days_to_end_red)
        
        # Convert booleans to int
        for col in ["is_weekend", "is_in_red_period", "can_be_red", "can_be_white"]:
            features[col] = features[col].astype(int)
        
        return features

    def train(
        self,
        start_year: int = 2015,
        test_size: float = 0.2,
        save_model: bool = True,
    ) -> dict:
        """
        Train the XGBoost model on historical Tempo data.
        
        Args:
            start_year: First year to include in training
            test_size: Fraction of data to use for testing
            save_model: Whether to save the trained model
        
        Returns:
            Dict with training metrics
        """
        try:
            from xgboost import XGBClassifier
        except ImportError:
            raise ImportError("XGBoost is required. Install with: pip install xgboost")
        
        print("Building training dataset...")
        df = self.collector.build_training_dataset(start_year=start_year)
        
        if df.empty:
            raise ValueError("No training data available")
        
        print(f"Dataset size: {len(df)} samples")
        
        # Prepare features
        features = self._prepare_features(df)
        
        # Fill missing values
        features = features.fillna(features.median(numeric_only=True))
        
        # Split into train/test (time-based split)
        split_idx = int(len(features) * (1 - test_size))
        train_df = features.iloc[:split_idx]
        test_df = features.iloc[split_idx:]
        
        X_train = train_df[self.feature_columns].values
        y_train = self.label_encoder.transform(train_df["color"])
        X_test = test_df[self.feature_columns].values
        y_test = self.label_encoder.transform(test_df["color"])
        
        print(f"Training samples: {len(X_train)}, Test samples: {len(X_test)}")
        
        # Train XGBoost
        # Use 'hist' tree method for Apple Silicon optimization
        self.model = XGBClassifier(
            n_estimators=200,
            max_depth=6,
            learning_rate=0.1,
            tree_method="hist",  # Optimized for CPU/Apple Silicon
            objective="multi:softprob",
            num_class=3,
            eval_metric="mlogloss",
            random_state=42,
            n_jobs=-1,
        )
        
        print("Training XGBoost model...")
        self.model.fit(
            X_train, y_train,
            eval_set=[(X_test, y_test)],
            verbose=True,
        )
        
        # Evaluate
        train_pred = self.model.predict(X_train)
        test_pred = self.model.predict(X_test)
        
        train_accuracy = (train_pred == y_train).mean()
        test_accuracy = (test_pred == y_test).mean()
        
        # Per-class metrics
        from sklearn.metrics import classification_report
        report = classification_report(
            y_test, test_pred,
            target_names=self.label_encoder.classes_,
            output_dict=True,
        )
        
        metrics = {
            "train_accuracy": float(train_accuracy),
            "test_accuracy": float(test_accuracy),
            "train_samples": len(X_train),
            "test_samples": len(X_test),
            "classification_report": report,
            "feature_importance": dict(zip(
                self.feature_columns,
                self.model.feature_importances_.tolist(),
            )),
        }
        
        print(f"\n✅ Training complete!")
        print(f"   Train accuracy: {train_accuracy:.2%}")
        print(f"   Test accuracy: {test_accuracy:.2%}")
        
        if save_model:
            self.save_model()
        
        return metrics

    def save_model(self):
        """Save the trained model to disk."""
        if self.model is None:
            raise ValueError("No model to save")
        
        model_path = self.model_dir / "tempo_xgboost.joblib"
        joblib.dump(self.model, model_path)
        
        # Save feature columns
        config_path = self.model_dir / "model_config.json"
        with open(config_path, "w") as f:
            json.dump({
                "feature_columns": self.feature_columns,
                "classes": self.label_encoder.classes_.tolist(),
            }, f, indent=2)
        
        print(f"Model saved to {model_path}")

    def load_model(self) -> bool:
        """Load a trained model from disk."""
        model_path = self.model_dir / "tempo_xgboost.joblib"
        config_path = self.model_dir / "model_config.json"
        
        if not model_path.exists():
            return False
        
        try:
            self.model = joblib.load(model_path)
            
            if config_path.exists():
                with open(config_path) as f:
                    config = json.load(f)
                    self.feature_columns = config.get("feature_columns", self.feature_columns)
            
            print(f"Model loaded from {model_path}")
            return True
        except Exception as e:
            print(f"Error loading model: {e}")
            return False

    def _predict_with_algorithm(
        self,
        dates: list[date],
        temperatures: Optional[list[float]] = None,
        state: Optional[TempoState] = None,
    ) -> list[dict]:
        """
        Fallback prediction using rule-based RTE algorithm.
        Used when no ML model is available.
        """
        if state is None:
            state = self._estimate_current_state()
        
        results = []
        current_state = state
        
        for i, d in enumerate(dates):
            temp = temperatures[i] if temperatures and i < len(temperatures) else 10.0
            
            # Use algorithm to predict
            consumption = estimate_consumption_from_temperature(temp, d.month)
            prediction = self.algorithm.predict_with_thresholds(d, current_state, consumption)
            
            color = prediction["predicted_color"]
            
            # Build probability estimates based on algorithm confidence
            # Rule-based = deterministic, so 100% confidence in prediction
            probas = {"BLUE": 0.0, "WHITE": 0.0, "RED": 0.0}
            probas[color] = 1.0
            
            results.append({
                "date": d.isoformat(),
                "predicted_color": color,
                "probabilities": probas,
                "confidence": 1.0,  # Algorithm is deterministic
                "constraints": {
                    "can_be_red": can_be_red(d),
                    "can_be_white": can_be_white(d),
                    "is_in_red_period": is_in_red_period(d),
                },
            })
            
            # Update state for next day simulation
            if color == "RED":
                current_state = TempoState(
                    stock_red=current_state.stock_red - 1,
                    stock_white=current_state.stock_white,
                    consecutive_red=current_state.consecutive_red + 1,
                )
            elif color == "WHITE":
                current_state = TempoState(
                    stock_red=current_state.stock_red,
                    stock_white=current_state.stock_white - 1,
                    consecutive_red=0,
                )
            else:
                current_state = TempoState(
                    stock_red=current_state.stock_red,
                    stock_white=current_state.stock_white,
                    consecutive_red=0,
                )
        
        return results

    def predict(
        self,
        dates: list[date],
        temperatures: Optional[list[float]] = None,
        state: Optional[TempoState] = None,
    ) -> list[dict]:
        """
        Predict Tempo colors for given dates.
        
        Args:
            dates: List of dates to predict
            temperatures: Optional temperature forecasts
            state: Optional current algorithm state
        
        Returns:
            List of prediction dicts with date, color, probabilities
        """
        # Try to load model if not loaded
        if not self.model:
            self.load_model()  # Don't raise if fails, we'll use algorithm fallback
        
        if state is None:
            state = self._estimate_current_state()
        
        # If no ML model, use rule-based algorithm
        if not self.model:
            return self._predict_with_algorithm(dates, temperatures, state)
        
        # Build prediction dataframe
        pred_data = []
        for i, d in enumerate(dates):
            temp = temperatures[i] if temperatures and i < len(temperatures) else 10.0
            pred_data.append({
                "date": d,
                "temperature_mean": temp,
            })
        
        df = pd.DataFrame(pred_data)
        features = self._prepare_features(df)
        
        # Get predictions
        X = features[self.feature_columns].fillna(0).values
        probas = self.model.predict_proba(X)
        predictions = self.model.predict(X)
        
        # Build results
        results = []
        for i, d in enumerate(dates):
            color = self.label_encoder.inverse_transform([predictions[i]])[0]
            
            # Apply constraint corrections
            if color == "RED" and not can_be_red(d):
                color = "WHITE" if can_be_white(d) else "BLUE"
            elif color == "WHITE" and not can_be_white(d):
                color = "BLUE"
            
            results.append({
                "date": d.isoformat(),
                "predicted_color": color,
                "probabilities": {
                    "BLUE": float(probas[i][0]),
                    "WHITE": float(probas[i][1]),
                    "RED": float(probas[i][2]),
                },
                "confidence": float(max(probas[i])),
                "constraints": {
                    "can_be_red": can_be_red(d),
                    "can_be_white": can_be_white(d),
                    "is_in_red_period": is_in_red_period(d),
                },
            })
        
        return results

    def predict_week(self) -> list[dict]:
        """Predict Tempo colors for the next 7 days."""
        today = date.today()
        dates = [today + timedelta(days=i) for i in range(7)]
        
        # Get temperature forecast
        forecast = self.collector.fetch_temperature_forecast(days=7)
        if not forecast.empty:
            temperatures = forecast["temperature_mean"].tolist()
        else:
            temperatures = [10.0] * 7  # Default
        
        return self.predict(dates, temperatures)

    def _estimate_current_state(self) -> TempoState:
        """Estimate current algorithm state from historical data."""
        today = date.today()
        start_year, end_year = get_tempo_year(today)
        season = f"{start_year}-{end_year}"
        
        history = self.collector.fetch_tempo_history(season)
        
        # Count used days
        red_used = sum(1 for c in history.values() if c == "RED")
        white_used = sum(1 for c in history.values() if c == "WHITE")
        
        # Count consecutive red days at the end
        consecutive_red = 0
        sorted_dates = sorted(history.keys(), reverse=True)
        for date_str in sorted_dates:
            if history[date_str] == "RED":
                consecutive_red += 1
            else:
                break
        
        return TempoState(
            stock_red=STOCK_RED_DAYS - red_used,
            stock_white=STOCK_WHITE_DAYS - white_used,
            consecutive_red=consecutive_red,
        )


def main():
    """Train and test the predictor."""
    predictor = TempoPredictor()
    
    # Try to load existing model
    if not predictor.load_model():
        print("Training new model...")
        metrics = predictor.train(start_year=2015)
        print(f"\nMetrics: {json.dumps(metrics, indent=2, default=str)}")
    
    # Predict next week
    print("\n📅 Predictions for next 7 days:")
    predictions = predictor.predict_week()
    
    for pred in predictions:
        color = pred["predicted_color"]
        conf = pred["confidence"]
        emoji = {"BLUE": "🔵", "WHITE": "⚪", "RED": "🔴"}[color]
        print(f"  {pred['date']}: {emoji} {color} (confidence: {conf:.1%})")


if __name__ == "__main__":
    main()
