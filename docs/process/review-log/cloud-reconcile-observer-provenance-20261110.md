# Guarded whole-scan observer provenance

Current status: R4 passed all 17 focused Rust cases.
Root's `core-test/20261110T114633Z-28443` (line 4506) and
`core-test/20261110T120154Z-84907` (line 4595) fail **before scanning** at
`assert_eq!(collection.last_seq().0, seq)`: current 8 versus held snapshot 7.
Those failures show a sequence mismatch, not a counter change. They give no
evidence of another production defect.
Phase A's reopened unchanged-row proof passes in the supplied root results.

R4 retains Phase B's real ordinary edit and `capture_cloud` barrier with its
nonempty flagged-history assertion, then drops the original snapshot, hold and
lease at block end. One fresh ordinary Cloud snapshot samples the actual
post-barrier seq/generation and is released before the one actual Phase B scan.
O1.1/O4 require a new seq even for the barrier's no-row commit, after its held
snapshot. Every original assertion remains, including the pre-scan last_seq
check, exact history/depth/UploadPending, counter/full EntityFacts, cursor and
SDK validity. Phase A and the original selected proof remain unchanged.
The all-kind/capture-proof diagnostics from the mistaken boundary are removed.
No production fix, extra writer, retry or scan is justified or added.

WIP: root R2 source review is clear. Focused Docker run
`core-test/20261110T113706Z-8882` passed ten cases and failed the new Host
fixture before its scan: it expected nonempty undo pending keys after reopen.
R3 corrects that setup and retains real same-opening history coverage; root
verification of the correction remains pending.
The old reopening regression and eight original-role cases passed. The changed
store case and held-scan targets were not selected; they still need execution.
Rust lint `20261110T113935Z-10844`, similarity `20261110T113935Z-10759`
and the five-path privacy scan passed. No unit is accepted.

- Recorded implementer; runner baseline `80f62b3c`, original batch base
  `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Authority: `sync/cloudkit-records#K11.4`,
  `sync/cloudkit-recovery#V2.4`, `conformance/cloudkit-agent#J3.2`;
  root supplied the independently reviewed normal-production diagnosis. No raw
  reports, private data or reference sources were available or read here.
- `workflow_merge` retains its durable-open validation and exact newer-same-open,
  same-generation, greater-seq early return. After successful `observe`, stamp
  the actual scan's supplied open/seq/generation in the existing entity/binding
  transaction, including equal payloads. No observer guard or incoming ApplyRun /
  WAL settled-proof requirement changes.
- Driver capture uses the existing test-hooks-only
  `RuntimeError::refusal_tags_for_testing` on the actual `sync_now_with_work`
  result. Failure line: `cloudCapture.refused site=exchangeCapture`, followed
  only by fixed error-kind/variant tags. The same error propagates; Release has
  no diagnostic branch. No retry, success log or media-ledger repair.

Added focused regressions (not executed in this runner):

- Host `whole_scan_after_actual_reopen_refreshes_acknowledged_row_provenance_without_tracking_changes`:
  actual publication/Saved fixture, clean acknowledged note plus edited
  acknowledged note with retained flight, actual close/reopen, successful normal
  whole scan. Requires new-open and snapshot seq/generation, complete entity
  equality except provenance, unchanged binding counter, pending-history keys,
  undo depth and collection seq, and SDK registration/validation admission.
- Store `guarded_scan_equal_payload_preserves_pending_facts_and_retained_flight`:
  note and config storage, full canonical entity equality except provenance and
  unchanged counter, with a real persisted pending mark/sent flag/flight.
- Store `guarded_changed_scan_marks_snapshot_revision_and_rejects_older_or_invalid_open`:
  note/config changed image, normal snapshot dirty revision/counter/digest and
  actual provenance; older same-open/generation image and both invalid opens
  preserve committed payload, provenance and counter.

## R2 contract correction

