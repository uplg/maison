# Tempo Prediction

AI-powered prediction of French EDF Tempo electricity pricing colors (Blue, White, Red).

## Overview

This module implements a **Hybrid predictor** that combines the official RTE algorithm with calibrated temperature-to-consumption estimation to forecast Tempo colors up to 7 days ahead.

## Algorithm

The Tempo color selection is based on:
- **Net consumption** = National consumption - (Wind + Solar production)
- **Normalized consumption** = (Net consumption - 46050) / 2160
- **Dynamic thresholds** depending on:
  - Day number in Tempo year (Sept 1 = day 0)
  - Remaining stock of red/white days

### Thresholds
- `Threshold_Red = 3.15 - 0.010 x day - 0.031 x stock_red`
- `Threshold_White+Red = 4.00 - 0.015 x day - 0.026 x (stock_red + stock_white)`

### Constraints
- 22 red days per year (Nov 1 - Mar 31, no weekends, max 5 consecutive)
- 43 white days per year (all year except Sundays)

## Data Sources

- **Tempo history**: RTE public API
- **Consumption/Production**: RTE eco2mix API
- **Weather forecasts**: Open-Meteo API (free, 7-day forecast)

## Installation

```bash
cd tempo-prediction
pip install -e .
```

## Usage

### Calibrate the predictor
```bash
tempo-train --backtest
```

### Run backtest
```bash
tempo-backtest --season 2024-2025
```

### Start the prediction server
```bash
tempo-serve
```

## API

The prediction server exposes a simple HTTP API that the main cat-monitor backend can call.
