# K10 status projection: bounded fixer handoff

Prepared from master `ff237184f946a89abe7e2bf009e316efcc93e2a3`.
Integration starting commit: `4e64ed28ac032eec41f29312c2fa441f851fd8ac`.
Role: fixer. Authority: `sync/cloudkit-records#K10`
and its root/crt/generation rules, `sync/policy#P3`, `#P6`, `#P14`, `#P19`,
`sync/cloudkit-recovery#V8`, `#V10`, `#V18`, and the relevant CloudKit acceptance
requirements. No reference, oracle or private material was read.

Disposition: regression and producer map only. Production code, ABI and schemas
are unchanged. Builds, tests, gates, commits and independent review belong to the
integrator; none were run here. This document records source findings, not
verification or acceptance evidence.

`policy_runtime.rs::policy_projection_with_work` leaves `attention_known` false.
That is necessary with today's producers: empty currently projected counts and
a successful P19 exchange cannot prove the missing K10 facts. Merely changing
this boolean would allow false `UpToDate` and leave the crt gate unimplemented.

The added Host tests use the existing generated stable-binding fixture, then
real reconcile, capture, owned SDK fetch events, exchange completion and the
core RecordExchange command. They seed no policy summary or completion receipt.
The fixture's initial baseline acknowledgement remains the existing test hook;
these tests do not establish production publication/adoption or signed behavior.

## Facts and producers

Paths below are relative to `core/crates/meguri-sync/src/cloud/`.
"Projected" describes present code, not certification that the whole condition
has complete coverage. Every condition must be known before an empty attention
map can support readiness.

