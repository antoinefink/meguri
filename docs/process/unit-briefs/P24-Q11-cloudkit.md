# Unit brief: P24 against Q11 (U113, U114)

## 1. Scope and sources

- **Date and head.** 2026-11-10, at `7e5f7494` (= `origin/master`). No `specs/sync/`, `specs/conformance/`, `core/`, `app/` or `docs/plans/` file changed after `81038ccc`, and no `specs/sync/`, `specs/conformance/cloudkit-*` or `docs/plans/` file changed after `21fc5c1f`.
- **Kind.** Spec-only audit, implementer view. No P24 item is closed here, and nothing is marked passing without a named run. This brief makes no product choice.
- **Specs.** `specs/sync/policy.md` P24 (lines 903-922), plus P3, P4, P5, P10.1, P10.3, P12.3, P15; `specs/conformance/cloudkit-acceptance.md` Q2, Q7.5, Q7.6, Q10, Q11 (lines 474-504), Q12; `specs/conformance/cloudkit-agent.md` J2.1; and the K and V ids each item names in `sync/cloudkit-records` and `sync/cloudkit-recovery`.
- **Plan.** `docs/plans/2026-10-12-1926-feat-meguri-anki-compatible-apple-app-plan.md` "### U113." (line 8872) and "### U114." (line 8905). Neither unit has a `Unit:` trailer at HEAD. `docs/plans/2026-11-02-cloudkit-scoped-type-change-amendment.md` is "Under review (R2066); inactive".
- **Evidence.** `/Users/antoine/code/meguri/build/mg/<job>/<run>/result.json` and `log.txt`. "Clean" and "dirty" are the `commit.dirty` values in `result.json`.
- **Rule of closure** (P24 preamble). An item with a finding "stays open, with its correction and test, until a review reads the fix". "An item is closed by removing it here in that commit" (a spec-writer commit).
- **Reviews of the open items.** One review log reads a change inside an open item: `docs/process/review-log/cloud-media-case-contract-20261102.md` reviewed `cfd1f33d`, the case-variant clarification of OP2's test (a) ("Independent reviewer: `native_bounds_review`, no P0–P2 findings in this clarification"). It does not read the move-first and "op ran" correction that OP2 names, and it says "The P24 item stays open". No other review log found under `docs/process/review-log/` reads the newest correction of an open item.
- **Out of scope.** Q11's O19 rows belong to U47. Two of them name a U114 adapter test: "O12 rules 1, 2, 3 and 8, State, Inputs (`FetchCallEnded`)" ("a U151 or U114 adapter test") and "O13 'One after the other', P3 `FetchDrained`" ("the O19 adapter tests (U114, fake engine)"). `B9-optimizer.md` items 1 and 4 hold them; B9 notes that O13 overlaps OP7 below.
- **Index.** `README.md` in this folder does not list this brief yet.

## 2. Items

Row ids: CF = confirmed, OP = open, X = closed in spec. They are this brief's labels only. ("C1" and "C2" in OP1 are P24's call names.)

Status values:
- **evidenced**: a named per-test PASS on a clean commit. The mark † means that a file of that test changed after the receipt.
- **test written, unrun**: the test exists but has no clean-commit pass.
- **candidate**: a test exists that may cover the clause; a per-clause reading must decide.
- **test missing**: no named test was found.
- **closed in spec**: removed from P24.

`core/` has 123 commits and `app/Packages/MeguriCore` 81 after `60b8cc90`, so every receipt below predates code changes, with or without †. A closure needs a fresh named run at the closure commit.

The "Q11" line of each item quotes the Q11 cell verbatim. The P24 test is the full P24 text, by its clause letters; summaries of P24 clauses here are only pointers.

Run notes:
- `ct-88611` is `core-test/20261102T075513Z-88611` at `60b8cc90`, clean. Its overall status is fail, but the one failure (`meguri-engine suspend::tests::control_lane_contention_counts_toward_suspend_deadline`) is unrelated. Per-test PASS lines are cited.
- `st-55243` is `swift-test/20261102T085734Z-55243` at `60b8cc90`, clean, pass 367/367.
- `ct-73358` is `core-test/20261102T150603Z-73358` at `087490ec`, clean, pass 17/17.
- `cloud_publication_phase.rs` is `core/crates/meguri-host/tests/cloud_publication_phase.rs`. `cloud_publication.rs`, `cloud_recovery.rs` and `cloud_records.rs` are in `core/crates/meguri-sync/tests/`. The Swift files are in `app/Packages/MeguriCore/Tests/CoreBridgeTests/`.

