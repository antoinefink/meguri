CloudKit record reads after a missed zone, 2026-11-01.

U113 start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Component base: c6c912a1; export integration base: 943b7f83.
Final integration base: 03d64a36; the separate Home fix stays intact.
Implementer: s-20261025T134357Z-235af9a2.
Fixer: s-20261025T162405Z-9dc656e4.
Authority: sync/policy P3, P4 and P24; sync/cloudkit-records K12.5;
sync/cloudkit-recovery V26.1; conformance/cloudkit-acceptance Q11.

The native fetch path did not call the existing missing-zone read helpers.
The change carries Core-issued record-read handles through FFI and the native
transport. Each read retains its original SDK, fetch, scan and no-state point.
The final queue admission checks those facts again. Reads do not complete an
absence pass. A found record can produce a false drain; failed reads wait for
the next trigger. Diagnostics retain the actual service outcomes and distinguish
the latest no-state point from an earlier zone fetch.

Initial native RED: swift-test/20261101T203435Z-84775. Both root-loss timings
failed at the empty transport trace; both pre-point absence controls passed.
A later Swift macro compile error was corrected without changing its assertion.

Independent review R1 found P1: stop waited for a held account resolver after
the actual read had ended. It also found P2: ending the owner before the Active
reply could forget an accepted collection-zone loss. The tests retain the real
transport callbacks, a held resolver, normal stop and the late result.
swift-test/20261101T212053Z-28769 reproduced the stop ordering failure.
swift-test/20261101T211115Z-18428 reproduced the two lost-zone stop cases.
R2 added cancellation and kept the collection loss until stop.

Independent review R2 found P1: that loss was still only in process memory.
P2: a failed stop could drop one owner and retain the other.
Tests use SIGKILL/reopen and a publication-file fault.
The read-only installed-SDK hook is restricted to the existing test-hooks feature.
It observes ownership without creating or replacing an SDK.
core-test/20261101T221222Z-72664: 2 failed, 4 passed.
swift-test/20261101T220003Z-62246: 2 failed. Assertions were kept.

Two test observations were corrected after source review: persisted Active is
the adapter snapshot's Active fact, separate from RootAck; a reopened isolated
runtime does not hydrate the process drain cache. Exact stored Active bytes,
unchanged RootAck, pending rows and no-send checks remain required.

R3 commits a loss obligation before returning the Active follow-up. Open reads
that fact from storage and isolates only the matching publication and file.
Ordinary callbacks keep the stored fact. That fact blocks confirmation,
sends and drains. A stop error leaves the owner installed.
Independent R3 source review found no findings requiring repair.
core-test/20261101T222006Z-78147 PASS6/6 crash and account cases.
swift-test/20261101T222051Z-80974 PASS10/10 selected native methods.

The four new failed-write cases compiled in core-test/20261101T223054Z-87441,
then blocked before any case ended. The owned run was cancelled and joined;
its processes and run-labelled containers ended. No test passed in that run.
Independent review found P1: the commit-error path held TRACKING_ACCESS through
with_scan, then entered invalidate_locked and tried to lock it again.
That cancelled run did not reach the reopen assertions.

R4 splits invalidation into an entry that takes tracking access and a helper
for callers that already hold it. The failed commit uses the latter. State
flags and the stored undrained mark stay inside the original lock boundary.
The original error is returned. Independent review found no remaining finding
in that change or new recursive entry among the other callers.
core-test/20261101T223638Z-94220: all four cases ended and failed at the
unchanged RootAck requirement after reopen. Later final assertions did not run.
Independent review found P1: rollback removed the loss fact, while no durable
fact required a new epoch check. Saved serialization allowed a cached Active
to confirm again. A failed result must keep that check pending across opens.

R5 stores a pending epoch check before returning its probe. Failed outcomes
keep it; a committed loss replaces it atomically with the loss fact. A reopen
checks its binding and file before admitting a new probe. Cached root proof,
sends and drains remain blocked while that check is pending. SDK bytes stay.
Review found no P0/P1 after the same lock helper was used on its Found branch.
private/20261101T225140Z-97182 and similarity/20261101T225203Z-97813 pass.
core-test/20261101T225200Z-97555 stopped at an undeclared UpdateKind import;
No tests ran. A source-only patch qualified the existing enum.

