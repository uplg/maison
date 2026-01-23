"""
MLX-based LSTM Tempo color predictor optimized for Apple Silicon.

Uses native MLX nn.LSTM and proper MLX APIs for training and inference.
"""

import json
from datetime import date, timedelta
from pathlib import Path
from typing import Optional

import pandas as pd

try:
    import mlx.core as mx
    import mlx.nn as nn
    import mlx.optimizers as optim
    from mlx.utils import tree_flatten
    MLX_AVAILABLE = True
except ImportError:
    MLX_AVAILABLE = False

from .algorithm import TempoAlgorithm
from .constants import is_in_red_period, can_be_red, can_be_white


class TempoLSTMModel(nn.Module):
    """Stacked LSTM with attention for Tempo color prediction."""
    
    def __init__(
        self,
        input_size: int,
        hidden_size: int = 64,
        num_layers: int = 2,
        num_classes: int = 3
    ):
        super().__init__()
        
        self.input_size = input_size
        self.hidden_size = hidden_size
        self.num_layers = num_layers
        
        # Stack multiple LSTM layers manually (MLX LSTM doesn't have num_layers)
        self.lstm_layers = []
        for i in range(num_layers):
            layer_input_size = input_size if i == 0 else hidden_size
            self.lstm_layers.append(nn.LSTM(layer_input_size, hidden_size, bias=True))
        
        # Attention mechanism
        self.attn_linear1 = nn.Linear(hidden_size, hidden_size // 2)
        self.attn_linear2 = nn.Linear(hidden_size // 2, 1)
        
        # Classification head
        self.dropout = nn.Dropout(p=0.2)
        self.fc1 = nn.Linear(hidden_size, hidden_size // 2)
        self.fc2 = nn.Linear(hidden_size // 2, num_classes)
    
    def __call__(self, x: mx.array) -> mx.array:
        # x shape: (batch, seq_len, input_size)
        
        # Pass through stacked LSTM layers
        out = x
        for lstm in self.lstm_layers:
            out, _ = lstm(out)  # (batch, seq_len, hidden_size)
        
        # Attention weights
        attn_hidden = nn.tanh(self.attn_linear1(out))  # (batch, seq_len, hidden/2)
        attn_scores = self.attn_linear2(attn_hidden)  # (batch, seq_len, 1)
        attn_weights = mx.softmax(attn_scores, axis=1)  # (batch, seq_len, 1)
        
        # Weighted sum of LSTM outputs
        context = mx.sum(attn_weights * out, axis=1)  # (batch, hidden_size)
        
        # Classification with dropout
        out = nn.relu(self.fc1(context))
        out = self.dropout(out)
        logits = self.fc2(out)
        
        return logits


class MLXTempoPredictor:
    """MLX-based Tempo color predictor optimized for Apple Silicon."""
    
    COLOR_MAP = {0: "BLUE", 1: "WHITE", 2: "RED"}
    COLOR_TO_IDX = {"BLUE": 0, "WHITE": 1, "RED": 2}
    
    def __init__(
        self,
        sequence_length: int = 14,
        hidden_size: int = 64,
        num_layers: int = 2,
        learning_rate: float = 0.001,
        batch_size: int = 32,
        epochs: int = 100,
        model_dir: str = "models"
    ):
        if not MLX_AVAILABLE:
            raise ImportError("MLX is not installed. Run: pip install mlx")
        
        self.sequence_length = sequence_length
        self.hidden_size = hidden_size
        self.num_layers = num_layers
        self.learning_rate = learning_rate
        self.batch_size = batch_size
        self.epochs = epochs
        self.model_dir = Path(model_dir)
        self.model_dir.mkdir(parents=True, exist_ok=True)
        
        print("🍎 Using MLX on Apple Silicon")
        
        self.model: Optional[TempoLSTMModel] = None
        self.feature_means: Optional[mx.array] = None
        self.feature_stds: Optional[mx.array] = None
        self.algorithm = TempoAlgorithm()
    
    def _prepare_features(self, df: pd.DataFrame) -> mx.array:
        """Prepare features from dataframe following RTE algorithm logic.
        
        The RTE algorithm determines Tempo colors based on:
        1. Normalized net consumption = (consumption - wind - solar) normalized by temperature
        2. Dynamic thresholds = f(day_in_season, remaining_stocks)
        
        Key features:
        - tempo_day: Day number in Tempo season (Sept 1 = 0)
        - temperature: For consumption normalization
        - consumption: Net consumption (main signal)
        - stock ratios: For threshold calculation
        - threshold distances: How far consumption is from color thresholds
        """
        df = df.copy()
        
        # Date features (always available)
        dates = pd.to_datetime(df["date"])
        df["day_of_week"] = dates.dt.dayofweek / 6.0
        df["month"] = dates.dt.month / 12.0
        df["is_weekend"] = (dates.dt.dayofweek >= 5).astype(float)
        
        # Day in Tempo season (Sept 1 = day 0, crucial for threshold calculation)
        def tempo_day(d):
            if d.month >= 9:
                start = pd.Timestamp(year=d.year, month=9, day=1)
            else:
                start = pd.Timestamp(year=d.year - 1, month=9, day=1)
            return (d - start).days
        
        df["tempo_day"] = dates.apply(tempo_day) / 365.0
        
        # Is in red period (Nov 1 - Mar 31)
        df["is_in_red_period"] = df.apply(
            lambda row: 1.0 if is_in_red_period(
                pd.to_datetime(row["date"]).date()
            ) else 0.0,
            axis=1
        )
        
        # Stock features (use defaults if not present)
        if "stock_red_remaining" in df.columns:
            df["stock_red_ratio"] = df["stock_red_remaining"] / 22.0
        else:
            df["stock_red_ratio"] = 0.5  # Default middle value
            
        if "stock_white_remaining" in df.columns:
            df["stock_white_ratio"] = df["stock_white_remaining"] / 43.0
        else:
            df["stock_white_ratio"] = 0.5  # Default middle value
        
        # === TEMPERATURE (critical for RTE algorithm) ===
        if "temperature" in df.columns:
            # Normalize: center around 10°C (French winter avg), scale by 10
            df["temperature_norm"] = (df["temperature"] - 10.0) / 10.0
        else:
            # Estimate from month (rough French averages)
            month_temp = {1: 4, 2: 5, 3: 8, 4: 11, 5: 15, 6: 18, 
                         7: 20, 8: 20, 9: 17, 10: 12, 11: 7, 12: 4}
            estimated_temp = dates.dt.month.map(month_temp)
            df["temperature_norm"] = (estimated_temp - 10.0) / 10.0
        
        # === CONSUMPTION (the main signal in RTE algorithm) ===
        # RTE uses normalized consumption: (conso - mean) / std
        MEAN_CONSO = 46050  # MW (RTE constant)
        STD_CONSO = 2160    # MW (RTE constant)
        
        if "consumption" in df.columns:
            df["consumption_norm"] = (df["consumption"] - MEAN_CONSO) / STD_CONSO
        else:
            # Estimate consumption from temperature
            # French grid: ~1500 MW additional load per degree below 15°C
            temp_sensitivity = 1500
            estimated_temp = df["temperature_norm"] * 10.0 + 10.0  # Denormalize
            estimated_conso = MEAN_CONSO + (15.0 - estimated_temp) * temp_sensitivity
            df["consumption_norm"] = (estimated_conso - MEAN_CONSO) / STD_CONSO
            df["consumption_norm"] = df["consumption_norm"].clip(-3, 3)
        
        # === RTE THRESHOLD FEATURES ===
        # Seuil_Rouge = 3.15 - 0.010 × jour - 0.031 × stock_rouge
        # Seuil_Blanc = 4.00 - 0.015 × jour - 0.026 × (stock_rouge + stock_blanc)
        tempo_day_raw = df["tempo_day"] * 365
        stock_red_raw = df["stock_red_ratio"] * 22
        stock_white_raw = df["stock_white_ratio"] * 43
        
        threshold_red = 3.15 - 0.010 * tempo_day_raw - 0.031 * stock_red_raw
        threshold_white = 4.00 - 0.015 * tempo_day_raw - 0.026 * (stock_red_raw + stock_white_raw)
        
        df["threshold_red_norm"] = threshold_red / 5.0  # Normalize to ~[0,1]
        df["threshold_white_norm"] = threshold_white / 5.0
        
        # Distance to thresholds (key decision signal)
        # Positive = above threshold (likely that color), negative = below
        df["dist_to_red"] = df["consumption_norm"] - threshold_red
        df["dist_to_white"] = df["consumption_norm"] - threshold_white
        
        # =====================================================
        # LAGGED COLOR FEATURES (previous days' colors)
        # =====================================================
        # These help the model learn patterns like:
        # - RED days tend to cluster
        # - After many BLUE days, RED becomes more likely (stock pressure)
        # Encode as one-hot: [prev_blue, prev_white, prev_red]
        if "color" in df.columns:
            # Map colors to numeric
            color_map = {"BLUE": 0, "WHITE": 1, "RED": 2}
            colors = df["color"].map(color_map).fillna(0).values
            
            # Lagged features for previous 1-3 days
            for lag in [1, 2, 3]:
                # Shift colors
                lagged = pd.Series(colors).shift(lag).fillna(0).values
                
                # One-hot encode
                df[f"prev_{lag}_blue"] = (lagged == 0).astype(float)
                df[f"prev_{lag}_white"] = (lagged == 1).astype(float)
                df[f"prev_{lag}_red"] = (lagged == 2).astype(float)
            
            # Count of each color in last 7 days
            color_series = pd.Series(colors)
            df["recent_red_count"] = color_series.rolling(7, min_periods=1).apply(
                lambda x: (x == 2).sum() / len(x)
            ).fillna(0).values
            df["recent_white_count"] = color_series.rolling(7, min_periods=1).apply(
                lambda x: (x == 1).sum() / len(x)
            ).fillna(0).values
        else:
            # No color history available (prediction mode)
            for lag in [1, 2, 3]:
                df[f"prev_{lag}_blue"] = 0.8  # Assume mostly blue
                df[f"prev_{lag}_white"] = 0.15
                df[f"prev_{lag}_red"] = 0.05
            df["recent_red_count"] = 0.0
            df["recent_white_count"] = 0.0
        
        # Fixed feature columns (now 23 features)
        feature_cols = [
            "tempo_day",           # Day in Tempo season (0-1)
            "day_of_week",         # Weekday pattern (0-1)
            "month",               # Month (0-1)
            "is_weekend",          # Weekend flag
            "is_in_red_period",    # Nov-Mar flag
            "temperature_norm",    # Temperature (normalized)
            "consumption_norm",    # Consumption (normalized, main signal)
            "stock_red_ratio",     # Red days remaining (0-1)
            "stock_white_ratio",   # White days remaining (0-1)
            "threshold_red_norm",  # Current red threshold
            "threshold_white_norm", # Current white threshold
            "dist_to_red",         # Distance to red threshold
            "dist_to_white",       # Distance to white threshold
            # Lagged color features
            "prev_1_blue", "prev_1_white", "prev_1_red",
            "prev_2_blue", "prev_2_white", "prev_2_red",
            "prev_3_blue", "prev_3_white", "prev_3_red",
            "recent_red_count",
            "recent_white_count",
        ]
        
        # Convert to MLX array (fill NaN with 0 using pandas before conversion)
        data = df[feature_cols].fillna(0.0).values.astype("float32")
        return mx.array(data)
    
    def _prepare_labels(self, df: pd.DataFrame) -> mx.array:
        """Prepare labels from dataframe."""
        labels = df["color"].map(self.COLOR_TO_IDX).values.astype("int32")
        return mx.array(labels)
    
    def _normalize_features(self, features: mx.array, fit: bool = False) -> mx.array:
        """Normalize features using z-score."""
        if fit:
            self.feature_means = mx.mean(features, axis=0)
            self.feature_stds = mx.std(features, axis=0)
            # Avoid division by zero
            self.feature_stds = mx.where(
                self.feature_stds == 0, 
                mx.ones_like(self.feature_stds), 
                self.feature_stds
            )
        
        return (features - self.feature_means) / self.feature_stds
    
    def _create_sequences(
        self, 
        features: mx.array, 
        labels: mx.array
    ) -> tuple[mx.array, mx.array]:
        """Create sequences for LSTM training."""
        n_samples = features.shape[0] - self.sequence_length
        n_features = features.shape[1]
        
        # Pre-allocate arrays
        X_list = []
        y_list = []
        
        for i in range(n_samples):
            X_list.append(features[i:i + self.sequence_length])
            y_list.append(labels[i + self.sequence_length])
        
        X = mx.stack(X_list, axis=0)
        y = mx.stack(y_list, axis=0)
        
        return X, y
    
    def train(
        self,
        train_df: pd.DataFrame,
        val_df: Optional[pd.DataFrame] = None,
        early_stopping_patience: int = 15
    ) -> dict:
        """Train the MLX LSTM model."""
        print(f"📊 Preparing training data ({len(train_df)} samples)...")
        
        # Prepare features and labels
        X_train = self._prepare_features(train_df)
        y_train = self._prepare_labels(train_df)
        
        # Normalize
        X_train = self._normalize_features(X_train, fit=True)
        
        # Create sequences
        X_seq, y_seq = self._create_sequences(X_train, y_train)
        mx.eval(X_seq, y_seq)
        
        print(f"📈 Created {X_seq.shape[0]} training sequences")
        
        # Validation data
        X_val_seq, y_val_seq = None, None
        if val_df is not None and len(val_df) > self.sequence_length:
            X_val = self._prepare_features(val_df)
            y_val = self._prepare_labels(val_df)
            X_val = self._normalize_features(X_val, fit=False)
            X_val_seq, y_val_seq = self._create_sequences(X_val, y_val)
            mx.eval(X_val_seq, y_val_seq)
            print(f"📈 Created {X_val_seq.shape[0]} validation sequences")
        
        # Initialize model
        input_size = X_train.shape[1]
        self.model = TempoLSTMModel(
            input_size=input_size,
            hidden_size=self.hidden_size,
            num_layers=self.num_layers
        )
        
        # Initialize parameters
        mx.eval(self.model.parameters())
        
        # Count parameters
        num_params = sum(v.size for _, v in tree_flatten(self.model.parameters()))
        print(f"🔢 Model parameters: {num_params:,}")
        
        # Class weights for imbalanced data (BLEU ~80%, BLANC ~12%, ROUGE ~6%)
        # VERY AGGRESSIVE weighting for RED - we MUST catch all RED days
        y_list = y_seq.tolist()
        class_counts = [y_list.count(i) for i in range(3)]
        total = sum(class_counts)
        
        # Aggressive weighting to prioritize RED recall
        # BLUE (0): penalize, WHITE (1): boost, RED (2): heavily boost
        class_weight_values = []
        for i, c in enumerate(class_counts):
            if c > 0:
                base_weight = total / (3 * c)
                if i == 2:  # RED - MUST catch all
                    # Very high weight: 3-5x the base weight
                    class_weight_values.append(base_weight * 4.0)
                elif i == 1:  # WHITE
                    class_weight_values.append(base_weight * 2.0)
                else:  # BLUE - penalize to reduce over-prediction
                    class_weight_values.append(base_weight * 0.5)
            else:
                class_weight_values.append(1.0)
        
        print(f"📊 Class distribution: BLUE={class_counts[0]}, WHITE={class_counts[1]}, RED={class_counts[2]}")
        print(f"⚖️  Class weights: BLUE={class_weight_values[0]:.2f}, WHITE={class_weight_values[1]:.2f}, RED={class_weight_values[2]:.2f}")
        
        # Optimizer
        optimizer = optim.AdamW(learning_rate=self.learning_rate, weight_decay=0.01)
        
        # Training state
        best_val_loss = float("inf")
        patience_counter = 0
        history = {"train_loss": [], "val_loss": [], "train_acc": [], "val_acc": []}
        
        def focal_loss(logits: mx.array, y: mx.array, gamma: float = 2.0, alpha: list = None) -> mx.array:
            """
            Focal Loss for handling class imbalance.
            
            FL(p_t) = -alpha_t * (1 - p_t)^gamma * log(p_t)
            
            - gamma: focusing parameter (higher = more focus on hard examples)
            - alpha: class weights
            
            This loss down-weights easy examples (confident correct predictions)
            and focuses on hard examples (typically minority classes).
            """
            probs = mx.softmax(logits, axis=-1)
            
            # Get probability of true class
            batch_size = y.shape[0]
            indices = mx.arange(batch_size)
            p_t = probs[indices, y]
            
            # Focal weight: (1 - p_t)^gamma
            focal_weight = mx.power(1 - p_t, gamma)
            
            # Cross entropy: -log(p_t)
            ce_loss = -mx.log(p_t + 1e-8)
            
            # Focal loss
            loss = focal_weight * ce_loss
            
            # Apply class weights (alpha)
            if alpha is not None:
                alpha_t = mx.array([alpha[int(yi)] for yi in y.tolist()])
                loss = alpha_t * loss
            
            return loss
        
        def loss_fn(model: TempoLSTMModel, X: mx.array, y: mx.array) -> mx.array:
            logits = model(X)
            # Use Focal Loss instead of standard cross-entropy
            # gamma=2.0 is the standard value from the paper
            # Higher gamma = more focus on hard (misclassified) examples
            loss = focal_loss(logits, y, gamma=2.5, alpha=class_weight_values)
            return mx.mean(loss)
        
        # Use nn.value_and_grad for proper gradient computation
        loss_and_grad_fn = nn.value_and_grad(self.model, loss_fn)
        
        print(f"🚀 Starting training for {self.epochs} epochs...")
        
        for epoch in range(self.epochs):
            self.model.train()
            
            # Shuffle training data
            perm = mx.random.permutation(X_seq.shape[0])
            X_shuffled = X_seq[perm]
            y_shuffled = y_seq[perm]
            
            # Training loop
            train_losses = []
            train_correct = 0
            train_total = 0
            
            for i in range(0, X_seq.shape[0], self.batch_size):
                end_idx = min(i + self.batch_size, X_seq.shape[0])
                X_batch = X_shuffled[i:end_idx]
                y_batch = y_shuffled[i:end_idx]
                
                loss, grads = loss_and_grad_fn(self.model, X_batch, y_batch)
                optimizer.update(self.model, grads)
                mx.eval(self.model.parameters(), optimizer.state)
                
                train_losses.append(float(loss.item()))
                
                # Accuracy
                logits = self.model(X_batch)
                preds = mx.argmax(logits, axis=-1)
                train_correct += int(mx.sum(preds == y_batch).item())
                train_total += y_batch.shape[0]
            
            train_loss = sum(train_losses) / len(train_losses)
            train_acc = train_correct / train_total
            history["train_loss"].append(train_loss)
            history["train_acc"].append(train_acc)
            
            # Validation
            if X_val_seq is not None:
                self.model.eval()
                
                val_logits = self.model(X_val_seq)
                val_loss = float(mx.mean(
                    nn.losses.cross_entropy(val_logits, y_val_seq)
                ).item())
                val_preds = mx.argmax(val_logits, axis=-1)
                val_acc = float(mx.mean(val_preds == y_val_seq).item())
                
                history["val_loss"].append(val_loss)
                history["val_acc"].append(val_acc)
                
                # Early stopping
                if val_loss < best_val_loss:
                    best_val_loss = val_loss
                    patience_counter = 0
                    self.save()
                else:
                    patience_counter += 1
                
                if (epoch + 1) % 10 == 0:
                    print(
                        f"  Epoch {epoch+1}/{self.epochs} - "
                        f"Train: loss={train_loss:.4f}, acc={train_acc:.4f} - "
                        f"Val: loss={val_loss:.4f}, acc={val_acc:.4f}"
                    )
                
                if patience_counter >= early_stopping_patience:
                    print(f"⏹️  Early stopping at epoch {epoch+1}")
                    break
            else:
                if (epoch + 1) % 10 == 0:
                    print(
                        f"  Epoch {epoch+1}/{self.epochs} - "
                        f"Train: loss={train_loss:.4f}, acc={train_acc:.4f}"
                    )
        
        # Load best model or save final
        if X_val_seq is not None:
            self.load()
        else:
            self.save()
        
        print("✅ Training complete!")
        return history
    
    def predict(
        self,
        df: pd.DataFrame,
        stock_red_remaining: int = 22,
        stock_white_remaining: int = 43
    ) -> list[dict]:
        """Predict Tempo colors for the given dates."""
        if self.model is None:
            raise ValueError("Model not trained. Call train() or load() first.")
        
        self.model.eval()
        
        # Add stock info
        df = df.copy()
        df["stock_red_remaining"] = stock_red_remaining
        df["stock_white_remaining"] = stock_white_remaining
        
        # Prepare features
        X = self._prepare_features(df)
        X = self._normalize_features(X, fit=False)
        
        # Pad if needed
        if X.shape[0] < self.sequence_length:
            padding = mx.zeros((self.sequence_length - X.shape[0], X.shape[1]))
            X = mx.concatenate([padding, X], axis=0)
        
        predictions = []
        
        for i in range(len(df)):
            # Get sequence ending at current position
            seq_end = X.shape[0] - len(df) + i + 1
            seq_start = max(0, seq_end - self.sequence_length)
            seq = X[seq_start:seq_end]
            
            # Pad if needed
            if seq.shape[0] < self.sequence_length:
                padding = mx.zeros((self.sequence_length - seq.shape[0], seq.shape[1]))
                seq = mx.concatenate([padding, seq], axis=0)
            
            # Predict
            seq_batch = mx.expand_dims(seq, axis=0)  # Add batch dim
            logits = self.model(seq_batch)
            probs = mx.softmax(logits, axis=-1)[0]
            
            # Convert to Python
            probs_list = probs.tolist()
            
            d = pd.to_datetime(df.iloc[i]["date"]).date()
            
            # Apply constraints from RTE rules
            red_allowed = can_be_red(d, 0) and stock_red_remaining > 0
            white_allowed = can_be_white(d) and stock_white_remaining > 0
            
            # Adjust probabilities
            adjusted_probs = list(probs_list)
            if not red_allowed:
                adjusted_probs[2] = 0
            if not white_allowed:
                adjusted_probs[1] = 0
            
            # =====================================================
            # THRESHOLD-BASED DECISION (not argmax)
            # =====================================================
            # Priority: RED > WHITE > BLUE
            # If RED probability exceeds threshold, predict RED
            # This ensures we catch most RED days (high recall)
            # 
            # After analysis: need to balance precision and recall
            # RED_THRESHOLD too low causes many false positives
            RED_THRESHOLD = 0.25    # Balanced threshold for RED
            WHITE_THRESHOLD = 0.30  # Moderate threshold for WHITE
            
            # Decision logic with priority
            if red_allowed and adjusted_probs[2] >= RED_THRESHOLD:
                # Predict RED if probability exceeds threshold
                final_class = 2
            elif white_allowed and adjusted_probs[1] >= WHITE_THRESHOLD:
                # Predict WHITE if probability exceeds threshold
                final_class = 1
            else:
                # Default to BLUE
                final_class = 0
            
            # Renormalize for reporting (doesn't affect decision)
            total = sum(adjusted_probs)
            if total > 0:
                adjusted_probs = [p / total for p in adjusted_probs]
            else:
                adjusted_probs = [1.0, 0.0, 0.0]
            
            predictions.append({
                "date": d.isoformat(),
                "predicted_color": self.COLOR_MAP[final_class],
                "probabilities": {
                    "BLUE": adjusted_probs[0],
                    "WHITE": adjusted_probs[1],
                    "RED": adjusted_probs[2]
                },
                "confidence": adjusted_probs[final_class],
                "constraints": {
                    "can_be_red": red_allowed,
                    "can_be_white": white_allowed,
                    "is_in_red_period": is_in_red_period(d)
                }
            })
            
            # Update stock
            if final_class == 2:
                stock_red_remaining = max(0, stock_red_remaining - 1)
            elif final_class == 1:
                stock_white_remaining = max(0, stock_white_remaining - 1)
        
        return predictions
    
    def predict_week(
        self,
        history_df: Optional[pd.DataFrame],
        stock_red_remaining: int = 22,
        stock_white_remaining: int = 43,
        start_date: Optional[date] = None,
        weather_forecast: Optional[pd.DataFrame] = None
    ) -> list[dict]:
        """
        Predict Tempo colors for the next 7 days.
        
        Following RTE algorithm, predictions require:
        - Temperature forecast (from Open-Meteo)
        - Consumption estimate (derived from temperature)
        
        Args:
            history_df: Historical data for context
            stock_red_remaining: Remaining red days
            stock_white_remaining: Remaining white days  
            start_date: First prediction date
            weather_forecast: DataFrame with columns [date, temperature_mean]
        """
        if start_date is None:
            start_date = date.today()
        
        future_dates = [start_date + timedelta(days=i) for i in range(7)]
        
        # Build context from history
        if history_df is not None and len(history_df) >= self.sequence_length:
            context_df = history_df.tail(self.sequence_length).copy()
        else:
            context_dates = [start_date - timedelta(days=i) for i in range(self.sequence_length, 0, -1)]
            context_df = pd.DataFrame({"date": context_dates})
            context_df["temperature"] = 10.0
            context_df["consumption"] = 50000
        
        # === Build future dataframe with weather forecasts ===
        future_df = pd.DataFrame({"date": future_dates})
        
        if weather_forecast is not None and len(weather_forecast) > 0:
            # Merge weather forecast
            weather_forecast = weather_forecast.copy()
            weather_forecast["date"] = pd.to_datetime(weather_forecast["date"]).dt.date
            future_df["date_key"] = pd.to_datetime(future_df["date"]).dt.date
            
            # Match forecast to future dates
            temp_map = dict(zip(weather_forecast["date"], weather_forecast["temperature_mean"]))
            future_df["temperature"] = future_df["date_key"].map(temp_map)
            future_df["temperature"] = future_df["temperature"].fillna(8.0)  # Default winter temp
            future_df.drop("date_key", axis=1, inplace=True)
        else:
            # Estimate from month (rough French averages)
            month_temp = {1: 4, 2: 5, 3: 8, 4: 11, 5: 15, 6: 18, 
                         7: 20, 8: 20, 9: 17, 10: 12, 11: 7, 12: 4}
            future_df["date_dt"] = pd.to_datetime(future_df["date"])
            future_df["temperature"] = future_df["date_dt"].dt.month.map(month_temp)
            future_df.drop("date_dt", axis=1, inplace=True)
        
        # === Estimate consumption from temperature ===
        # French grid thermosensitivity: ~1500 MW per degree below 15°C
        MEAN_CONSO = 46050  # MW
        TEMP_SENSITIVITY = 1500  # MW per degree below 15°C
        
        future_df["consumption"] = MEAN_CONSO + (15.0 - future_df["temperature"]) * TEMP_SENSITIVITY
        future_df["consumption"] = future_df["consumption"].clip(35000, 70000)  # Reasonable bounds
        
        # Combine history and future
        combined_df = pd.concat([context_df, future_df], ignore_index=True)
        
        # Predict
        all_predictions = self.predict(combined_df, stock_red_remaining, stock_white_remaining)
        
        return all_predictions[-7:]
    
    def predict_week_deterministic(
        self,
        stock_red_remaining: int = 22,
        stock_white_remaining: int = 43,
        start_date: Optional[date] = None,
        weather_forecast: Optional[pd.DataFrame] = None
    ) -> list[dict]:
        """
        Predict Tempo colors using the deterministic RTE algorithm.
        
        This method applies the actual RTE threshold algorithm instead of
        relying on ML predictions. The only uncertainty is in consumption
        estimation from temperature forecasts.
        
        Args:
            stock_red_remaining: Remaining red days in season
            stock_white_remaining: Remaining white days in season
            start_date: First prediction date
            weather_forecast: DataFrame with [date, temperature_mean]
        """
        from .algorithm import TempoAlgorithm, TempoState, estimate_consumption_from_temperature
        from .constants import get_tempo_day_number
        
        if start_date is None:
            start_date = date.today()
        
        algo = TempoAlgorithm()
        state = TempoState(
            stock_red=stock_red_remaining,
            stock_white=stock_white_remaining
        )
        
        predictions = []
        
        for i in range(7):
            d = start_date + timedelta(days=i)
            
            # Get temperature from forecast or estimate
            temperature = 8.0  # Default winter temp
            if weather_forecast is not None and len(weather_forecast) > 0:
                forecast_copy = weather_forecast.copy()
                forecast_copy["date"] = pd.to_datetime(forecast_copy["date"]).dt.date
                temp_row = forecast_copy[forecast_copy["date"] == d]
                if len(temp_row) > 0:
                    temperature = float(temp_row.iloc[0]["temperature_mean"])
            
            # Estimate consumption from temperature
            estimated_consumption = estimate_consumption_from_temperature(
                temperature=temperature,
                day_of_week=d.weekday(),
                month=d.month
            )
            
            # Normalize consumption (RTE formula)
            normalized_consumption = algo.normalize_consumption(estimated_consumption)
            
            # Apply RTE algorithm
            color, new_state = algo.determine_color(d, normalized_consumption, state)
            
            # Calculate thresholds for context
            tempo_day = get_tempo_day_number(d)
            threshold_red = algo.calculate_threshold_red(tempo_day, state.stock_red)
            threshold_white = algo.calculate_threshold_white_red(
                tempo_day, state.stock_red, state.stock_white
            )
            
            # Calculate probabilities based on distance to thresholds
            dist_to_red = normalized_consumption - threshold_red
            dist_to_white = normalized_consumption - threshold_white
            
            # Convert distances to pseudo-probabilities
            # Using sigmoid-like function
            import math
            def sigmoid(x, scale=2.0):
                return 1 / (1 + math.exp(-x * scale))
            
            prob_red = sigmoid(dist_to_red) if can_be_red(d, state.consecutive_red) and state.stock_red > 0 else 0.0
            prob_white = sigmoid(dist_to_white) if can_be_white(d) and state.stock_white > 0 else 0.0
            prob_blue = 1.0 - max(prob_red, prob_white)
            
            # Normalize
            total = prob_blue + prob_white + prob_red
            prob_blue /= total
            prob_white /= total
            prob_red /= total
            
            predictions.append({
                "date": d.isoformat(),
                "predicted_color": color,
                "probabilities": {
                    "BLUE": prob_blue,
                    "WHITE": prob_white,
                    "RED": prob_red
                },
                "confidence": max(prob_blue, prob_white, prob_red),
                "constraints": {
                    "can_be_red": can_be_red(d, state.consecutive_red) and state.stock_red > 0,
                    "can_be_white": can_be_white(d) and state.stock_white > 0,
                    "is_in_red_period": is_in_red_period(d)
                },
                "algorithm_details": {
                    "temperature": temperature,
                    "estimated_consumption_mw": estimated_consumption,
                    "normalized_consumption": normalized_consumption,
                    "threshold_red": threshold_red,
                    "threshold_white": threshold_white,
                    "stock_red": state.stock_red,
                    "stock_white": state.stock_white
                }
            })
            
            state = new_state
        
        return predictions

    def save(self, name: str = "mlx_tempo"):
        """Save the model using MLX native save_weights."""
        if self.model is None:
            raise ValueError("No model to save")
        
        weights_path = self.model_dir / f"{name}.safetensors"
        params_path = self.model_dir / f"{name}_params.json"
        
        # Save weights using MLX native method
        self.model.save_weights(str(weights_path))
        
        # Save config and normalization params
        params = {
            "sequence_length": self.sequence_length,
            "hidden_size": self.hidden_size,
            "num_layers": self.num_layers,
            "input_size": self.model.input_size,
            "feature_means": self.feature_means.tolist() if self.feature_means is not None else None,
            "feature_stds": self.feature_stds.tolist() if self.feature_stds is not None else None,
        }
        
        with open(params_path, "w") as f:
            json.dump(params, f, indent=2)
        
        print(f"💾 Model saved to {weights_path}")
    
    def load(self, name: str = "mlx_tempo") -> bool:
        """Load the model using MLX native load_weights."""
        weights_path = self.model_dir / f"{name}.safetensors"
        params_path = self.model_dir / f"{name}_params.json"
        
        if not weights_path.exists() or not params_path.exists():
            return False
        
        # Load config
        with open(params_path, "r") as f:
            params = json.load(f)
        
        self.sequence_length = params["sequence_length"]
        self.hidden_size = params["hidden_size"]
        self.num_layers = params["num_layers"]
        
        if params["feature_means"] is not None:
            self.feature_means = mx.array(params["feature_means"])
        if params["feature_stds"] is not None:
            self.feature_stds = mx.array(params["feature_stds"])
        
        # Initialize model
        self.model = TempoLSTMModel(
            input_size=params["input_size"],
            hidden_size=self.hidden_size,
            num_layers=self.num_layers
        )
        
        # Load weights using MLX native method
        self.model.load_weights(str(weights_path))
        
        print(f"📂 Model loaded from {weights_path}")
        return True