### CF1. Confirmed: K2 sign, K5.2 `MediaRoot`, V10.1, V15.2 step 3, V23.2, V24 A18, V26.1

- **Q11.** "the P24 test (U114, fake CloudKit): codes 1 to 37 at the first reset-path `root` read after zone creation, including the exact record's error within `partialFailure`, are never the unreadable-value sign; failed reads save no `root` and delete nothing; root-specific `unknownItem` is K2's missing-record result and permits only the new-record save; a `partialFailure` without that record's outcome remains a failed read; outside reset an `e`-absent `root` deletes nothing; preserve K2's second-read and zone-not-found rules; with Q6 `encrypted_data_reset` and its variants"
- **Spec change after confirmation.** `399e082a` (2026-11-04, spec-writer) changed this item's test text in P24 and Q11 (the `unknownItem` new-record save). `docs/process/review-log/U114-reset-root-read-20261104.md` says "The reviewed contract clarification preserves the existing missing-root case".
- **Evidence.**
  - Swift `NativeCloudOwnTransportTests.swift` `noCloudKitReadErrorBecomesFoundWithAbsentE(rawCode:)`: 37 cases PASS in `swift-test/20261108T192558Z-26543` @`c6fd2697` clean (the run failed 51/52 on `adoptionPartialFailureUsesOnlyExactIssuedZone`). The U114 log says "The original classification-only 37-code test does not close Q11". The file changed after (`752eaf81`).
  - The same matrix passed 22/22 in `swift-test/20261104T095923Z-4229` @`c812bf87`, dirty (U114 log).
  - Rust first-read `reset_publication_*` tests in `cloud_publication.rs` (7 codes) pass only on a dirty tree (`core-test/20261104T142332Z-11450` @`470a2205`). `docs/process/review-log/U113-reset-adoption-20261104.md` ("Status: WIP; no unit acceptance") records the core reset work.
- **Status.** End-to-end through the core and the fake: **test missing**. Swift classification: **evidenced** †. Rust first read: **test written, unrun**.

### CF2. Confirmed: K12.6 control records, K19.3 step 3, K19.7 `claimed`, K13.1 control records

- **Q11.** "the P24 test (U113): a lost `complete` result meets its own manifest on resend and the generation becomes `complete`; with `conformance/cloudkit-faults` `cloud.claim.after_complete_send`"
- **P24 adds.** "a fetch delivering it first gives the same commit; an `abandoned` manifest gives K19.8's commit with no K19.6 copies".
- **Evidence.** None. No test names the case, and `after_complete_send` appears in no test file.
- **Status.** **test missing**.

### CF3. Confirmed: P10.3 deletion results and `zones` row removal, V26.1 A21 listing

- **Q11.** "the P24 test (U113 `cloud_policy`, A21 false): a listing captured before `DeleteZones` and handled after its result leaves no row; unknown item on a blob deletion sets `deleted`"
- **P24 adds to the test.** "fetched deletions of another `c-` zone remove the row for each reason".
- **Correction, not test.** "a zone-not-found result with the reset flag isolates as K12.5's first row" is in the correction text; P24 names no test clause for it.
- **Evidence (CF3a: the Q11 clauses).** `core/crates/meguri-host/tests/cloud_cleanup_listing_q11.rs` `p24_q11_a21_false_listing_captured_before_delete_zones_handled_after_result_leaves_no_row` and `core/crates/meguri-sync/tests/cloud_media_cleanup.rs` `original_missing_record_result_commits_blob_removal_without_collection_absence`: both PASS in `core-test/20261102T224349Z-83881` @`0a5806f8` clean, pass 5/5. The other `p24_q11_*` tests of the first file PASS in ct-88611; that file is unchanged since. `cloud_media_cleanup.rs` changed after (`f57ad8f7`, `d0b17189`). The blob test maps to the clause by name only.
- **Evidence (CF3b: fetched deletions of another `c-` zone).** None.
- **Status.** CF3a: **evidenced** †. CF3b: **test missing**.

### OP1. Open: P3 `FetchDrained` exception, P12.3 step 3, K12.5, V26.1 no-state read

