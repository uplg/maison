These fixtures capture the legacy Tuya HTTP contract without requiring the legacy backend during normal regression runs.

- `cargo test --test tuya_regression` reads these files only.
- `cargo test --test tuya_regression refresh_legacy_tuya_fixtures -- --ignored --nocapture` refreshes them sequentially from the live legacy backend.
- The litter-box fixture now validates the Rust `3.5` path without requiring a live legacy comparison in the normal test run.