R5 review P2: a completed fetch with fetched:true was treated as fetched:false.
The absence pass did not complete. P3/K12.5 allow the successful post-point
drain to end pending verification in the intake/absence transaction. The
accepted-loss case must stay blocked. The next regression exercises this path.

core-test/20261101T225910Z-99336 compiled and ran all five cases. The new
progress test failed at its actual false drain versus the required true drain.
The four write-failure cases passed the earlier RootAck and data assertions,
then failed an absence-bit equality. Review confirmed that equality was wrong:
real ZoneMissing isolation clears the ledger. Its corrected oracle must require
exact ZoneMissing, a false completion bit and a null persisted absence ledger,
while keeping all zero-confirmation/send/drain and data requirements.

R6 clears matching verification and records fetched:true in one transaction.
File and publication reads precede the writer guards; SDK,
guard, no-state point, binding and zone are checked again afterwards. Failed,
stale and unchanged callbacks keep the fence. Accepted loss keeps priority.
The approved test correction requires exact ZoneMissing and a cleared ledger.
Independent R6 review found no P0/P1 or new substantive P2 finding.
core-test/20261101T230631Z-2044 PASS5/5: all failed-write cases and the true
drain regression. Existing crash and native tests run in the common check.
core-format/20261101T230559Z-921 PASS.
private/20261101T230850Z-2952 PASS26/26 after four redundant comment lines
were removed. No scanner rule, exception or test condition was changed.

Final affected checks:

- core-test/20261101T230911Z-3262 PASS557/557, no selected skips.
- swift-test/20261101T231235Z-4539 FAIL128/129 checks. The old V9.1 test
  expected sending after an empty fetch at a new no-state point. Review
  confirmed that it lacked the collection/root proof P3 now requires.
  Its correction keeps the empty-fetch assertion, proves a blocked send keeps
  ACK/local revision/flights/mark/pending unchanged, then supplies actual
  collection and typed Active root fetches before retaining the exact ACK3.
  Independent test review found no relaxed assertion or production change.
- swift-test/20261101T232140Z-7160 PASS1/1 on that corrected exact case.
  The other 128 checks were unchanged. The earlier common receipt stays failed;
  these receipts do not represent a new complete passing Core run.
- swift-test/20261101T232159Z-8157 FAIL39/42, no skips. Outstanding:
  realServiceAssetInboxAutomaticallyRetriesOncePerDrainAndResetsOnReopen;
  realServiceReviewAndRegisteredMediaSurviveReopenAndFreshAdoption;
  explicitResumeConsumesOnlyTheRetainedResumeDecisionAndStartsWithoutCursor.
  Current-owner status/Resume commands report staleCommand. The first issue
  was attributed to the earlier test; its close and Service join finished.
  Read-only tracing locates the same error in the new test's backend. The
  process-lifetime fanout task may explain attribution; that is not yet proven.
  Both fixtures provide scoped callbacks and real control records, so another
  missing-proof fixture correction is not justified by the source evidence.
- core-lint/20261101T232915Z-11680 PASS, including 17-crate layering checks.
- private/20261101T232233Z-9296 PASS26/26 source files.
- similarity/20261101T232236Z-9542 PASS on the final fixture and source.

The six-round limit is reached. E-20261101T232833Z-dbc5 records blocked-red
for U114 under docs/process/merge.md. The source and failure evidence are WIP;
no extra repair round, unit acceptance or failure waiver is implied.

Post-check review identifies P1 at SyncCoordinator.swift:643-650. Core rejects
an old statusChanged command at validate.backend-guard; generation and binding
still match, so the Runtime token is the changed guard component. That command
does no write, but its rejection becomes a persistent UI operation failure.
The exact token-changing event and Resume's second action are not yet known.
The next limited cycle needs fixed action/guard-stage labels, a held-status
test and a stale mutating-command control. An old status result must publish
neither its UI data nor its failure; mutating errors must remain errors.

Remaining coverage: the suspended beginIdentityRead return boundary; direct
builder refusal while inference is held; a later collection-only C2 after
isolation; and real Core A7 facts through Copy Debug Info. The resolver test
does not prove the earlier begin-return interval. Signed SDK and rendered app
evidence are also outstanding. This component does not close those clauses.

The earlier full gate was interrupted after a known export assertion failure;
it is not a passing receipt. Its export repair has separate focused evidence.
The original batch base above remains in force. No Unit trailer, P24 closure,
milestone, signed-device or release acceptance is granted by this component.
No VM job ran while Antoine was resizing it.