- **Q11.** "the P24 test (U114, fake engine with per-zone events): a same-identity `signIn` during a call with `meguri-root` deleted, also after that call's root zone fetch ended, isolates `active_missing` with no `FetchDrained{root}`; a collection zone fetch from before the `signIn` completes no absence pass; the A7 failure is logged only when the latest no-state point is the instance's creation and no zone fetch of it started for the zone, also when P4's repeat after a first call joined to a scheduled fetch is the checked call, and a call that fetched the zone before a `signIn` logs only"
- **P24 test.** (a) to (d).
- **Evidence (OP1a: the Q11 clauses).**
  - `CloudKitSyncTests.swift`: `cloudKitManualSameIdentityNoStatePointRejectsEarlierRootFetch(afterRootDid:)`, `cloudKitSameIdentityNoStatePointRejectsEarlierZoneFetch`, `cloudKitManualPrePointCollectionFetchCompletesNoAbsencePass(createdWithoutState:)`, `cloudKitJoinedOrShortCircuitedFetchRepeatsOnce(joined:)`.
  - `CloudKitFetchReadTests.swift`: `cloudKitNoStateFoundControlsUseLatestOriginIncludingJoinedRepeat(origin:)`, `cloudKitNoStateFoundEpochAfterPriorWillLogsWithoutA7OrAbsenceCompletion`.
  - All PASS in st-55243. `CloudKitSyncTests.swift` changed after (`f9f6eae6`); `CloudKitFetchReadTests.swift` is unchanged since.
  - `core/crates/meguri-host/tests/cloud_native_absence.rs` `same_identity_signin_after_zone_end_defers_completion_until_next_zone_start`: PASS in ct-88611; the file changed after (`509662ed`).
- **OP1b: P24-only sub-clauses.** (a) "no batch is built meanwhile"; (b) the following call C2 "gives no `fetched: false` drain"; (d) "appears in Copy Debug Info". No assertion found. (d) asserts the core diagnostics' `a7Failure` (`CloudKitFetchReadTests.swift:121`), not the Copy Debug Info output (D6).
- **Premise.** The no-state point at a same-identity `signIn` rests on A12 (P3: "the signed runs check when it does (A12, P21)").
- **Status.** OP1a: **evidenced** †. OP1b: **test missing**.

### OP2. Open: K15.9, K9.3 "At open", K10.3 `apply_intents`, V2.4 step 8

- **Q11.** "the P24 fault tests (U113): a kill before the move with a fixed-name write before step 8 rewrites no note; a kill before the move runs the move before the op; a rerun after every key was edited leaves no old name; a kill after the rerun's COMMIT settles with no third run; with Q6 `media_content_conflict` and the media rows of `conformance/cloudkit-faults`"
- **P24 test.** (a) to (e). Q11 leaves out (a), including its case-variant part from `cfd1f33d`, and (e)'s deleted-note skip.
- **Evidence (OP2a: P24 (a) to (e)).**
  - `core/crates/meguri-sync/tests/cloud_publication_intent_replay.rs`: 6 `q11_*` tests, all PASS in ct-88611; the file is unchanged since.
  - `core/crates/meguri-sync/tests/cloud_media_name_conflicts.rs`: `extensionless_both_created_conflict_tracks_and_acks_trimmed_hash_name`, `both_created_conflict_reuses_preexisting_live_acked_hash_entity`, `both_created_conflict_reuses_preexisting_live_acked_case_variant_hash_entity` and `both_created_conflict_keeps_canonical_ack_after_case_only_rename_and_reopen`: PASS in `core-test/20261109T212651Z-46750` @`a8c1099d` clean (the run failed 9/71 in other tests); the file is unchanged since.
- **OP2b: related Q6 and fault rows.** Q6 `media_content_conflict` and the `conformance/cloudkit-faults` media rows. `cloud_acceptance.rs` and `cloud_faults.rs` (Q2) do not exist, and `tools/mg --list` has no fake-lane job.
- **Status.** OP2a: **evidenced**. OP2b: **test missing**.

### OP3. Open: K2 reset path, V15.2 step 3, V26.1

- **Q11.** "the P24 test (U114, fake CloudKit): each second-read result; a submission more than 10 s after the second read is not made; a deletion held past 30 s times out and deletes nothing; two publishers in each order where each deletion lands before the other's creation; with Q6 `encrypted_data_reset`, variant `survivors`"
- **P24 test.** (a) to (d). Q11 leaves out (d)'s late-deletion order: "P2's deletion lands after P1's creation and `root` save, P1's next media start sets `media_zone_missing`, status offers V15, and no blob named with P1's key is saved after that media start".
- **Evidence.** None. Only the first-read precursors of CF1 exist.
- **A18.** See D1.
- **Status.** **test missing**.

### OP4. Open: K12.3 "Refused save conflicts", K12.6 claim row, K19.4 "Refused anchor", K13.1, K10.3 `refused`, K12.1 rule 8

