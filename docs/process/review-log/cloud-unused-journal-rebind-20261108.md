# Unused review journal binding

U151 and related U113/U114 work remain unaccepted. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Component start: `3299984cb480715b1f4aef4e64d0f38e40790638`.

The normal phone open reports the fixed category `pending_answer_identity_mismatch` during the review-session open step. Its journal eligibility is unknown. No actual journal contents were read by the orchestrator and no phone data repair has been performed.

## Contract and proof

Spec commit `3299984c` adds only the independently reviewed managed-opening exception for an exact unused version-2 Header. Raw strict opening and all history-bearing, compacted-history, corrupt, retired, legacy and wrong-platform rules remain. The exception requires a durable exact archive, a distinct fresh incarnation/current binding and zero replay. Spec writer session: `s-20261108T190858Z-d79453ef`.

Filtered test export `cleanroom/20261108T203152Z-79290` uses fixer session `s-20261103T133750Z-f276f58e`. Independent review clears the real managed Engine/ReviewService setup, strict raw refusal, pre-Ready archive observation and teardown. Existing production code and tests are unchanged.

`core-test/20261108T203725Z-81786` compiled and ran the exact regression with test-hooks. It failed at the intended pre-Ready assertion with `review journal: pending_answer_identity_mismatch`. Workers and engine close completed before the assertion. This is semantic RED evidence, not a passing test. Implementation, fault cases, negative coverage and phone qualification remain pending.

## Implementation export (verification pending)

Fixer session `s-20261103T133750Z-f276f58e`; filtered baseline `38e22cb` (permitted-source snapshot, no upstream history). Upstream component/spec base remains `3299984cb480715b1f4aef4e64d0f38e40790638` above. This pass writes only permitted source in the filtered checkout; the orchestrator owns integration, attribution, independent review and all verification.

The review worker now qualifies the complete retained journal for C12.5 during Opening. Its evidence lasts through the existing Migrate barrier, including any opening compaction, and ends before Ready. A matching unused header follows normal recovery. A mismatched header qualifies only as one complete current-platform v2 frame, with counter 1 and no notice or event records. Publication keeps an exact durable archive, creates a different incarnation and transfers no events. A header carrying the new notice requires the matching archive proof before recovery. The writer control uses the existing SQLite native-identity validation API. Append receipt handling checks files without waiting on that writer. Raw opening remains strict; the codec additionally recognizes the specified migration reason.

Source consistency only: the complete existing journal test module, including the recorded RED proof, is preserved byte-for-byte against the filtered baseline. No build, test, formatter, privacy/similarity scan, native run, VM/device work, network access or main-checkout write was performed. No acceptance or phone-eligibility claim follows.

Seed modules: `session::journal::managed::{tests,lifecycle_tests,retained_directory_tests,opening_fault_tests}`. Cases cover eligibility, malformed input, restart/compaction archives, identity/link changes, publication faults and boundaries, ordinary opens, native rebinding, subsequent mismatch refusal and failed-open cleanup. Directory replacement is checked against the held descriptor. Protected proof: `session::journal::tests::c12_managed_open_rebinds_pristine_v2_after_path_change_before_ready`. R2 privacy edits affect new names and prose; other assertions keep their meaning.

At export time the new cases had no compilation or execution evidence; the subsequent round below records focused seed results. Direct full-sync and directory-sync syscall failures, cancellation/drain with retained notices, concurrent mutation timing and Apple integration still need root evidence. Existing cancellation, drain and safety tests remain untouched. Root owns formatting, checks and acceptance. Phone qualification has no result. Commit trailers: Role fixer; Spec sched/review-session#C12.5, sched/review-session#C12.7, sched/review-session#C12.8; Session s-20261103T133750Z-f276f58e.

## Integration round 1

Round 1 integrated `cleanroom/20261108T204659Z-85357` with the prior journal tests and protected regression intact. Formatting receipt `core-format/20261108T210652Z-97096` is green. Focused receipt `core-test/20261108T210714Z-97522` records 68/68 engine library tests passing, including the earlier RED proof and eleven new cases. An earlier restricted-Docker infrastructure error preceded the successful host-authorized MG run through OrbStack. Receipt `doctor/20261108T210727Z-98017` reports healthy Docker and VM services. These are seed results, before R2 edits.

R1 P2: parent policy. R2: two reviewers clear. Existing Writer validation available. Seed 97522 passed. Acceptance pending.

## R2 candidate — root verification pending

Same fixer session `s-20261103T133750Z-f276f58e`; resume the reviewed seed without another unit or feature change. The focused seed result is 68/68 passing in `core-test/20261108T210714Z-97522`. Both independent reviewers identified P2: the added parent nofollow and ownership restrictions rejected previously allowed journal locations before matching-file recovery.

R2 policy: no parent nofollow, effective-UID or journal-to-parent UID requirement. Matching pristine journals take ordinary recovery. Held directory device/inode must match the followed path; replacement fails the revised parent test. Preserved checks: regular, single-link, nofollow leaves; retained handles and bytes; source/archive publication; migration archive proof; current collection lock and SQLite validation. Pre-seed tests and the protected proof are unchanged.

Privacy receipts: 99681/3464/6378. Privacy pending.

R2 review receipts: two independent reviewers, no P0–P2 findings. This pass performed source edits and one consistency comparison only; no formatter, build, test, scan, network, device or main-checkout action. Root owns export integration and verification. Direct full-sync/directory-sync failures, retained-notice cancellation/drain, concurrent mutation timing, native integration, required gates, phone eligibility and unit acceptance still lack proof.

## Final focused checks

R2: `core-test/20261108T211451Z-3729`, 68 passed. R3/R4 change new names and log text only. Final source review: no P0/P1 findings; previous P2 closed. The complete existing journal test module matches `b2238289`, including the protected regression.

`private/20261108T212217Z-8036` passes. Final formatting passes without edits. `core-test/20261108T212300Z-8917`: 104 passed, 19 excluded by the focused filter. Coverage includes engine library tests, session recovery, session durability with process-kill cases, and five host adoption guards. Native checks, phone outcome, lint, similarity and the integration gate remain pending. No Unit acceptance.

## Native and phone results

Repair commit: `9dad9d02`, pushed to master. Lint `core-lint/20261108T212345Z-9372` passes. Similarity `20261108T212346Z-9654` passes all four changed Rust files, with no exceptions. Native durability `20261108T213412Z-18934`: 191 passed, zero failed.

Normal signed phone run `20261108T212437Z-10768` passes. Preserved inspection `drive/20261108T212720Z-14206` passes both steps with continuity verified: existing decks appear, collection actions are enabled and the opening error is absent. No reset or import was used. Review render drive `20261108T212926Z-15742` fails wait step 6 after Study: the review remains loading. No rating was sent. See `cloud-phone-review-start-20261108.md`. Real review/media convergence and the full integration gate remain pending; U113/U114/U151 are unaccepted.