| Condition / authority | Actual producer and persisted fact | Projection / missing contract |
|---|---|---|
| Binding/account isolation; K10.2, V9, P14.4 `isolated` | `runtime.rs::Bound`; account and known-loss paths persist `bindings` state/reason; publication identity, scope, file and current-process identity guard usability | `policy_runtime` supplies BindingView; `policy/status.rs` adds the count with its isolation/recovery exclusions. Preserve these fences. |
| `chooseCollection`; K10.1, V8/V11/V13 | Selected ordinary binding and sidecar publication decision; salvage owners are separate | Status derives from absence/fence. No-bound projection currently supplies no full attention ledger; do not mark an absent binding clean. |
| `rootUnreadable`; K10.3 bindings, P3, V10.2 | Active/root ACKs exist in runtime scan state and `adapter::Durable.active`; `workflow_runtime::accept_active_proof` validates current-instance proof | **Missing durable mark and ordinary set/clear producer.** `fetch_read_runtime::adapter_complete_fetch_read`'s unreadable Found Active branch returns without storing the required mark (loss handling remains separate). Ordinary root intake decodes Active or returns an error. A confirmed epoch is not proof that every required root/complete-MGEpoch fact is known. |
| `conflicts`; K10.3 conflicts, V17 | Inbox merges / conflict insertion persist conflict copies; resolution retains resolved-at | Projection counts unresolved copies of the bound binding only. Audit required V11/V17 listing/count scope, including preserved copies when no ordinary binding is selected. Never infer zero from absence of a usable binding. |
| `conflictsUnreadable`; V8.1 | `recovery.rs::salvage`/`finalize` preserve incomplete salvage files; `TrackingStore::retained_damaged_sources` exposes `recovery::retained_sources` | Integrated by the 2026-11-13 retained-conflict producer: policy reads the existing finalized-source count under State/tracking order, including without an ordinary binding. Unsafe identities remain errors. See `k10-retained-conflicts-producer-20261113.md` for scope and verification. |
| `refused`; K10.3, K20, P14.4 | `incoming::classify`, `incoming_store` persist refused names/envelopes; ordinary sends consult them | Projected as bound-binding refused row count. Detail/blocked-entity accounting remains distinct; never clear a refusal to obtain readiness. |
| `referenceWaiting`; P14.4, V11 | `incoming_runtime` checks references and `incoming_store` persists inbox reasons; actual owned zone drains are recorded by `adapter_runtime` | Projection only increments incoming-waiting. **Missing qualified attention count after the relevant zone drain.** Must tie drain/entry/binding/instance facts together; an idle engine or a pre-entry drain is insufficient. |
| `oversize`, `unreadable`; K10.3 entities, K7/K6.1 | Raw-row/payload encoders and scans can refuse unsupported/invalid/large data | **No complete durable entity-flag producer/ledger in `workflow::EntityFacts` or NoteTracking.** A returned codec/scan error is not a durable count. A whole scan receipt covers existing scan work, not these missing classifications. |
| `unsyncable`; K10.3, K9.4 | `media::NameState.unsyncable` in media ledger, set by media feed classification | Projected from media names. It does not fill the other missing entity flags. |
| K19.4 | `generation_store.rs` | Runtime does not call `initialize_generation_ledger`. This integration remains open. |
| `generationIncomplete`; K19.7, V18 | `incoming::classify` retains generation controls/members with reason Generation | **No remote waiting/progress ledger/producer.** `generation::Origin` only has Local. Fetched-at is not last member progress; empty generation inbox is not proof a complete ledger exists. |
| `assetUnavailable`; K10.3 inbox, K13.1 | Asset read paths persist Asset reason/retry count; open resets attempts | Projected at retry_count >= 5. Unknown counts cannot be filled from an SDK return alone. |
| `applyFailed`, `rekeyFailed`; K10.3 intents/inbox | Incoming/apply/rekey paths persist reason, typed failure and retries; settle uses real WAL proof | Projection counts apply failures after 5 attempts and rekey failure immediately. Preserve retry/open and intent rules; no new retries. |
| `crtMismatch`; K10.3 bindings, P6 | Publication and adoption supply Epoch crt to `cloud_publication_bindings.crt`; `accept_media_root` can insert the current local value instead | This gap is historical: the qualified `cloud_epoch_crt` producer and `f5bb13d4` Sync Now preparation landed afterward. Summary uses qualified Epoch knowledge; legacy publication/media fallback stays nonauthoritative. See `p6-epoch-crt-producer-fixer-20261112.md` and the later Sync Now checkpoint. Other missing K10 producers remain open. |
| `collectionCorrupt`; P14.4 | Engine lifecycle supplied by Host | Status derives Corrupt attention; projection must retain lifecycle precedence. |
| `mediaZoneMissing`; K10.3 bindings, V10.1 | Media ledger's monotonic zone_missing, media/loss callbacks and persisted media state | `media_runtime::policy_media_facts_locked` supplies optional fact; status derives count. Never recreate/reset this binding's missing media zone. |
| `mediaChanged`; K10.3, P13.3 | Clone check writes NameState.media_changed; the media feed can clear it | Projection counts media names with this value. |
| `mediaMissing`; K10.3 blobs, P3 | NameState plus blob server state from actual reads; `policy_media_facts_locked` computes missing_names | Optional MediaFacts count reaches status. Unknown/absent server evidence is distinct from Deleted; do not infer server absence. |
| `recoveryOpWaiting`; K10.3 recovery_ops, V2.3/V18 | Build-scoped publication/adoption rows and `publication.waiting` | Current projection counts waiting publication or any adoption op as one. No complete prefetch/abandon-generation waiting ledger is projected. Audit running-vs-waiting and multiple-row counts before certifying completeness. |
| `exchangeIncomplete`; P12.6 | Accepted exchange accounting/results and P19 last attempt | Attention enum exists, but no `Attention::ExchangeIncomplete` producer in policy code. Must derive the approved lifetime from the actual accepted outcome; lastCompleted alone cannot erase later incomplete work. |
| `quotaFull`; P15 | Policy operation-error/cycle events, cleared by qualifying successful send | Status adds the policy fact; not a missing K10 zero to synthesize. |
| `localStorageFull`, `trackingError`; P15.2 | Tracking failure/probe events, Runtime tracking_stopped | Status adds typed tracking stop. Read/deserialize/constraint failures must remain errors/stops, never empty facts. |
| `unsupportedSchema`; K5.4, V8.1 | Incoming Format reason and tracking version gates | Format inbox count projected; unsupported tracking schemas fail admission. Preserve refusal without writes; don't convert errors to an empty ledger. |
| `cloudError`; P15 | Policy operation cycles | Status derives count from actual cycle failures, preserving precedence. |
| K10 scan / no pending / in-flight / intents | `workflow_runtime::reconcile_binding` uses all-kind coverage and held snapshot/WAL receipt; durable scan state and intent stores; current SDK flight map | Existing projection uses `whole_complete`, scan flags, current-instance flights, inbox and apply intents. None substitutes for missing attention producers. Re-key/media-intent completeness also needs auditing as producer integration advances. |
| P19 lastCompleted / attempt / quiet | `adapter_complete_exchange` retains accepted outcome/end clock; policy core RecordExchange checks ownership and commits through `policy_store` | Existing projection reads `cloud_policy_evidence(binding)`. Success proves the recorded exchange, not subsequent cleanliness or missing K10 classifications. No-data remains None. |