- **Q11.** "the P24 test (U113): an older accepted version fetched after a refused one is stale and keeps the row; a newer one of the origin deletes it; with K22.2 \"a refused save-conflict version\""
- **P24 test.** (a) to (e). The Q11 cell matches P24 (b) (the `erev` 5, 7, 6 case, with its `RunCleanup` clause) and (c). Q11 leaves out (a), (d) and (e).
- **Evidence (OP4a: P24 (b), (c)).** `core/crates/meguri-sync/tests/cloud_incoming_intents.rs` `erev_order_ignores_duplicates_and_older_accepted_versions_cannot_clear_newer_refusal` and `cloud_native_absence.rs` `stale_refused_and_deleted_intake_are_seen_without_applying_them`: PASS in ct-88611. It is not shown that either test runs P24 (b)'s save-conflict path (S at `erev` 7) or `RunCleanup`, or (c)'s delete-and-apply in the K13.1 step 2 transaction.
- **OP4b: P24 (a), (b)'s `RunCleanup` blob, (d), (e); K22.2.** No test names `claimStuck`.
- **Status.** OP4a: **candidate** (passing, clean, †: `cloud_native_absence.rs` changed after). OP4b: **test missing**.

### OP5. Open: K19.4 "Another generation fetched first", K19.2, K19.5, K12.6, K12.3, K12.4, K12.1 rules 7 and 8

- **Q11.** "the P24 tests (U113, two devices): a `local` generation waiting for a `claiming` one joins it when that one is `lost`; an edit on a `lost` generation's notetype joins it; a note of it waits until `rolled_back`; an `asset` entry decides when it settles; no claim anchor is added back alone; a claim is still built whole after the record bound is halved, and repeated `limitExceeded` reports `claimStuck`; with K22.2 \"control records\""
- **P24 test.** (i) to (vii) with two devices on a fake server, and "as before" the offline-first-fetch case and the CSS-edit case.
- **Evidence.** `core/crates/meguri-sync/tests/cloud_generation_runtime.rs` has 15 tests. Its header says they are "not SDK/Q11 proofs". Some are ledger-level parts of (i) and (ii), for example `claiming_successor_waits_and_retry_or_loss_joins_in_one_durable_result_transaction` and `fetched_anchor_decides_local_and_lost_edit_joins_but_rolled_back_fixture_is_final`. They pass only on a dirty tree (`core-test/20261102T184358Z-33290` @`7d2ab7a3`, 41/41).
- **Status.** End-to-end with two devices: **test missing**. Ledger parts: **test written, unrun**.

### OP6. Open: V17.6, V2.4 step 8, V14.2 steps 6 and 9, V15.2 steps 1, 2 and 4, V13.4, P10.3 item 4

- **Q11.** "the P24 `cloud_recovery` fault tests (U113): the binding before's intents are settled before the new binding's scan; the preview's backup notes; a salvage binding's row goes with its last copy; with Q6 `epoch_switch_offline_device` and V23.2 \"A waiting generation at Replace iCloud\""
- **P24 test.** (i) to (iii), and "as before": "every V17.6 table is emptied after `done`, also after a kill right after that commit; the row goes with its last copy; an active binding, and an isolated one that keeps its work, keep their rows with no copy". Q11 leaves out the "as before" clauses and (i)'s V14 step-6 variant.
- **Evidence (OP6a: (i) for V15, (iii)).** `cloud_publication.rs` `prior_binding_media_and_resolution_intents_settle_before_new_publication_scan`: PASS in ct-73358; the file changed after (`f30ada47`, `eb352ba9`). `cloud_recovery.rs` `recovery_salvage_owner_row_disappears_with_its_last_expired_copy`: PASS in `native-durability/20261108T213412Z-18934` @`9dad9d02` clean, pass 191/191; the file is unchanged since.
- **Candidates for "emptied after `done`".** In `cloud_publication_phase.rs`, all PASS in ct-88611: `stopped_binding_committed_incoming_settles_before_republication_and_survives_cleanup` (comment "V17.6 / V2.4 step 8"), `stopped_binding_resume_settles_committed_incoming_before_scan` (comment "P24 V2.4 step 8 / Q11"), `queued_ready_incoming_is_not_applied_by_republication_and_is_discarded_only_at_completion` and `actual_field_conflict_retains_isolated_owner_once_after_republication_and_clean_reopen`. They assert that the prior binding's `apply_intents`, `inbox` and `refused` rows are empty. V17.6 lists 14 work tables, and no kill right after the `done` commit was found. The file changed 13 times after `60b8cc90` (latest `e7ee18db`).
- **Test written, unrun.** `core/crates/meguri-sync/tests/cloud_adoption_prior_intents.rs` `cloud_adopt_settles_*` and `cloud_publication_phase.rs` `local_information_qualifies_isolated_work_without_claiming_a_retained_backup` pass only on dirty trees (`core-test/20261104T142332Z-11450`, `core-test/20261110T083716Z-87097` @`57c9dfd2`). They map to (ii) and the V14 variant by name only.
- **Status.** OP6a: **evidenced** †. Emptied after `done`: **candidate** †; after a kill: **test missing**. (ii) and V14 step 6: **test written, unrun**. The rows kept with no copy, Q6 and V23.2: **test missing**.

