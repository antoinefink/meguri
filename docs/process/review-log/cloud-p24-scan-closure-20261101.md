# P24 scan-close prerequisite review — 2026-11-01

Keep the K10.2 / K11.1 / K11.4 bullet in `sync/policy#P24`. The fault regressions pass. Q11 also cites K22.2, and the available run does not establish every case in that citation. The contract itself needs no correction.

- Role: spec-writer; session `s-20261101T113954Z-77a58a2a`.
- Reviewed source base: `dc68c0a5faa185b76893763bd1f0205ad76b5bb9`; the inspected spec, recovery test and child-driver files have no changes from that base.
- Original integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
- Scope: that bullet, Q11 and the cited K22.2 requirement. Other P24 bullets were considered only where they could constrain recovery after Undo of an unsent review. The audit used Meguri specifications, Meguri code and the retained test result; restricted sources and raw guard reports were not opened.

## Evidence inspected

Existing immutable receipt: `build/mg/core-test/20261101T113927Z-21225/result.json`, with `log.txt` beside it. It records clean master `dc68c0a5faa185b76893763bd1f0205ad76b5bb9`, Docker, `core-test -- --features meguri-ffi/test-hooks --test cloud_recovery`, 27 passed, 0 failed, 0 skipped, exit 0. Both regressions below appear as PASS in the log at lines 143–144. No job was rerun for this audit.

| Obligation | Source and assertions | Finding |
|---|---|---|
| Drop a committed op's dirty mark, then kill | `core/crates/meguri-sync/tests/cloud_recovery.rs:988`, `dropped_dirty_mark_offline_clean_close_next_verified_open_scans_and_saves`: the Note changes, observer capture was attempted but not persisted, tracking stays unchanged, WAL coverage reaches the op seq on the same native file, then SIGKILL returns signal 9. Fresh persisted reads retain the edited Note without the mark. | Proved |
| Failed identity reads for the reopened session; clean close stays undrained | The same test at lines 1029–1058 opens `FailAllSession`, checks no engine/batch/scan, no writes in any Service account partition, then closes normally. Fresh reads find a clean sidecar and `scan_drained == false`, unchanged tracking and the edited Note. | Proved |
| Next verified open scans before sending | The same test at lines 1060–1134 checks the same native file, WAL receipt coverage, snapshot dirty source, newer local revision and change seq, canonical digests, pending name, verified-identity-before-scan-before-engine trace order, closed builder before root confirmation, then an actual accepted Service Save of the encrypted Note. A final normal close records drained state. | Proved |
| Edits before another identity resolves stay marked and never send | `cloud_recovery.rs:1138`, `preverification_edits_then_different_identity_never_send`: a Note edit while identity is unread has a persisted observer mark for the original binding. Resolving synthetic account B isolates `AccountChanged`; the mark survives clean close, neither account partition changes, no engine/batch starts, and no transfer/replacement path ran. | Proved |
| Additional cases cited by Q11 | `specs/conformance/cloudkit-acceptance.md:450` includes K22.2. At `specs/sync/cloudkit-records.md:1011`, that requirement covers answering cards and changing notes after a normal close and offline restart. Both failed identity reads and `temporarilyUnavailable` are named. Pending writes must survive another normal close and restart before identity verification, then upload successfully. | Evidence incomplete |

These are production-path tests rather than assertions over a separate model. `meguri-testkit/src/cloud/coordinator.rs:48` opens the SDK rig; its barriers send identity/reconcile/start inputs to the child. `child_driver.rs:1744` opens `meguri_host::CoreHost` with a real identity provider. Its input handlers at lines 2232, 2239, 2300 and 2304 invoke production identity refresh, binding reconcile, capture-loss injection and a real undoable engine Note update. The close input at line 2867 calls `CoreHost::close`; `ChildDriver::kill_sigkill` at line 1627 kills and waits for the actual child. The Service request log and persisted reply references are checked by the test's `assert_saved` helper at line 934.

## Rule review and remaining case

The corrected rule is internally consistent. `sync/cloudkit-records#K10.2` separates stored usability, which admits local dirty capture before identity is known, from verified identity required for remote work. K11.1 keeps sends/applies gated after observer failure. K11.4 requires the open's required scan to commit before a close can record drained state, and V2.4 steps 7–10 put identity and intent recovery before that scan and engine start. The clean-open scan exemption remains the one stated in K11.4; this review adds no requirement to scan an otherwise qualifying clean open.

Production checks agree with that rule: `cloud/runtime.rs:746` verifies the matching identity or isolates an account mismatch; `workflow_runtime.rs:449` rejects reconciliation without a usable verified binding; the scan's successful tracking commit clears its required flag at line 737. `cloud/runtime.rs:1350` records drained state only after whole-scan completion with no scan required/running or tracking stop, valid file/publication identity, and a usable verified binding. A session whose identity never verifies cannot mark its unrun scan drained.

Closure requires a passing K22.2 test using actual engine operations. Start from a normal shutdown. Reopen with each specified unavailable identity result; answer a card and update a Note. Check the pending writes, shut down normally and reopen without verifying the identity. Check that the pending writes remain, then verify the original account and inspect the accepted Service saves. `IdentityReads` at `meguri-testkit/src/cloud/devices.rs:23` currently offers only `Available` and `FailAllSession`, so `TemporarilyUnavailable` is untested here. The existing crash regression changes its Note before termination; the account-switch regression has no card answer. These runs leave the additional K22.2 cases open.

## Narrow review Undo dependency

No other remaining P24 item specifically requires closure before a repair confined to K11.4's local absent-row reconciliation and K17.1's removal of a **proven never-sent** review-log entity. K17.1 at `cloudkit-records.md:689` says an entity created and deleted with `sent == false` leaves tracking and sends nothing; K10.3 and K12.1 retain `sent` separately from acknowledgements and in-flight entries. A missing row with no acknowledgement or with an empty current in-flight list does not alone prove it was never handed. Preserve that distinction and the handed-but-never-acked tombstone case.

The P24 K17.4 absence-pass item concerns records absent from a server fetch and purge safety, not a collection snapshot's absent review row. The P24 deferred `UpdatePending` item concerns adapter event ordering; it becomes a prerequisite if the repair changes that scheduling. Neither is closed by this review. P23's completed in-flight/Undo correction remains in force: an Undo cannot discard durable entries for a save that may already have landed.

This audit accepts no unit, batch, milestone, gate, signed CloudKit run, human approval or trial. The receipt is Rust recovery evidence only; it does not prove Apple SDK behavior, mobile behavior, publication/adoption or the full iCloud workflow. All other P24 items remain unchanged.
