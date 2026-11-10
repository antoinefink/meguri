# Guarded whole-scan observer provenance

Latest root result: `core-test/20261110T114633Z-28443` ran 17 cases; 16 passed.
The reopened unchanged-row proof now passes. The new current-opening history
phase retains real nonempty pending history but fails its unchanged-counter
assertion (8 versus 7). No asynchronous drain is proved missing: the observer
is synchronous. Keep the assertion and diagnose the exact changed entity.
All selected held-scan, newer-write, old reopening and original-role tests pass.

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