### OP7. Open: V10.2 mark, V2.3 "replaced through", K5.2 `Active` field 5, K18.3 step 5, K10.3 `bindings`, P3 "The start of a fetch call"

- **Q11.** "the P24 test (U114 with U113, two devices): after a restore or a `file_mismatch` the publisher writes no `replaced_through_ms` and every acked entity counts; a no-state `fetched: false` drain leaves the last-drained instant; with V23.2 \"A switch during a long fetch call\""
- **P24 test.** (i) to (iii), and "as before" the joined fetch, the relaunch, the clock 2 h ahead and the 721 marks over 31 days. Q11 leaves out (ii) and the "as before" clauses.
- **Evidence.** None. `replaced_through_ms` appears in test code only as fixture values: `cloud_recovery.rs:873`, `core/crates/meguri-ffi/src/cloud_connection_tests.rs:731` and `core/crates/meguri-testkit/src/cloud/devices.rs:283` (the last two are `None`).
- **Status.** **test missing**.

### OP8. Open: V2.3 "Rows of another build", V2.4 step 3, V21 `Busy`, V15.2 steps 2 and 5, V15.3, `engine/collection-replace#R6.7`

- **Q11.** "the P24 test (U113, two build configurations): another build's published decision survives a Cancel; the three restart cases with that decision; with V23.2 \"Rows of another build\""
- **P24 test clauses.** Neither build settles the other's rows; a cancel after an import's fence keeps that fence; the three kill cases around the sidecar write; another build's `CloudAdopt` intent activates with its recorded decision; A's Cancel leaves B's decision with no fence, "B's binding stays usable through `done` and B shows no V12.2 sheet"; "B killed between its `confirmed` row and its write runs the write at relaunch (case (a))"; "A at `bound` ... cancels at restart (case (c))".
- **Evidence (OP8a: the Q11 clauses).** `cloud_publication.rs` `other_build_decision_survives_cancel_and_all_three_bound_restart_cases`: PASS in ct-73358 and ct-88611. `cloud_publication_phase.rs` `decision_write_kill_restart_reopens_durable_binding_and_preserves_new_restore_or_other_build_decisions` and `cancellation_leaves_intervening_restore_or_other_build_decision_and_row_unchanged`: PASS in ct-88611. Both files changed after.
- **Candidate for "neither build settles the other's rows".** `cloud_records.rs:123` `builds_share_a_file_but_cannot_read_or_write_each_others_facts` opens three build scopes on one directory; a foreign binding's inbox, engine state and fetched commits are refused (`BindingNotFound`). PASS in ct-88611 and in `native-durability/20261108T213412Z-18934` (`cloud_records.log`); the file is unchanged since `60b8cc90`. Whether "cannot read or write" shows "settles" needs a reading.
- **Status.** OP8a: **evidenced** †. Neither build settles: **candidate**. The import fence, `CloudAdopt` activation, B's usable binding and V12.2 sheet, cases (a) and (c) as worded, and V23.2: no mapped assertion; each needs a per-clause reading of the OP8a tests, else **test missing**.

### OP9. Open: K19.8, K19.6, K12.6, K19.4 "Claim landed", K19.5, V18, V2.3

- **Q11.** "the P24 tests (U113, fake server, four devices): an abandon covers the owner's later generation of the same anchor and restores from its layout; a complete later generation ends the chain, and the restoring generation leaves out its anchors (none published when it covers them all) and its claim succeeds; one rebuild through every rolled-back inverse; the hold ends per anchor; with K22.2 \"an abandoned own generation\" and Q6 `abandon_three_devices`"
- **P24 test.** (1) to (4) with devices A to D, and "as before": "G2 delivered before G's `abandoned` manifest leaves G2's layout" and the landed-claim conflict case (A8, signed runs).
- **Evidence.** None. `abandon_generation` and `abandon_three_devices` appear in no test file.
- **Status.** **test missing**.

### OP10. Open: P6 gate row, V15.2 at `confirmed`, V2.3, V2.4 step 3, V15.3

