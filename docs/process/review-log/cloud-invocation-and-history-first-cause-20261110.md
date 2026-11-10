# Invocation and history first-cause diagnostics

Root evidence supersedes the supplied history diagnosis below.
`core-test/20261110T120154Z-84907` failed before scanning, at the exact
collection-sequence assertion: current sequence 8, held snapshot 7. The barrier
commits after taking that snapshot. No counter change is established. The
next fixture correction will use a fresh post-barrier scan baseline and remove
the unnecessary all-kind diagnostic helpers, preserving every assertion.

`swift-test/20261110T120258Z-86282` compiled. Its manual Core/Service variant
completed; automatic sending failed with `sdkLocked check=stopping`, followed
by `originalSDKValidation` / `StaleIncarnation` at invocation reporting.
The original SDK was already retiring. Keep its stopping check. The first
stop trigger is still unknown.
Independent R1 review is clear. Privacy passed for all five paths.

Recorded fixer, filtered runner. Supplied clean baseline: `86a1ff40`.
Original batch: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Only permitted Meguri source/process instructions were read. No Git, network,
build, test, raw-log, private, reference or Anki-source access.

Root supplied actual native run `14094`: manual completes through original ACK,
actual SDK removal, peer baseline, actual incoming/WAL apply, final data equality
and refusals. Automatic fails at invocation reporting before SDK enqueue with
`connection.authorityChanged`. This patch diagnoses existing failures only.

`CloudPolicyExecution::operation_activity_with_work` prints failure-only
`cloudOperation.refused` with fixed `site=commandValidation`,
`site=originalSDKValidation` or `site=operationInput`. The first two also identify
owner/driver or originalSDK/runtime acquisition with fixed `step` tags where
applicable. Driver's actual Operation SDK check uses `site=inputSDKValidation`;
its existing projection path uses `site=inputReviewPoll` or `site=inputProjection`.
The latter locates the projection call, not a proven inner provider cause.
Runtime errors reuse `refusal_tags_for_testing`; other Driver variants have
closed literal tags, without associated data. Policy errors use literal
CloudPolicyFailure variant tags (fallback `OtherCoreError`). All diagnostics
and diagnostic helpers are `test-hooks` gated and preserve the original results.

`Runtime::sdk_locked` prints `cloudSDK.refused site=sdkLocked` with the first
existing failed `check`: `absent`, `currentIDMatch`, `stopping`, `guardMatch`.
The original guard is computed once at the same reached comparison; its mismatch
mask has bits 0=generation, 1=binding, 2=token. No values or provider reads are
added. A later existing caller failure uses `check=callerValidation` and the
Runtime classifier. Release retains the original short-circuit expression.

Root supplied focused Host result `28443`: 17 run, 16 pass, one failure.
Reopen Phase A now passes; Phase B's single whole scan changes the global counter
after a real ordinary edit/capture and valid current history. Cause remains
unproved. Only
`meguri-host --test cloud_publication_phase`'s
`whole_scan_after_actual_reopen_refreshes_acknowledged_row_provenance_without_tracking_changes`
gains fixture inspections. All existing assertions, including both counters and
full equality except actual provenance, remain unchanged.

After the real edit/capture, the test retains the existing pre-scan status and
reads complete public `entity_facts_page` pagination for all nine kinds, bounded
to fewer than 100 rows per kind. Only on counter mismatch does it read after
facts and emit `cloudHistoryScan.counterMismatch`: closed kind and aggregate
`new`, `missing`, `payloadChanged`, `provenanceOnly`, `markSourceChange` counts.
Provenance-only means full equality after normalizing exactly open/seq/generation;
mark-source change compares optional sources, including presence changes.
Incomplete/oversized inspections emit only `inspectionComplete=false`.
The same failure branch prints pre-scan tracking-stopped/scan-required and
capture-present/persisted/matches-current booleans. The existing API's actual
name is `capture_proof(seq, generation)`; it already filters by that pair, so
absence does not distinguish missing from nonmatching proof. No API was added.
No names, IDs, payloads, digests, counter values, settings, reasons or freeform
errors enter the new diagnostics.

One source-only consistency check verified SDK short circuit/one guard sample,
invocation order, bounded pagination, scan count, retained counter/full-equality
assertions and diagnostic interpolation allowlist. Its final format-count check
expected 16 but found 17: six FFI, five SDK, three Driver and three Host formats.
That checker expectation was incorrect; no rerun or compilation/runtime
verification was performed here.
Root owns compilation, the focused Host test above, both existing native variants,
independent review and provenance/privacy/similarity checks. No behavior, guard,
assertion, status, limit, observer provenance, retry, scan, SDK operation, FFI,
logger, dependency, Swift/UI/AX or production-provider sampling change.
The media-feed recovery candidate remains unproved and unimplemented.
Actual native VM AX remains externally blocked (`-25211`); TCC is untouched.
No Unit, gate, signed-product or acceptance claim.
