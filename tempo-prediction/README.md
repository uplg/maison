# Tempo Prediction

Prediction of French EDF Tempo electricity pricing colors (Blue, White, Red) using the official RTE algorithm with calibrated temperature-to-consumption estimation.

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

## Prerequisites

Install [pixi](https://pixi.sh) (the package manager):

```bash
curl -fsSL https://pixi.sh/install.sh | bash
```

## Setup

From the `tempo-prediction/` directory:

```bash
pixi install
```

This creates a managed environment in `.pixi/` with Python 3.11+ and all dependencies (numpy, pandas, requests, python-dateutil). The project is installed in editable mode, so changes to the source are reflected immediately.

## Usage

All commands are run via `pixi run` from the `tempo-prediction/` directory.

### Start the prediction server

```bash
pixi run serve
```

Starts the HTTP prediction server on `http://127.0.0.1:3034`. The main home-monitor backend proxies requests to this server.

### Calibrate the predictor

```bash
pixi run calibrate
```

Runs a grid search over historical data (2015-present) to find the optimal thermosensitivity parameter. The result is saved to `cache/calibration_params.json`.

To calibrate and immediately run a backtest:

```bash
pixi run calibrate-backtest
```

### Run backtest

```bash
pixi run backtest
```

Backtests the predictor on all available seasons (2020-present) and prints accuracy, precision, recall, and confusion matrices.

### Run tests

```bash
pixi run -e test test
```

Runs pytest in the `test` environment (which includes pytest and pytest-cov).

### Clean generated files

```bash
pixi run clean
```

Removes cached model files and backtest results.

## Available Tasks

| Task                  | Command                       | Description                          |
|-----------------------|-------------------------------|--------------------------------------|
| `serve`               | `pixi run serve`              | Start prediction server (port 3034)  |
| `calibrate`           | `pixi run calibrate`          | Calibrate from historical data       |
| `calibrate-backtest`  | `pixi run calibrate-backtest` | Calibrate + backtest                 |
| `backtest`            | `pixi run backtest`           | Backtest on all seasons              |
| `clean`               | `pixi run clean`              | Remove generated files               |
| `test`                | `pixi run -e test test`       | Run pytest (test environment)        |

## Configuration

All pixi configuration lives in `pyproject.toml` under `[tool.pixi.*]` sections. The standard `[project]` fields (name, version, dependencies) are shared between pixi and any PEP 621 tool.

Key files:
- `pyproject.toml` — Project metadata, dependencies, and pixi tasks
- `pixi.lock` — Locked dependency versions (committed to git)
- `.pixi/` — Local environment (gitignored, recreated by `pixi install`)

## API

The prediction server exposes a simple HTTP API that the main home-monitor backend can call. See [ARCHITECTURE.md](ARCHITECTURE.md) for the full endpoint reference and response formats.
