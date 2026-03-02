# Tempo Prediction System - Technical Architecture

## Overview

The Tempo prediction system forecasts EDF Tempo electricity pricing colors (BLUE, WHITE, RED) using a **hybrid approach** that combines the official RTE algorithm with machine learning-based confidence estimation.

The system achieves high overall accuracy by implementing the exact RTE algorithm with calibrated temperature-to-consumption estimation.

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Frontend (React)                          │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐  │
│  │ TempoPrediction │  │  TempoCalendar  │  │   TempoWidget       │  │
│  │     Page        │  │   (4-month)     │  │   (Dashboard)       │  │
│  └────────┬────────┘  └────────┬────────┘  └──────────┬──────────┘  │
└───────────┼─────────────────────┼─────────────────────┼─────────────┘
            │                     │                     │
            └─────────────────────┼─────────────────────┘
                                  │ HTTP (api.ts)
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                      Elysia Backend (TypeScript)                     │
│                         src/routes/tempo.ts                          │
│  Proxies requests to Python prediction server                        │
└─────────────────────────────────────────────────────────────────────┘
                                  │
                                  │ HTTP Proxy
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                Python Prediction Server (port 3034)                  │
│                    tempo_prediction/server.py                        │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                    HybridTempoPredictor                       │   │
│  │  ┌─────────────────────────────────────────────────────────┐ │   │
│  │  │              Calibrated RTE Algorithm                    │ │   │
│  │  │  Temperature → Consumption → Normalized → Color         │ │   │
│  │  └─────────────────────────────────────────────────────────┘ │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐   │
│  │ TempoDataCollec │  │  TempoAlgorithm │  │  CalibrationParams │   │
│  │      tor        │  │  (RTE formulas) │  │  (persisted JSON)  │   │
│  └────────┬────────┘  └─────────────────┘  └─────────────────────┘   │
└───────────┼─────────────────────────────────────────────────────────┘
            │
            ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        External Data Sources                         │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐   │
│  │    Open-Meteo   │  │   RTE Tempo     │  │    eco2mix RTE      │   │
│  │ Weather API     │  │   History API   │  │  (renewables data)  │   │
│  └─────────────────┘  └─────────────────┘  └─────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

## The RTE Algorithm

### How Tempo Colors Are Determined

RTE uses a **deterministic algorithm** based on normalized consumption and dynamic thresholds:

1. **Consumption Normalization**
   ```
   normalized = (net_consumption - 46050) / 2160
   ```
   Where:
   - `net_consumption` = gross consumption - wind - solar (in MW)
   - `46050 MW` = normalization mean
   - `2160 MW` = normalization standard deviation

2. **Dynamic Thresholds**
   
   The thresholds change daily based on:
   - Day number within Tempo year (0 = Sept 1st)
   - Remaining stock of RED/WHITE days
   
   **RED threshold:**
   ```
   threshold_RED = 3.15 - 0.010 × day - 0.031 × stock_red
   ```
   
   **WHITE+RED threshold:**
   ```
   threshold_WHITE = 4.00 - 0.015 × day - 0.026 × (stock_red + stock_white)
   ```

3. **Color Decision**
   - If `normalized > threshold_RED` AND can_be_red → **RED**
   - Else if `normalized > threshold_WHITE` AND can_be_white → **WHITE**
   - Else → **BLUE**

### Constraints

- **RED days**: Only between Nov 1 - Mar 31, not on weekends, max 5 consecutive
- **WHITE days**: Not on Sundays
- **Stock limits**: 22 RED days, 43 WHITE days per season (Sept-Aug)

## The Hybrid Predictor

Since we don't have access to RTE's real-time consumption data, we **estimate consumption from temperature** using a calibrated model.

### Consumption Estimation

```python
consumption = (base + (temp_ref - temp) × thermosensitivity) × weekend_factor × monthly_factor
```

**Calibrated Parameters:**
| Parameter | Value | Description |
|-----------|-------|-------------|
| `base_consumption` | 46,050 MW | Base load (RTE mean) |
| `thermosensitivity` | 1,900 MW/°C | Heating demand sensitivity |
| `temp_reference` | 12°C | Reference temperature |
| `weekend_factor` | 0.92 | Weekend consumption reduction |
| `renewable_factor` | 0.12 | Renewable production offset |

**Monthly Adjustments:**
| Month | Factor | Reason |
|-------|--------|--------|
| January | 1.05 | Peak winter |
| February | 1.03 | Late winter |
| March | 0.98 | Early spring |
| April | 0.90 | Mild weather |
| July | 0.80 | Summer minimum |
| November | 1.00 | Heating season starts |
| December | 1.02 | Winter |

### Calibration Process

The predictor is calibrated using grid search over historical data (2015-present):