- **Q11.** "the P24 `cloud_recovery` tests (U113): a `confirmed` row settled or removed; a Cancel after V8.2 or a restore returns the binding before isolated, with no scan; with P21 \"Publication (P6)\""
- **P24 test clauses.** The kill between `confirmed` and `bound` with another or the same account; the kill at `confirmed` with a file identity change; "no `Binding` input carries `publishStep: confirmed`"; the kill at `zones` with the file replaced; the in-process restore while a `publish` op waits "with its new binding isolated by an account event"; "with the identities unchanged, Cancel returns the binding before `active` and its scan sends the edits made during the publication to the old epoch".
- **Evidence.**
  - `cloud_publication.rs` `confirmed_restart_requires_exact_account_publication_and_native_file_authority`: PASS in ct-73358 and ct-88611. It asserts `!rig.binding_inputs().iter().any(|b| b.publish_step == Some(PublishStep::Confirmed))` (line 1630 at HEAD; line 860 at `087490ec` and `60b8cc90`).
  - `cloud_publication.rs` `cancel_after_file_mismatch_or_restore_preserves_isolation_fence_and_backup_notices`: PASS in ct-73358 and ct-88611. It restores a backup, expects `ReplacedLocally` and a pending `Restore` fence with no scan. It does not show that the new binding was isolated by an account event.
  - `cloud_publication_phase.rs` `cancellation_after_native_file_mismatch_restores_fence_and_keeps_old_binding_isolated`: PASS in ct-88611.
  - Both files changed after.
- **Status.** The Q11 clauses and "no `Binding` input carries `publishStep: confirmed`": **evidenced** †. The in-process restore with an account-event isolation: **candidate** †. The unchanged-identities Cancel and P21 "Publication (P6)": **test missing**.

### Closed in spec

| # | P24 item | Q11 cell now | Closing commit | Receipts |
|---|---|---|---|---|
| X1 | K9.5, Fresh deletions, K10.3 `blobs`, P10.3 item 2 | "P24 test closed (2026-11-02): core72126, 19/19, 0 ignored" | `f6236e81` (spec-writer) | `core-test/20261102T221128Z-72126` @`601e3747` is **dirty** |
| X2 | K10.2, K11.1, K11.4 | "P24 confirmed" | `98382e55` (spec-writer) | none in Q11; `cloud-p24-scan-closure-20261101.md` cites `core-test/20261101T113927Z-21225` (`dc68c0a5`, clean, 27/27) |
| X3 | K13.1 asset retries, P4 `RetryAssetEntries` | "P24 confirmed" | `70901b08` (spec-writer) | none in Q11; `cloud-p24-asset-retries-closure-20261101.md` cites `core-test/20261101T134745Z-91787` (`4fa09dad`, clean, 420/420) and `swift-test/20261101T144749Z-20763` (clean, 2/2) |
| X4 | K17.4, A9a | "P24 test closed (2026-11-02): core-test/20261102T222530Z-79794, selected 4/4" | `36734582` (spec-writer) | @`0777a711`, clean, 4/4 |
| X5 | K5.2 `Active`, K18.3 step 5, K10.3 `zones`, V2.3 | "P24 test closed (2026-11-02): core-test/20261102T224349Z-83881, selected 5/5" | `303ed37a`, **`Role: integrator`** | @`0a5806f8`, clean, 5/5 |
| X6 | `root_read` acceptance | "P24 confirmed" | `68e86985` (spec-writer) | Q11 cites `core-test/20261101T193641Z-63923` @`9d88dddf`, **dirty**, 545/545; `cloud-p24-root-confirmation-20261101.md` also cites 91787 (clean, 420/420). SDK evidence is still required (P21, K23). |
| X7 | K12.4 "Between full updates", A23 | "P24 confirmed" | `aeb9e555` (spec-writer) | none in Q11; `cloud-p24-deferred-updates-20261101.md` cites 91787 (clean, 420/420). Signed A23 is still required (P21, K23). |

The review logs are in `docs/process/review-log/`.

### Fake and signed lanes

- No `cloud-fake`, `cloud-faults` or `soak` result exists under `build/mg/`, and `tools/mg --list` has no such job.
- A shared fake CloudKit service and multi-device harness exists in `core/crates/meguri-testkit/src/cloud/` (`service.rs`, `devices.rs`, `coordinator.rs`, `schedule.rs`, `AGENTS.md`), with a four-device test (`core/crates/meguri-testkit/tests/cloud_schedule.rs:324` `four_actual_devices_exchange_and_unsupported_programs_block_explicitly`). `tests/cloud_fake_service.rs` says: "These synthetic cases prove no P24, U117, Q5 or signed-lane acceptance." It is a base for the two-device and four-device tests, not evidence.
- `cloudkit-signed` has three runs, all clean, args `["--selected"]`, `details.selection` `sync_now`, status fail (`selected-evidence`): `20261110T144620Z-22782` @`b678b624`, `20261110T145657Z-76388` @`1aa1d518`, and `20261110T150455Z-9498` @`ef39c65a` (`latest`). None covers a P24 item.

