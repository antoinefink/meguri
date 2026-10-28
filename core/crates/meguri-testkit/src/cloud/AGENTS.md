# Shared fake cloud service

- Treat `e` as opaque. This slice owns service persistence only; no client codec or scheduler.
- Keep bodies in `payloads` only. Versions are immutable; current rows, changes, outcomes and snapshot members hold references. Request logs contain size/status metadata.
- Retain snapshot membership for cursor replay. Lexical pages capture a watermark; later writes belong to the next delta. No expiry or payload garbage collection is modeled yet.
- `inspect` is capped fixture materialization. Use outcome/log pages and single-version resolution for large runs. Debug output must remain type-only.
- Wire limits include MGC1 bytes, independently of SQL overhead and client budgets. Configuration and quota changes are logged transactions.

- `adapter`, `coordinator`, `devices` share `child_driver`; only the parent owns Service. The child uses normal CoreHost and production runtime APIs. `cloud_child_entry` in three integration targets is IPC infrastructure, never behavior evidence.
- Baseline preparation may acknowledge actual raw synthetic payloads and a real prior snapshot/receipt only. Durable reads use closed-file recovery readers without opening a host. Never seed edits, pending facts, traces or results.
- SDK scheduling is bounded and uses no sleeps: retain Saved names until handler return, drop nonretry failures before callbacks, then execute complete pending lists in production order. The production core owns budgets, exchanges and action emission.
