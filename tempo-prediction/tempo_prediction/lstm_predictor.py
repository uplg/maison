"""
LSTM-based Tempo color predictor optimized for Apple Silicon (MPS).

This model captures temporal dependencies in consumption patterns
and learns the relationship between weather, consumption, and Tempo colors.
"""

import json
import os
from datetime import date, timedelta
from pathlib import Path
from typing import Optional

import numpy as np
import pandas as pd
import torch
import torch.nn as nn
from torch.utils.data import DataLoader, Dataset

from .algorithm import RTEAlgorithm
from .constants import TEMPO_SEASON_START_MONTH, TEMPO_SEASON_START_DAY


class TempoDataset(Dataset):
    """Dataset for Tempo LSTM training."""
    
    def __init__(
        self,
        features: np.ndarray,
        labels: np.ndarray,
        sequence_length: int = 14
    ):
        self.features = torch.FloatTensor(features)
        self.labels = torch.LongTensor(labels)
        self.sequence_length = sequence_length
    
    def __len__(self) -> int:
        return len(self.features) - self.sequence_length
    
    def __getitem__(self, idx: int):
        # Get sequence of features
        x = self.features[idx:idx + self.sequence_length]
        # Target is the day after the sequence
        y = self.labels[idx + self.sequence_length]
        return x, y


class TempoLSTM(nn.Module):
    """LSTM network for Tempo color prediction."""
    
    def __init__(
        self,
        input_size: int,
        hidden_size: int = 64,
        num_layers: int = 2,
        dropout: float = 0.2,
        num_classes: int = 3
    ):
        super().__init__()
        
        self.hidden_size = hidden_size
        self.num_layers = num_layers
        
        # LSTM layers
        self.lstm = nn.LSTM(
            input_size=input_size,
            hidden_size=hidden_size,
            num_layers=num_layers,
            batch_first=True,
            dropout=dropout if num_layers > 1 else 0,
            bidirectional=True
        )
        
        # Attention mechanism
        self.attention = nn.Sequential(
            nn.Linear(hidden_size * 2, hidden_size),
            nn.Tanh(),
            nn.Linear(hidden_size, 1),
            nn.Softmax(dim=1)
        )
        
        # Classification head
        self.classifier = nn.Sequential(
            nn.Linear(hidden_size * 2, hidden_size),
            nn.ReLU(),
            nn.Dropout(dropout),
            nn.Linear(hidden_size, num_classes)
        )
    
    def forward(self, x: torch.Tensor) -> torch.Tensor:
        # LSTM forward pass
        lstm_out, _ = self.lstm(x)  # (batch, seq_len, hidden*2)
        
        # Attention weights
        attn_weights = self.attention(lstm_out)  # (batch, seq_len, 1)
        
        # Weighted sum of LSTM outputs
        context = torch.sum(attn_weights * lstm_out, dim=1)  # (batch, hidden*2)
        
        # Classification
        logits = self.classifier(context)  # (batch, num_classes)
        
        return logits


