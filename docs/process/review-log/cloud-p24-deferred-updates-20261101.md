# P24 deferred updates and root confirmation audit

Role: spec-writer, session `s-20261101T113954Z-77a58a2a`.
Read-only master audit at `d1d2611d496f0903cb3f735ea08230ba4946004e`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
No source changes, jobs, commits, forbidden-source access or normative edits.
The proposed patch removes only the deferred UpdatePending bullet and updates its matching Q11 row to P24 confirmed, with reviewed fake evidence and signed A23 still outstanding. The root item stays.

## Deferred UpdatePending: closure supported

Authority: `sync/policy#P24` K12.4 / K12.3 / A23 / P4 / P12.3 step 4; matching `conformance/cloudkit-acceptance#Q11` row; K22.2 revision-3/revision-4 case.

Clean receipt `build/mg/core-test/20261101T134745Z-91787/result.json` records `4fa09dad1b6eca37355ead52bf75a260b5d3aec9`, 420 selected tests passed, no failures/skips. It covers CloudKit tests in meguri-sync and meguri-host, not the whole workspace. The inspected Rust test, runtime, adapter and coordinator files are unchanged from that receipt. The later Resume work is committed separately as 6060d95c; this receipt does not prove those changes.

`core/crates/meguri-sync/tests/cloud_adapter_regressions.rs`:

- Test at line 282: revision 4 is saved after revision 3 and callback completion, without another trigger. The final ACK survives reopen.
- Test at line 368: a saved review and an edit wait for SessionEnded; the full update precedes sending.
- `deferred_add_then_full_remove_keeps_name_absent_and_comparison_matches` (427): complete add/removal update is deferred inside the production handler; both updates apply in emission order; the name stays absent and the later skipped full comparison agrees with core sendables.
- `deferred_blob_save_add_then_delete_add_keeps_delete` (468): deduplicated SDK keeps Delete when the deferred Save and subsequent Delete apply in emitted order.
- `sync_now_auto_off_400_after_first_250_limit_exceeded_finishes_within_eight_calls` (491): real 250-record Service failure, SDK drops failed names, original send results re-add them, 2..8 returned send calls, every selected name accepted once, exact payloads/conditional records/ACKs and no outstanding names after reopen.
- `core_pending_actions_preserve_emission_order` (635): production action IDs and complete updates, removals included, match emission order. The queue tests use opaque pending snapshots to isolate adapter routing; they do not claim structural-generation or blob-cleanup behavior.

The Rust fake retains a saved name until its handler returns and deduplicates additions. `SDKRig::return_sent_handler` performs SDK removal before consuming the emitted add-backs; this makes the Saved race observable instead of inserting a success trace. Core `Actions` and runtime own the update list.

Native corroboration exists in `CoreBridgeTests/CloudKitSyncTests.swift`: `cloudKitSavedRevision3CannotEraseRevision4` checks both A23 values and the late-removal fallback; the held-edit, complete-update ordering, blob Save/Delete ordering and auto-off 400-record tests cover the same cases through the real Swift adapter with a registered fake SDK. All appear passing in `swift-test/20261101T155312Z-78666/log.txt`, although that suite is failed overall (58/62, four later shutdown fixtures). The earlier `swift-test/20261101T001042Z-42933` passed 75 selected checks with dirty inputs. Neither is represented here as fresh clean native proof. The repaired shutdown slice separately passes 24 app checks and seven adapter checks, per `cloud-owned-shutdown-20261101.md`; those checks are not a rerun of every native test.

Rule review: the newest correction is consistent. Deferring the entire update preserves removal order; sequential emitted updates prevent a Save re-add from overtaking a later removal/Delete; waiting for deferred adds before another bounded send covers the dropped-failure case. Signed A23 evidence remains required under P21/K23. The fake tests exercise its fallback and do not assert Apple's actual behavior.

Proposed edit: apply `/tmp/meguri-p24-deferred-update-closure-20261101.patch`. It deletes one P24 bullet and updates only the corresponding Q11 status/evidence row. It changes no normative rule or other item. Root still owns independent review, privacy/provenance checks and the closure commit. U113/U114, integration and milestone acceptance stay open.

Independent review P2 corrected: Q11 says its table follows P24's lists, so leaving this row marked open after the deletion was inconsistent. The row now uses the existing P24 confirmed status and explicitly limits the evidence to reviewed fake tests; signed A23 stays required by P21/K23. Resume commit `6060d95c` landed after the original source audit and is separate work. The old clean receipt is not claimed as verification of that Resume change. No main-checkout file was edited for this correction.

## Root confirmation: retain the item

`core/crates/meguri-sync/tests/cloud_recovery.rs:1196`, `root_confirmation_safety_finite_permutations_require_current_instance_proof`, passes in the same clean 420-test receipt. Twelve runs permute epoch-only reads, stale Active reads and stale drains, then accept a current-instance root drain or confirming Active read. Each invalid event keeps the builder closed and Service unchanged. Its lost-zone case records a diagnostic event, keeps the local edit/mark, emits no RootConfirmed, isolates zone_missing and stays closed after reopening.

The durable recording clause is not yet proved. P24 says the lost-zone Active read records the returned Active. `sync/cloudkit-recovery#V26.1` explicitly requires that read, including one naming the bound epoch, to become the binding's last applied MGActive before isolation, without confirmation. The test checks only `TraceEvent::RootReadRecorded`.

Current `workflow_runtime.rs::accept_active_proof` emits that trace before its isolation branch. For lost_zone it calls `isolate_runtime_binding` and returns before root persistence. `runtime_store.rs::isolate_runtime_binding` writes binding state only. `workflow_store.rs::save_root_ack` is per-open confirmation storage and must not be used to grant confirmation to a lost-zone read.

Required narrow follow-up: prove a distinguishably newer Service Active version naming the same bound epoch is durably retained with its exact system fields after the lost-zone read and reopening, while RootConfirmed remains absent and the builder stays closed. Add the minimal last-applied fact persistence if the assertion exposes its absence. No root-item deletion is proposed. This audit does not evaluate the separate P3 no-state-fetch P24 item or publication-done coverage.

Independent review: the matching Q11 status was corrected after P2 feedback. Recheck found no P0-P2 issues. No unit or gate acceptance.
