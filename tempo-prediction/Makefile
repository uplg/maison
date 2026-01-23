# Makefile for Tempo Prediction
# ================================

.PHONY: help install train backtest serve clean

# Default Python (adjust if needed)
PYTHON := python3

# Directories
MODEL_DIR := models
CACHE_DIR := cache

help:
	@echo "Tempo Prediction - Available commands:"
	@echo ""
	@echo "  make install     Install dependencies with uv"
	@echo "  make train       Train the MLX LSTM model"
	@echo "  make train-fast  Train with fewer epochs (for testing)"
	@echo "  make backtest    Run backtesting on the trained model"
	@echo "  make serve       Start the prediction HTTP server"
	@echo "  make clean       Remove model files and cache"
	@echo "  make all         Install, train, and backtest"
	@echo ""

install:
	@echo "Installing dependencies..."
	@if command -v uv >/dev/null 2>&1; then \
		uv pip install -e .; \
	else \
		pip install -e .; \
	fi
	@echo "Done!"

train:
	@echo "Training MLX LSTM model (100 epochs)..."
	$(PYTHON) -m tempo_prediction.train --model mlx --epochs 100 --output-dir $(MODEL_DIR)

train-fast:
	@echo "Training MLX LSTM model (30 epochs, fast mode)..."
	$(PYTHON) -m tempo_prediction.train --model mlx --epochs 30 --output-dir $(MODEL_DIR)

train-full:
	@echo "Training MLX LSTM model (200 epochs, full)..."
	$(PYTHON) -m tempo_prediction.train --model mlx --epochs 200 --output-dir $(MODEL_DIR)

backtest:
	@echo "Running backtest on season 2024-2025..."
	$(PYTHON) -m tempo_prediction.backtest --model mlx

backtest-all:
	@echo "Running full backtest..."
	$(PYTHON) -m tempo_prediction.backtest --model mlx --season 2023-2024

serve:
	@echo "Starting prediction server on port 3034..."
	$(PYTHON) -m tempo_prediction.server

# Train and immediately backtest
train-and-test: train backtest

# Full pipeline
all: install train backtest
	@echo ""
	@echo "=========================================="
	@echo "Training and backtesting complete!"
	@echo "=========================================="

clean:
	@echo "Cleaning model files and cache..."
	rm -rf $(MODEL_DIR)/*.safetensors
	rm -rf $(MODEL_DIR)/*.json
	rm -rf $(CACHE_DIR)/*.json
	rm -f backtest_results.json
	@echo "Done!"

# Development helpers
lint:
	ruff check tempo_prediction/

format:
	black tempo_prediction/

test:
	pytest tests/ -v
