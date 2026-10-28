# Shared fake cloud service

- Treat `e` as opaque. This slice owns service persistence only; no client codec or scheduler.
- Keep bodies in `payloads` only. Versions are immutable; current rows, changes, outcomes and snapshot members hold references. Request logs contain size/status metadata.
- Retain snapshot membership for cursor replay. Lexical pages capture a watermark; later writes belong to the next delta. No expiry or payload garbage collection is modeled yet.
- `inspect` is capped fixture materialization. Use outcome/log pages and single-version resolution for large runs. Debug output must remain type-only.
- Wire limits include MGC1 bytes, independently of SQL overhead and client budgets. Configuration and quota changes are logged transactions.