R2 runner baseline `9ff06124`, supplied own prepared seed `80303`; original
batch base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Root supplied independent R1's remaining P2: the existing Host test expected
prior-opening provenance after successful normal reconciliation, conflicting
with approved K11.4/V2.4 actual-snapshot provenance. Correct only those three
postscan expectations to the actual current durable open and Query snapshot
sequence/generation. Keep the old pending sequence greater than the reopened
snapshot, old open less than current open, same-generation precondition,
original payload/revision assertions, actual root fetch/accept, Sync Now
selection, boundary comparison against the original pending sequence, batch
and decoded saved-payload proof. Add full `EntityFacts` equality allowing only
the three actual provenance fields to change. No extra edit, fetch, retry or
fabricated observation is added. Seed production repair, new tests and tiny
feature-gated capture error classifier are unchanged.

Exact focused selectors for root (not executed here):

- Existing corrected: package `meguri-host`, target `cloud_publication_phase`,
  `prior_open_observation_does_not_block_reopened_batch_or_sync_now_selection`.
- New Host: package `meguri-host`, target `cloud_publication_phase`,
  `whole_scan_after_actual_reopen_refreshes_acknowledged_row_provenance_without_tracking_changes`.
- New store: package `meguri-sync`, library,
  `cloud::workflow_store::tests::guarded_scan_equal_payload_preserves_pending_facts_and_retained_flight`;
  `cloud::workflow_store::tests::guarded_changed_scan_marks_snapshot_revision_and_rejects_older_or_invalid_open`.
- Existing same-open boundary: package `meguri-host`, target `cloud_publication_phase`,
  `same_open_observation_after_cloud_capture_defers_the_old_batch_and_preserves_the_edit`.

All held-scan/account/suspension/fault refusals remain enabled and unchanged,
including these package `meguri-host`, target `cloud_runtime` selectors:
`paused_snapshot_scan_allows_a_real_note_op_and_preserves_its_newer_capture`,
`account_event_while_scan_paused_prevents_page_and_cursor_commit`,
`suspend_resume_during_pre_snapshot_pause_invalidates_same_generation_scan`,
`creation_fact_mismatch_during_pause_prevents_scan_completion`, and
`mid_capture_write_failure_rolls_back_every_tracking_row_but_not_collection_commit`.

One source-consistency check only; no Git, network, build or test commands.
Root owns compilation, focused Rust execution, both native positive variants,
independent review and provenance/privacy/similarity checks. The supplied native
failure and the concurrent media-feed candidate were not independently observed
here. History checks cover pending-key membership/undo depth/collection sequence,
not a bytewise audit of every stored history row. No unit, gate or signed
acceptance is claimed.

## R3 reopened-history fixture correction

Recorded implementer clean runner; starting baseline is evidenced by root's
commit record. Original batch remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. Supplied focused result `8882`
is retained: old reopening, equal-payload store and original-role eight passed;
the new Host case failed before reconciliation on the invalid nonempty-map
expectation. `engine/undo#M6` and `#M9.7` require close to drop history and its
in-memory pending map.

Only the new Host test changes. Phase A still publishes through actual Saved,
retains a clean acknowledged note and a dirty note with handed flight through
actual close/reopen, and proves full `EntityFacts` equality except actual scan
provenance. Fresh-open depth and pending map must both be empty and remain
exactly equal after its first normal whole scan. Binding, counter, collection
seq and SDK registration/validation assertions remain.

Only after that proof, Phase B makes a new ordinary `UserUndoable` edit to the
pending note. The actual Cloud reader's `capture_cloud` receives that note's
`(Table::Notes, RowKey::Id(NoteId(note.id).0))`, using the production
`build_collection_batch` barrier and `begin_cloud_hold`, with no row writes.
It requires real flagged history, a nonempty exact pending-key map, positive
depth and `UploadPending`; captures actual seq/generation/open/counter and full
entity baselines; then performs one normal reconcile. It compares the entire
map, depth, counter, collection seq and entity facts (only actual provenance
may differ), plus binding, whole-scan cursor and SDK validity. No extra send,
fetch, root confirmation, retry or fabricated flags/history is introduced.