1. Fetch all historical Tempo colors and temperatures
2. For each thermosensitivity value (800 - 2500 MW/°C, step 50):
   - Simulate the algorithm on historical data
   - Calculate accuracy and RED recall
3. Select the value that maximizes RED recall (critical for users)
4. Save parameters to `cache/calibration_params.json`

**Current Performance:**
- Overall accuracy: ~85%
- RED recall: **100%** (detects all RED days)
- Calibration date: stored in params

## API Endpoints

### Prediction Server (port 3034)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Server health and model status |
| `/predict/week` | GET | 7-day forecast (best model) |
| `/predict/hybrid` | GET | Hybrid predictor forecast |
| `/calendar` | GET | Season calendar with predictions |
| `/history` | GET | Historical colors for a season |
| `/state` | GET | Current stock levels |
| `/calibration` | GET | Calibration parameters |
| `/calibrate` | POST | Trigger recalibration |
| `/thresholds` | GET | Algorithm thresholds for a date |

### Response Format

**Prediction Response:**
```json
{
  "success": true,
  "predictions": [
    {
      "date": "2025-01-24",
      "predicted_color": "BLUE",
      "probabilities": {
        "BLUE": 0.75,
        "WHITE": 0.20,
        "RED": 0.05
      },
      "confidence": 0.75,
      "constraints": {
        "can_be_red": true,
        "can_be_white": true,
        "is_in_red_period": true
      },
      "details": {
        "temperature": 8.5,
        "estimated_consumption": 52340,
        "normalized_consumption": 2.91,
        "threshold_red": 2.35,
        "threshold_white": 3.15
      }
    }
  ],
  "state": {
    "season": "2024-2025",
    "stock_red_remaining": 18,
    "stock_red_total": 22,
    "stock_white_remaining": 35,
    "stock_white_total": 43
  },
  "model_version": "hybrid-calibrated-1.0.0"
}
```

**Calendar Response:**
```json
{
  "success": true,
  "season": "2024-2025",
  "calendar": [
    {
      "date": "2024-09-01",
      "color": "BLUE",
      "is_actual": true,
      "is_prediction": false
    },
    {
      "date": "2025-01-25",
      "color": "WHITE",
      "is_actual": false,
      "is_prediction": true,
      "probabilities": { "BLUE": 0.2, "WHITE": 0.7, "RED": 0.1 },
      "confidence": 0.7
    }
  ],
  "statistics": {
    "total_days": 147,
    "color_counts": { "BLUE": 129, "WHITE": 14, "RED": 4 },
    "predictions_count": 7
  },
  "stock": {
    "red_remaining": 18,
    "red_total": 22,
    "white_remaining": 29,
    "white_total": 43
  }
}
```

## Data Sources

### Open-Meteo (Weather)

- **Forecast API**: `api.open-meteo.com/v1/forecast`
- **Historical API**: `archive-api.open-meteo.com/v1/archive`
- Location: France centroid (46.6°N, 1.9°E)
- Variables: `temperature_2m_mean`

### RTE Tempo API

- **URL**: `services-rte.com/cms/open_data/v1/tempo`
- Provides historical Tempo colors
- Updated daily after official announcement

### eco2mix RTE (Optional)

- **URL**: `odre.opendatasoft.com/api/explore/v2.1/catalog/datasets/eco2mix-national-tr/records`
- Real-time/historical production data
- Used for wind/solar production (optional enhancement)

## Running the System

All commands use [pixi](https://pixi.sh) from the `tempo-prediction/` directory. See [README.md](README.md) for setup instructions.

### Start Prediction Server

```bash
pixi run serve
```

### Calibrate

```bash
pixi run calibrate
```

### Backtest

```bash
pixi run backtest
```

### Trigger Recalibration via API

```bash
curl -X POST http://localhost:3034/calibrate
```

### Test Predictions

```bash
curl http://localhost:3034/predict/week | jq
```

## Future Improvements

1. **Real-time consumption integration**: If RTE provides API access to real consumption data, accuracy would improve significantly.

2. **Renewable production forecasts**: Integrate wind/solar forecasts from Meteo-France or other sources.

3. **Holiday calendar**: Add French public holidays to reduce consumption estimates.

4. **Industrial activity indicators**: Economic indicators that affect industrial consumption.

5. **Multi-region temperature**: Weighted average of regional temperatures based on population density.

## References

- [RTE Tempo Documentation](https://www.services-rte.com/fr/visualisez-les-donnees-publiees-par-rte/calendrier-tempo.html)
- [Open-Meteo API](https://open-meteo.com/en/docs)
- [eco2mix Open Data](https://odre.opendatasoft.com/explore/dataset/eco2mix-national-tr)
