# Shared fake cloud service

- Treat `e` as opaque in the service. Client codecs and scheduling belong to the production runtime and adapter, never service persistence.
- Keep bodies in `payloads` only. Versions are immutable; current rows, changes, outcomes and snapshot members hold references. Request logs contain size/status metadata.
- Retain snapshot membership for cursor replay. Lexical pages capture a watermark; later writes belong to the next delta. No expiry or payload garbage collection is modeled yet.
- `inspect` is capped fixture materialization. Use outcome/log pages and single-version resolution for large runs. Debug output must remain type-only.
- Wire limits include MGC1 bytes, independently of SQL overhead and client budgets. Configuration and quota changes are logged transactions.

- `adapter`, `coordinator`, `devices` share `child_driver`; only the parent owns Service. The child uses normal CoreHost and production runtime APIs. Each child-backed test target needs `cloud_child_entry`; that helper is IPC infrastructure, never behavior evidence.
- Baseline preparation may acknowledge actual raw synthetic payloads and a real prior snapshot/receipt only. Durable reads use closed-file recovery readers without opening a host. Never seed edits, pending facts, traces or results.
- SDK scheduling is bounded and uses no sleeps: retain Saved names until handler return, drop nonretry failures before callbacks, then execute complete pending lists in production order. The production core owns budgets, exchanges and action emission.
- Publication opens unbound. Keep prebound baseline fixtures confined to existing stable-binding tests; never use them as adoption or publication implementation.
- Guard synchronous transport enqueue only. Release its reader lease before waiting for a reply or handling nested Close/Restore. A held reply is the original Service response, delivered once without resubmission.
- After a hold or a prior zone-deletion reply, read and check Active before the next enqueue.
- Ordinary SDK batches are non-atomic. Persist all successful record results before returning an unsupported sibling-result error.

- Incoming latches own child workers, not parent stages. A Paused frame is separate from the one deferred command reply; consume each stage event before reporting it, and resume the original callback/page without fetching again. Queries run on the IPC thread with independent reader leases. Release all stages before a clean close. Never move a ReaderLease or guard to a worker.
- `cloud.apply.after_intent` stays before collection COMMIT, after state/tracking guards are dropped. After its callback, revalidate cached authority before allowing COMMIT. Do not observe through cached expected rows or hold a reader across WAL coverage waits.

- Stage fetched envelopes in one parent spool per callback and each asset in its own file. Keep command files until its final reply even if work pauses, is suspended or loses its SDK. Resume with the original command and paths. Only normal child intake creates durable inbox files.
- Hydrate callback sources inside its failure boundary, before core intake. Source errors must complete the original exchange as failed; an early return can leave it active after the parent has released it.

- For production adoption, set the fixture Service page_size to at most 32, matching native fetch resultsLimit. The generic Service default is 200; do not relax core validation or slice a delivered page.