Root's next focused selection must include both Host reopen tests above, the
same-open boundary case, both store cases (`guarded_scan_equal_payload_` **and**
`guarded_changed_scan_`), the unchanged original-role Core eight, and all five
held-scan/account/suspension/fault cases listed above. Include the Host
`--test cloud_runtime` target explicitly; `guarded_scan_` alone misses the
changed-payload store case. No assertions, skips or limits are weakened.

One source-consistency check only; no Git, network, build or test execution in
this runner. Production, diagnostic, store tests, old selection regression and
held-scan refusals are unchanged. Root owns compilation, focused checks,
independent review and provenance/privacy/similarity verification. No unit,
gate or signed acceptance is claimed.


## R4 post-barrier baseline handoff

Recorded fixer, filtered source-only runner; starting baseline is the supplied
current clean master (root retains its exact commit record). Original batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. Root independently confirmed the
exact source/spec diagnosis. Prior failed receipts above remain historical
evidence. Removed the local FixtureFacts pagination/report helpers, before/after
all-kind reads, capture-proof booleans and counterMismatch block; restored the
existing `runtime.status().unwrap().change_counter` baseline call.

Exact focused 17-case nextest selection for root (not executed here):

```text
(package(meguri-ffi) & binary(cloud_original_role)) |
(package(meguri-host) & binary(cloud_publication_phase) &
 (test(=prior_open_observation_does_not_block_reopened_batch_or_sync_now_selection) |
  test(=whole_scan_after_actual_reopen_refreshes_acknowledged_row_provenance_without_tracking_changes))) |
(package(meguri-sync) & binary(meguri_sync) &
 (test(=cloud::workflow_store::tests::guarded_scan_equal_payload_preserves_pending_facts_and_retained_flight) |
  test(=cloud::workflow_store::tests::guarded_changed_scan_marks_snapshot_revision_and_rejects_older_or_invalid_open))) |
(package(meguri-host) & binary(cloud_runtime) &
 (test(=paused_snapshot_scan_allows_a_real_note_op_and_preserves_its_newer_capture) |
  test(=account_event_while_scan_paused_prevents_page_and_cursor_commit) |
  test(=suspend_resume_during_pre_snapshot_pause_invalidates_same_generation_scan) |
  test(=creation_fact_mismatch_during_pause_prevents_scan_completion) |
  test(=mid_capture_write_failure_rolls_back_every_tracking_row_but_not_collection_commit)))
```

This selects eight original-role, two reopen Host, two store and five runtime
cases. Include the `cloud_runtime` target and both store names; `guarded_scan_`
alone omits the changed-payload case. The additional existing same-open boundary
selector listed above is preserved. Root also owns both native positive variants,
compilation, independent review and provenance/privacy/similarity verification.
One source-only consistency check; no build/test execution or passing receipt
in this runner. No Unit, gate, signed-product or acceptance claim.

Consistency-check outcome: the one source-only checker confirmed all original
Host assertions unchanged, then stopped on its own overbroad Phase A comparison:
it included the intentionally removed helper definitions before the fixture.
This is a checker-boundary error, not evidence of a Phase A change. It was not
rerun; its later Release-equivalence and diagnostic-field checks did not execute.
Root must independently review/compile/verify the exported source.

## Root verification

`core-test/20261110T121804Z-53563` passed all 17 selected cases with no skips.
This includes both store cases, actual reopening and current history, the
same-open boundary, held-scan refusals and the original-role tests.
Independent source review is clear of P0–P2 findings. Similarity
`20261110T122011Z-59171` passed. The failed prior receipts remain above.
No Unit or integration gate is accepted.

The separate rollback fault case omitted by the 17-case selector also passes:
`core-test/20261110T124827Z-16062`,
`mid_capture_write_failure_rolls_back_every_tracking_row_but_not_collection_commit`.
This completes 18 distinct focused cases; the omitted case was run once.
