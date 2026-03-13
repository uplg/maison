# Tempo regression fixtures

These snapshots are generated from the legacy backend running on `http://localhost:3033`.

- Do not store bearer tokens in fixtures.
- Prefer sanitizing timestamps if we add committed snapshots later.
- Current regression tests compare normalized JSON responses live instead of checking in full snapshots.
