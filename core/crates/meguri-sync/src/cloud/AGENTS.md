# CloudKit tracking

- Keep routine CloudKit code here. The sibling `../policy/` holds frozen manual-transfer policy.
- Serialize the private WAL-copy probe/open against every tracking writer and checkpoint, including other stores/builds. The copy shares no SQLite locks with the source; serializing schema setup alone is insufficient.
- `recover` needs caller-held exclusive ownership through move replay, salvage and finalization. It acquires no host lock. Plain `open` refuses outstanding recovery; neither API may be used concurrently with another build's writer/checkpoint.
- Salvage owners live outside ordinary bindings: absent provenance stays optional, and ordinary binding APIs cannot select or mutate them. Conflict envelopes/digests are storage inputs, not semantic record validation.
- `workflow*` is the stable-layout, prebound runtime. Notes stay authoritative in `cloud_runtime_notes`; generic access adapts them. The separate `reconcile_notes` API remains partial and cannot clear whole scan-required.
- Whole completion needs all nine kind counts, raw notetype parts/deck parents, empty physical media plus complete J6 and a matching held-snapshot WAL receipt. Only lifecycle suspend writes durable drain.
- Pending updates are whole ordered lists. SDKs apply them after handler return and report actual additions/removals; Saved must preserve a newer local revision. Unsupported incoming/structural/assets workflows stay blocked.
- A conditional conflict can prove an older retained flight. Keep its validation separate from the exact current Saved result, preserve whole-receipt validation, and share ACK bookkeeping; never rewrite request flights to make an old record pass.
- Probe the runtime v1 marker before opening the V2 source. Validate ordinary runtime payloads only within the store scope. Fixture acknowledgements require `test-hooks`.
- Lock state before `with_tracking_access`. Store owners also gate opens, writes, checkpoints and drops. Call providers outside both guards. Scan reads/encoding run outside them; page merges recheck incarnation, account and full file identity. Retain the snapshot and matching WAL receipt until return.

- V5 keeps image bodies in `cloud_runtime_payloads`. Headers retain only image structure for current, base and ACK state. `FlightPayload::Image` selects a historical body. Read bodies only for a selected flight or an explicit diagnostic request; validation and sending require complete images.
- Runtime V6 fences durable monotonic `sent` metadata; missing legacy `sent` decodes true. Payload images and per-binding `cloud_runtime_payload_owners` remain V5. Markerless V5 is historical; markerless V6 is invalid. Install upgrades only non-salvage owners in this build scope, including later upgrades after another scope advanced V5.
- Runtime V7 adds the separate durable file identity; retain strict `NativeFileIdentity` equality for live guards. V7 also needs the V6 owner tables. Payload migration must retain the V7 fence.
- SDK and non-SDK control intake pin a ScanGuard and recheck the actual file/publication through `with_scan_file` before its tracking transaction. Cached caller validity alone cannot mark or clear root readability.
- Publication V4 fences the root-readability ledger: missing legacy rows are unknown, never clean. Only accepted Active/Epoch intake evaluates its mark; cursor/drain/exchange writes preserve it. Epoch knowledge is explicit unknown/complete/incomplete, separate from historical qualified crt; readable status requires complete knowledge.
- Publication V3 adds qualified epoch-crt knowledge separately from the media fallback column. Never backfill it from legacy crt or media-root acceptance; absent knowledge stays unknown.
- GC checks only blob IDs freed by this write in its binding. Header and own-flight intent refs can keep them live. Check that small list again when an intent ends. Digest-only own-save intents still need real writer and WAL proof.

- Workflow V2 pairs `cloud_rekey_intents` with `cloud_apply_intents`. Persist and settle both together; the server inbox remains ready after the move.

- Final native validation alone reads `NativeAuthority`. Actual State guards publish before unlock; direct shared-State aliases must use `StateGuard`, including Condvar wait/reacquisition. Prepared State never publishes.
- Publish actual authority changes before later tracking/I/O/waits. Compact authority locks never span another lock acquisition, provider, store operation or callback; SDK isolation readback failure retains committed unusability.