class LSTMTempoPredictor:
    """LSTM-based Tempo color predictor."""
    
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
        self.sequence_length = sequence_length
        self.hidden_size = hidden_size
        self.num_layers = num_layers
        self.learning_rate = learning_rate
        self.batch_size = batch_size
        self.epochs = epochs
        self.model_dir = Path(model_dir)
        self.model_dir.mkdir(parents=True, exist_ok=True)
        
        # Detect device (MPS for Apple Silicon, CUDA, or CPU)
        if torch.backends.mps.is_available():
            self.device = torch.device("mps")
            print("🍎 Using Apple Silicon GPU (MPS)")
        elif torch.cuda.is_available():
            self.device = torch.device("cuda")
            print("🎮 Using NVIDIA GPU (CUDA)")
        else:
            self.device = torch.device("cpu")
            print("💻 Using CPU")
        
        self.model: Optional[TempoLSTM] = None
        self.feature_means: Optional[np.ndarray] = None
        self.feature_stds: Optional[np.ndarray] = None
        self.algorithm = RTEAlgorithm()
    
    def _prepare_features(self, df: pd.DataFrame) -> np.ndarray:
        """Prepare features from dataframe."""
        feature_cols = []
        
        # Temperature features
        if "temperature" in df.columns:
            feature_cols.append("temperature")
        if "temp_min" in df.columns:
            feature_cols.append("temp_min")
        if "temp_max" in df.columns:
            feature_cols.append("temp_max")
        
        # Consumption features
        if "consumption" in df.columns:
            feature_cols.append("consumption")
        if "consumption_normalized" in df.columns:
            feature_cols.append("consumption_normalized")
        
        # Date features
        df = df.copy()
        df["day_of_week"] = pd.to_datetime(df["date"]).dt.dayofweek / 6.0
        df["day_of_year"] = pd.to_datetime(df["date"]).dt.dayofyear / 365.0
        df["month"] = pd.to_datetime(df["date"]).dt.month / 12.0
        feature_cols.extend(["day_of_week", "day_of_year", "month"])
        
        # Stock features (if available)
        if "stock_red_remaining" in df.columns:
            df["stock_red_ratio"] = df["stock_red_remaining"] / 22.0
            feature_cols.append("stock_red_ratio")
        if "stock_white_remaining" in df.columns:
            df["stock_white_ratio"] = df["stock_white_remaining"] / 43.0
            feature_cols.append("stock_white_ratio")
        
        # Is in red period
        if "is_in_red_period" in df.columns:
            feature_cols.append("is_in_red_period")
        else:
            df["is_in_red_period"] = df.apply(
                lambda row: 1.0 if self.algorithm.is_in_red_period(
                    pd.to_datetime(row["date"]).date()
                ) else 0.0,
                axis=1
            )
            feature_cols.append("is_in_red_period")
        
        return df[feature_cols].values.astype(np.float32)
    
    def _prepare_labels(self, df: pd.DataFrame) -> np.ndarray:
        """Prepare labels from dataframe."""
        return df["color"].map(self.COLOR_TO_IDX).values
    
    def _normalize_features(
        self, 
        features: np.ndarray, 
        fit: bool = False
    ) -> np.ndarray:
        """Normalize features using z-score."""
        if fit:
            self.feature_means = np.nanmean(features, axis=0)
            self.feature_stds = np.nanstd(features, axis=0)
            self.feature_stds[self.feature_stds == 0] = 1  # Avoid division by zero
        
        return (features - self.feature_means) / self.feature_stds
    
    def train(
        self,
        train_df: pd.DataFrame,
        val_df: Optional[pd.DataFrame] = None,
        early_stopping_patience: int = 10
    ) -> dict:
        """Train the LSTM model."""
        print(f"📊 Preparing training data ({len(train_df)} samples)...")
        
        # Prepare features and labels
        X_train = self._prepare_features(train_df)
        y_train = self._prepare_labels(train_df)
        
        # Normalize features
        X_train = self._normalize_features(X_train, fit=True)
        
        # Handle NaN values
        X_train = np.nan_to_num(X_train, nan=0.0)
        
        # Create dataset and dataloader
        train_dataset = TempoDataset(X_train, y_train, self.sequence_length)
        train_loader = DataLoader(
            train_dataset, 
            batch_size=self.batch_size, 
            shuffle=True
        )
        
        # Validation data
        val_loader = None
        if val_df is not None:
            X_val = self._prepare_features(val_df)
            y_val = self._prepare_labels(val_df)
            X_val = self._normalize_features(X_val, fit=False)
            X_val = np.nan_to_num(X_val, nan=0.0)
            val_dataset = TempoDataset(X_val, y_val, self.sequence_length)
            val_loader = DataLoader(val_dataset, batch_size=self.batch_size)
        
        # Initialize model
        input_size = X_train.shape[1]
        self.model = TempoLSTM(
            input_size=input_size,
            hidden_size=self.hidden_size,
            num_layers=self.num_layers
        ).to(self.device)
        
        # Class weights for imbalanced data
        class_counts = np.bincount(y_train, minlength=3)
        class_weights = 1.0 / (class_counts + 1)
        class_weights = class_weights / class_weights.sum()
        class_weights = torch.FloatTensor(class_weights).to(self.device)
        
        # Loss and optimizer
        criterion = nn.CrossEntropyLoss(weight=class_weights)
        optimizer = torch.optim.AdamW(
            self.model.parameters(), 
            lr=self.learning_rate,
            weight_decay=0.01
        )
        scheduler = torch.optim.lr_scheduler.ReduceLROnPlateau(
            optimizer, mode="min", factor=0.5, patience=5
        )
        
        # Training loop
        best_val_loss = float("inf")
        patience_counter = 0
        history = {"train_loss": [], "val_loss": [], "train_acc": [], "val_acc": []}
        
        print(f"🚀 Starting training for {self.epochs} epochs...")
        
        for epoch in range(self.epochs):
            # Training
            self.model.train()
            train_loss = 0.0
            train_correct = 0
            train_total = 0
            
            for X_batch, y_batch in train_loader:
                X_batch = X_batch.to(self.device)
                y_batch = y_batch.to(self.device)
                
                optimizer.zero_grad()
                outputs = self.model(X_batch)
                loss = criterion(outputs, y_batch)
                loss.backward()
                torch.nn.utils.clip_grad_norm_(self.model.parameters(), 1.0)
                optimizer.step()
                
                train_loss += loss.item()
                _, predicted = torch.max(outputs, 1)
                train_total += y_batch.size(0)
                train_correct += (predicted == y_batch).sum().item()
            
            train_loss /= len(train_loader)
            train_acc = train_correct / train_total
            history["train_loss"].append(train_loss)
            history["train_acc"].append(train_acc)
            
            # Validation
            if val_loader:
                self.model.eval()
                val_loss = 0.0
                val_correct = 0
                val_total = 0
                
                with torch.no_grad():
                    for X_batch, y_batch in val_loader:
                        X_batch = X_batch.to(self.device)
                        y_batch = y_batch.to(self.device)
                        
                        outputs = self.model(X_batch)
                        loss = criterion(outputs, y_batch)
                        
                        val_loss += loss.item()
                        _, predicted = torch.max(outputs, 1)
                        val_total += y_batch.size(0)
                        val_correct += (predicted == y_batch).sum().item()
                
                val_loss /= len(val_loader)
                val_acc = val_correct / val_total
                history["val_loss"].append(val_loss)
                history["val_acc"].append(val_acc)
                
                scheduler.step(val_loss)
                
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
                        f"Train Loss: {train_loss:.4f}, Acc: {train_acc:.4f} - "
                        f"Val Loss: {val_loss:.4f}, Acc: {val_acc:.4f}"
                    )
                
                if patience_counter >= early_stopping_patience:
                    print(f"⏹️  Early stopping at epoch {epoch+1}")
                    break
            else:
                if (epoch + 1) % 10 == 0:
                    print(
                        f"  Epoch {epoch+1}/{self.epochs} - "
                        f"Train Loss: {train_loss:.4f}, Acc: {train_acc:.4f}"
                    )
        
        # Load best model if validation was used
        if val_loader:
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
        
        # Add stock info
        df = df.copy()
        df["stock_red_remaining"] = stock_red_remaining
        df["stock_white_remaining"] = stock_white_remaining
        
        # Prepare features
        X = self._prepare_features(df)
        X = self._normalize_features(X, fit=False)
        X = np.nan_to_num(X, nan=0.0)
        
        # We need at least sequence_length samples
        if len(X) < self.sequence_length:
            # Pad with zeros
            padding = np.zeros((self.sequence_length - len(X), X.shape[1]))
            X = np.vstack([padding, X])
        
        # Create sequence for prediction
        X_tensor = torch.FloatTensor(X).unsqueeze(0).to(self.device)
        
        # Predict
        self.model.eval()
        predictions = []
        
        with torch.no_grad():
            for i in range(len(df)):
                # Get sequence ending at current position
                seq_start = max(0, i + len(X) - len(df) - self.sequence_length + 1)
                seq_end = seq_start + self.sequence_length
                seq = X_tensor[:, seq_start:seq_end, :]
                
                if seq.size(1) < self.sequence_length:
                    # Pad if needed
                    padding = torch.zeros(1, self.sequence_length - seq.size(1), seq.size(2)).to(self.device)
                    seq = torch.cat([padding, seq], dim=1)
                
                outputs = self.model(seq)
                probs = torch.softmax(outputs, dim=1)[0].cpu().numpy()
                predicted_class = int(torch.argmax(outputs, dim=1).item())
                
                d = pd.to_datetime(df.iloc[i]["date"]).date()
                
                # Apply constraints
                can_be_red, can_be_white = self.algorithm.can_select_color(
                    d, stock_red_remaining, stock_white_remaining, 0
                )
                
                # Adjust probabilities based on constraints
                adjusted_probs = probs.copy()
                if not can_be_red:
                    adjusted_probs[2] = 0
                if not can_be_white:
                    adjusted_probs[1] = 0
                
                # Renormalize
                if adjusted_probs.sum() > 0:
                    adjusted_probs /= adjusted_probs.sum()
                else:
                    adjusted_probs = np.array([1.0, 0.0, 0.0])  # Default to BLUE
                
                final_class = int(np.argmax(adjusted_probs))
                
                predictions.append({
                    "date": d.isoformat(),
                    "predicted_color": self.COLOR_MAP[final_class],
                    "probabilities": {
                        "BLUE": float(adjusted_probs[0]),
                        "WHITE": float(adjusted_probs[1]),
                        "RED": float(adjusted_probs[2])
                    },
                    "confidence": float(adjusted_probs[final_class]),
                    "constraints": {
                        "can_be_red": can_be_red,
                        "can_be_white": can_be_white,
                        "is_in_red_period": self.algorithm.is_in_red_period(d)
                    }
                })
                
                # Update stock for next prediction
                if final_class == 2:  # RED
                    stock_red_remaining = max(0, stock_red_remaining - 1)
                elif final_class == 1:  # WHITE
                    stock_white_remaining = max(0, stock_white_remaining - 1)
        
        return predictions
    
    def predict_week(
        self,
        history_df: pd.DataFrame,
        stock_red_remaining: int = 22,
        stock_white_remaining: int = 43,
        start_date: Optional[date] = None
    ) -> list[dict]:
        """Predict Tempo colors for the next 7 days."""
        if start_date is None:
            start_date = date.today()
        
        # Create dataframe for the next 7 days
        future_dates = [start_date + timedelta(days=i) for i in range(7)]
        
        # We need historical context for LSTM
        # Use the last sequence_length days from history
        if history_df is not None and len(history_df) >= self.sequence_length:
            context_df = history_df.tail(self.sequence_length).copy()
        else:
            # Create empty context with basic features
            context_dates = [start_date - timedelta(days=i) for i in range(self.sequence_length, 0, -1)]
            context_df = pd.DataFrame({"date": context_dates})
            context_df["temperature"] = 10.0  # Default
            context_df["consumption"] = 50000  # Default
        
        # Create future dataframe
        future_df = pd.DataFrame({"date": future_dates})
        
        # Add placeholder features for future dates
        # In production, these would come from weather forecasts
        future_df["temperature"] = 8.0  # Winter average
        future_df["consumption"] = 55000  # Default
        
        # Combine context and future
        combined_df = pd.concat([context_df, future_df], ignore_index=True)
        
        # Get predictions for just the future dates
        all_predictions = self.predict(combined_df, stock_red_remaining, stock_white_remaining)
        
        # Return only future predictions
        return all_predictions[-7:]
    
    def save(self, name: str = "lstm_tempo"):
        """Save the model and normalization parameters."""
        if self.model is None:
            raise ValueError("No model to save")
        
        model_path = self.model_dir / f"{name}.pt"
        params_path = self.model_dir / f"{name}_params.json"
        
        # Save model weights
        torch.save(self.model.state_dict(), model_path)
        
        # Save normalization parameters and config
        params = {
            "feature_means": self.feature_means.tolist() if self.feature_means is not None else None,
            "feature_stds": self.feature_stds.tolist() if self.feature_stds is not None else None,
            "sequence_length": self.sequence_length,
            "hidden_size": self.hidden_size,
            "num_layers": self.num_layers,
            "input_size": self.model.lstm.input_size
        }
        
        with open(params_path, "w") as f:
            json.dump(params, f)
        
        print(f"💾 Model saved to {model_path}")
    
    def load(self, name: str = "lstm_tempo") -> bool:
        """Load the model and normalization parameters."""
        model_path = self.model_dir / f"{name}.pt"
        params_path = self.model_dir / f"{name}_params.json"
        
        if not model_path.exists() or not params_path.exists():
            return False
        
        # Load parameters
        with open(params_path, "r") as f:
            params = json.load(f)
        
        self.feature_means = np.array(params["feature_means"]) if params["feature_means"] else None
        self.feature_stds = np.array(params["feature_stds"]) if params["feature_stds"] else None
        self.sequence_length = params["sequence_length"]
        self.hidden_size = params["hidden_size"]
        self.num_layers = params["num_layers"]
        
        # Initialize and load model
        self.model = TempoLSTM(
            input_size=params["input_size"],
            hidden_size=self.hidden_size,
            num_layers=self.num_layers
        ).to(self.device)
        
        self.model.load_state_dict(torch.load(model_path, map_location=self.device))
        self.model.eval()
        
        print(f"📂 Model loaded from {model_path}")
        return True