## Required contract before a production fix

A projection-only patch cannot complete this. The smallest required *logical*
additions are the missing entity classifications, durable root-unreadable mark,
authoritative epoch-crt observation plus current-collection check, and integrated
local/remote generation attempt/progress facts. Each needs actual producers,
ownership, completeness and invalidation rules, then a coherent read under the
existing State/tracking order. Missing legacy facts must remain unknown until
real work establishes them; default values cannot certify old rows.

At least durable storage representation/version decisions are needed for facts
not stored today. This handoff does not choose an extension version, add JSON
fields under an old marker, install an empty generation ledger, change an ABI,
or supply an invented migration. Determine the exact physical minimum and
review its contract before editing schemas or callback interfaces. Existing
native read inputs may suffice for some producers; their presence alone does
not implement the missing semantics.

Keep account/scope/publication/file/incarnation/generation fences, confirmed-root
and loss handling, no signed-reset deletion inference, and P14 precedence. No
network error proves an encrypted reset or server absence. No UI wording,
SDK scope experiment or retry is part of this task.

## Regression export and remaining coverage

Changed Rust file: `core/crates/meguri-host/tests/cloud_policy_driver.rs`.
New selectors (nextest expressions for the integrator):

```text
package(meguri-host) & binary(cloud_policy_driver) & test(/^k10_projection_/)
package(meguri-host) & binary(cloud_policy_driver) & test(completed_runtime_exchange_uses_accepted_end_clock_not_projection_time)
package(meguri-host) & binary(cloud_policy_driver) & test(actual_sdk_drains_qualify_quiet_only_after_both_zones_and_durable_evidence_write)
package(meguri-sync) & binary(cloud_policy)
```

The new enabled reproducer asserts an account-verified usable binding, confirmed
root, completed whole scan, zero current work, empty currently available
attention counts, and a durably recorded successful exchange, while status
remains AwaitingEvidence. It explicitly preserves unknown attention knowledge.
Additional cases cover missing P19 evidence/timestamp, two corrupt fetched
records becoming two real refused rows with NeedsAttention despite prior
success, and account isolation rejecting stale SDK intake while retaining prior
P19 evidence and NeedsAttention.

These are bounded regression/safety coverage, not the requested fully
K10-verified clean-to-UpToDate acceptance case: that case cannot honestly be
created with current producers. No ready Summary was supplied,
no known flag forced and no desired assertion weakened. After producers land,
add the positive case and per-fact negative matrix: missing/legacy, stale account,
epoch/root/crt/generation/file proof, inconsistent ledger, corrupt stored state,
each attention condition, and transitions that clear only on its approved
producer. This patch does not claim that full matrix or the status fix complete.

## Integration evidence

Filtered author: `cleanroom/20261112T185930Z-77768`. Independent review:
`cleanroom/20261112T192930Z-91946`, no P0–P2 findings in the four tests or
bounded handoff. Other map rows were not independently re-audited.

`core-test/20261112T192952Z-92386` runs all four new tests successfully;
32 unrelated cases were filtered. Existing assertions and time limits remain.
This preserves missing-authority safety and does not mark U113, K10 or any gate
complete. The positive fully-known case and producer integration remain open.
The pending batch base is still `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Final correctness lint: `core-lint/20261112T193225Z-92827`, three checks pass.
Privacy: `private/20261112T193417Z-94043`, source and handoff pass.
Static checks: `static-checks/20261112T193418Z-94041`, three checks pass.
Similarity: `similarity/20261112T193230Z-93138`, the source file passes without
exceptions. Source assertions and guards are unchanged.