## 3. What remains, in a suggested order

1. **Spec-writer, before more tests.** Decide D1 to D4 in section 4. D1 decides whether OP3 can close.
2. **Write the missing core tests (U113, `tools/mg core-test`, Docker).** Put each test where Q2 places its section (`meguri-sync/tests/cloud_records.rs` or `cloud_recovery.rs`; `cloud_policy.rs` for P21 rows), and name the spec ids it proves. Build multi-device cases on the `meguri-testkit` fake (section 2).
   - CF2: a lost `complete` result; the fetch-first variant; the `abandoned` manifest.
   - CF3b.
   - OP4b: (a), (b)'s `RunCleanup` blob, (d), (e).
   - OP5: (i) to (vii) and the two "as before" cases, two devices.
   - OP6: the V14 step-6 variant, all 14 V17.6 tables after `done` and after a kill, the rows kept with no copy, (ii) on a clean run.
   - OP8: the clauses with no mapped assertion.
   - OP9: (1) to (4) and the "as before" cases, four devices. OP5 and OP9 stand on approved K19.2 (see D5).
   - OP10: the unchanged-identities Cancel.
3. **Write the missing U114 tests (fake CloudKit or fake engine).**
   - CF1 end-to-end: 37 codes through the core and the fake on the reset path, with the `partialFailure` per-record variants.
   - OP3: (a) to (d), with a suspendable continuous clock and a held deletion.
   - OP7: (i) to (iii) and the "as before" cases, with U113.
   - OP1b: assert no batch is built while the check is open, assert C2's drain, and check the Copy Debug Info output (D6).
   - Swift runs through `tools/mg swift-test --package MeguriCore` (host).
4. **Read the candidates per clause** (OP4a, OP6, OP8, OP10) against the full P24 text, and add an assertion where they fall short (D3).
5. **Rerun every named test of an item on the closure commit, clean.**
   - Rust: `tools/mg core-test -- <filter>` (Docker).
   - Cloud tracking durability: `tools/mg native-durability` (host).
   - Swift: `tools/mg swift-test` (host).
   - This applies to every row, with or without †.
6. **Then a review of each item's newest correction, then a spec-writer commit** that removes the item from P24 and records the run in Q11.
7. **Rows without a lane yet.** Q6 rows, the `conformance/cloudkit-faults` rows and K22.2 run in U117 (`cloud_acceptance.rs`) and U118 (`cloud_faults.rs`); neither file exists. Q10's R2066 row also needs them before acceptance.
8. **Signed lane (J2.1: the Mac app plus the VM iPhone and iPad simulators; `tools/mg cloudkit-signed`).** Needed for:
   - OP9's A8 clause (Q7.6 leg `structure`);
   - X6's SDK root evidence and X7's A23 (P21, K23; A23 is leg `engine`);
   - A7 behind OP1 (legs `no_state` and `engine`);
   - A12 behind OP1's no-state point at a same-identity `signIn` (leg `sign_out`, assisted; Q7.2 defers assisted legs to the final session).
   - No physical device is needed.

## 4. Conflicting or ambiguous text

- **D1. A18 and OP3.**
  - Text: `sync/policy#P24` (K2 reset path) says whether a device whose keys have not arrived after a reset sees its own `MGEpoch` with `e` absent "is known only from the signed runs (A18)".
  - Against it: `conformance/cloudkit-acceptance#Q7.5`: "Decided 2026-10-27: no signed leg triggers an encrypted data reset, which would need account security changes beyond a test step. A18 stays covered by the fake and by V10.1's fallback, which deletes nothing on any sign other than K2's; the ledger records A18 as `unverified`, which does not fail the run." Q12 repeats it, and Q7.6 has "A18 | none | `unverified` (Q7.5 above)".
  - Decision needed (spec-writer): reconcile P24's sentence with Q7.5, and say whether OP3 can close with A18 `unverified` and the fake's `survivors` variant (Q3.2). No waiver is assumed here.
- **D2. Q11 status labels.**
  - Text: Q11 says "The table follows P24's Confirmed and Open lists" and "Closed test obligations remain in the table with their evidence".
  - Against it: rows X2, X3, X6 and X7 still read "P24 confirmed", although `98382e55`, `70901b08`, `68e86985` and `aeb9e555` removed them from P24. X2, X3 and X7 cite no receipt in Q11. X1 and X6 cite dirty receipts, while Q10 accepts a criterion when it "passes on one clean commit".
  - Decision needed (spec-writer): relabel the rows and attach clean receipts (the closure logs in section 2 name some), or record why not.
