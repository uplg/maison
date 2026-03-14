# Tempo Calibration

The Tempo predictor uses cached RTE history from `cache/tempo/tempo_history_*.json` and derives a calibrated parameter set saved in `cache/tempo/calibration_params.json`.

## What changed

- The predictor now calibrates more than `base_consumption` and `thermosensitivity`.
- It also tunes monthly weighting, weekend effect, red/white threshold offsets, and probability scales.
- Calibration metrics now include accuracy, red recall, white recall, macro F1, and sample count.

## Rebuild calibration

Dry run on specific seasons:

```bash
cargo run --manifest-path backend/Cargo.toml --bin recalibrate_tempo -- 2024-2025 2025-2026 --no-persist
```

Persist calibration to `cache/tempo/calibration_params.json`:

```bash
cargo run --manifest-path backend/Cargo.toml --bin recalibrate_tempo -- 2024-2025 2025-2026
```

Use every cached season automatically:

```bash
cargo run --manifest-path backend/Cargo.toml --bin recalibrate_tempo
```

## HTTP endpoint

You can also trigger a rebuild through the authenticated API:

```bash
curl -X POST "http://localhost:3033/api/tempo/calibration/rebuild?seasons=2024-2025,2025-2026&persist=true" \
  -H "Authorization: Bearer <token>"
```

## Inputs used by calibration

- Tempo history: `cache/tempo/tempo_history_*.json`
  - missing season files are downloaded automatically from the RTE Tempo API before calibration
- Historical temperatures: `cache/tempo/temperature_history.json`
  - fetched automatically from Open-Meteo archive on first run
  - if the cache exists but does not cover enough requested days, the tool now refreshes it automatically
  - falls back to `cache/tempo/temp_forecast.json` if the archive is unavailable
  - calibration now refuses to persist if fewer than 120 dated temperature samples are available
  - if you still get a 503, the archive/fallback data simply does not cover enough days for the requested seasons yet

## Validation

Run the backend tests after recalibration:

```bash
cargo test --manifest-path backend/Cargo.toml
```

Check the resulting metrics in `cache/tempo/calibration_params.json`:

- `calibration_accuracy`
- `calibration_red_recall`
- `calibration_white_recall`
- `calibration_macro_f1`
- `calibration_sample_count`

## Notes

- `--no-persist` is useful to compare candidate seasons before overwriting the live calibration file.
- If you previously generated a tiny `temperature_history.json`, you no longer need to delete it manually before retrying.
- If you want stable results in CI, commit the updated `cache/tempo/calibration_params.json` after validating the metrics.