- **D3. Q11 cells are narrower than P24.**
  - Text: Q11 says "each item's test is the one below, as P24 and O19 name it".
  - Against it, the clauses that each item's "P24 adds" or "leaves out" line in section 2 names: CF2, CF3, OP1, OP2, OP3, OP4, OP6, OP7, OP8, OP9 and OP10.
  - Decision needed: confirm that the full P24 text is the test requirement. This brief assumes it, under the P24 preamble.
- **D4. Closing role.** The P24 preamble says "An item is closed by removing it here in that commit" (a spec-writer commit). `303ed37a` (X5) carries `Role: integrator`. Decision needed: accept that closure, or redo it as a spec-writer commit.
- **D5. Inactive amendment.** `docs/plans/2026-11-02-cloudkit-scoped-type-change-amendment.md` ("Under review (R2066); inactive") proposes to change K19.2 "Joining", on which OP5 and OP9 stand. It is not a conflict today. Decision needed: write the tests against approved K19.2 now, or wait for the amendment.
- **D6. The Copy Debug Info surface.** P24 OP1 (d) says the A7 failure "appears in Copy Debug Info". The existing tests assert the core diagnostics' `a7Failure`. Decision needed (reviewer): does the core diagnostics record count as the Copy Debug Info output, or is a test of the Swift surface needed?
- **D7. Test location.** Q11 says each test "lives where Q2 puts the section it tests". Many existing P24 tests are in `meguri-host/tests/*` and `meguri-sync/tests/cloud_publication*.rs`, not in Q2's `cloud_policy.rs`, `cloud_records.rs` or `cloud_recovery.rs`. Decision needed (reviewer): accept these locations or move the tests.

## 5. Diagnosis in progress (as given by the master agent)

- **Same-backend overlap.** The overlap of a standalone `RequestSend` with a `ConfigureAutomatic` on the same backend is under diagnosis. No rule is added here. The text that bears on it:
  - `sync/policy#P4`, the `ConfigureAutomatic{on}` row: "While an exchange runs, `SyncCoordinator` holds it until that exchange's `ExchangeEnded`, so the exchange is never cut by a new instance (decided 2026-10-27); the held action returns at once, so the rest of its action list runs on the old instance, and the re-creation with its update and fetch runs after `ExchangeEnded` (decided 2026-10-27)."
  - The same row: "it stops the old instance as `StopEngine` does and creates the new one only after the old one's `EngineStopped` ... That stop is not a `StopEngine` action and does not change the token."
  - P4: "`RequestFetch`, `RequestSend`, `ReadBlobs`, `ResumeMedia`, `RetryAssetEntries` and `DeleteZones` (review round 4) also run in a task of `SyncCoordinator` ... a later action list never waits for them. An action of such a task uses the engine instance current when it starts (as P12.3's steps do), and does nothing when its guard is no longer current or no engine runs for its binding (P5)."
  - P10.1, row `DeviceSettings{automatic: on}`: "`ConfigureAutomatic{on}`, `UpdatePending{full}`, `RequestFetch{root, collection}`, `RequestSend{collection}` when sendable, `ResumeMedia` when media is on". The re-creation runs its own update and fetch.
  - P5 marks an instance stopping only "when the policy emits `StopEngine` for it".
  - Gap, listed only: during an exchange, the held action puts the row's `RequestSend` on the old instance. No text orders a send whose task still runs when the old instance stops after `ExchangeEnded`, or a send emitted when no exchange runs, against the re-creation's stop. P5's stopping mark does not cover that stop.
- **P4 hold.** Internal contract review confirms that P4 holds through an active `ExchangeEnded`. That is consistent with the P4 text above; no conflicting text was found.
- **P15.** Internal contract review confirms that P15 covers only real cancellation. The P15 row is "`operationCancelled` after the app's own `StopEngine`, `ConfigureAutomatic` or cancel"; an SDK authority refusal falls to its own row or to "any other error". No text was found that allows relabelling a refusal as cancellation.
  - Watch item: `docs/process/review-log/cloud-native-core-refusal-diagnostic-20261110.md` says of `CloudMedia::start_next_work`: "`preparation_error` and its cancellation mapping are unchanged". The reviewer should check that mapping against P15.
- **Signed path.** The approved signed path is the Mac app plus the VM iOS simulators (`conformance/cloudkit-agent#J2.1`: "Physical iOS devices are not a new requirement"), with no physical expansion. No text was found that conflicts with J2.1's device set. D1 is a conflict between P24 and Q7.5/Q12, not with J2.1.
