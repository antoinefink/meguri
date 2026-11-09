# Original asset-read lifecycle jobs

Actual clean master starting commit supplied by root: `21c5e02e`.
Original batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer session: `s-20261109T094715Z-c7c7c45e`.
Filtered snapshot HEAD: `6a5ae6db13d54dc156818a5a70bc14078d438c50`.
Contract: `engine/jobs#J2.1`, `#J2.1.1`, `engine/lifecycle#L3`, `#L8`,
`#L11`, `engine/suspend#S1`, `#S4`, and all mandatory rows in
[the resource amendment](cloud-asset-resource-contract-20261109.md).
Anki: none. No P24, unit, batch or milestone acceptance is claimed.

This source handoff reuses `91116331`'s same-reader pin and required cell,
`bf1aa6a2`'s original-resource registration/liveness/cutoff, and `21c5e02e`'s
ordinary next conversion. It does not modify the parallel live-resume files,
suspend specifications, CollectionReview, SyncCoordinator, CloudKit wire or
merge/recovery rules.

## Changes

The engine pool runs typed submission with the retained pass, selected request
and native queue. Its worker prepares the reader, file and row. After Enqueueing,
the reader stays pinned until Runtime has processed rejection or recorded
acceptance, even on error. Retry admission checks committed policy and SDK facts;
first-read admission checks its original SDK and collection. The last cancellation
check still applies after the effect latch. A rejected request completes Runtime
cleanup before returning its sampled error. Accepted work keeps its original
Started event and accounting owner.

Complete/cancel/finish use the original-resource selector and the existing
required result cell. Complete checks the latched resource authority again at
actual mutation boundaries, and retries also check committed original command
authority after waits. DisposalOnly retires only original memory and returns the
specified `The result cannot be applied.` error for complete. Synchronous
compatibility exports remain. Finish's typed Boolean is a completion marker;
Swift's public finish still returns Void, and absence of the marker is not a
cleanup result.

Swift routes submit and resources through the existing ordered start/drain/read
helpers. Submit tracking failure drains the actual job before receipt inspection.
Required results outrank setup failures. The existing result lease spans awaited
Core completion. Started uses ordinary policy work; the original raw-error/Ended
sequence uses required original-SDK completion. Existing adapter producer and
terminal ordering remains intact. Instrumentation includes all ten new production
start/result names and its exact expectation.

Test hooks observe the real Cloud slot and actual transferred-reader waits,
submit preparation/accounting, and actual resource pool bodies. The native body
barrier reuses the existing gate; no thread, reservation, slot or CoreEvent kind
is introduced. New bounded Rust witnesses reserve an idle nextest runner.

## Authored witnesses, not verification receipts

- Actual queued cancelled complete/cancel/finish bodies, exact results, one body
  and one terminal event (including a terminal fence).
- Required body panic and retained absent cell; separate original-selector
  normal Cancelled and absent-return cells.
- Cancelled admission, rejected SDK authority and an emptied Cloud reader all
  stop after Enqueueing. Each case checks that the native queue was untouched,
  the retry count and grant stayed intact, and Runtime removed the request.
- Actual callback entry while Driver and Runtime projections remain held; real
  reader-slot try_lock and a second Cloud reader observed waiting.
- Real replacement held after callback return until accounting releases the pin,
  for both true and false queues; real close joins accepted accounting without
  replacing its true typed result with cancellation.
- Ordinary queued submit cancellation and accepted accounting failure with the
  original owner/accounting fence retained through required completion.
- Registered resource body close drain; original liveness loss during an actual
  Runtime wait; paused resource mode retained through actual resume.
- Late close/unpause/reopen cleanup and actual successor isolation, including
  same-Runtime successor SDK, policy changes, duplicates, cancel and finish.
- Queued cancelled original raw-error/Ended work after submit/complete/finish,
  one failed activity cycle, exact raw-error order and late paused refusal.

`CloudAssetReadLifecycleJobTests.swift` adds the accepted-but-callback-not-returned
tracking-failure witness and parameterized real Core complete/finish body
barriers. They retain the original terminal, assert concurrent close is still
joined, and check the source lease through awaited complete. The existing
line-166 false-queue regression and retained tests/assertions remain. The older
cancel-after-next fixture now records/cancels only its first admission, retaining
that test's original handle as subsequent cleanup gains jobs.

## Outstanding mandatory proof and integration

No compilation, build, test, native run, dependency access, independent review,
privacy or similarity job was run in this filtered session. Root owns all of
those, including formatting and the original coherent batch gate. The only
consistency check is the final scoped `git diff --check` (passed).

The new source witnesses above are unexecuted. Additional resource-path evidence
is still needed for these exact mandatory cases; the prerequisite primitives and
retained synchronous tests alone do not establish them:

- Prepared original Host calls with actual retry/first capabilities, plus the
  complete late-call matrix across account changes and failed-close recovery;
  counted refusal of every provider/tracking/reader/writer/SDK access in each
  DisposalOnly case.
- Resource-specific calls on both sides of the registry cutoff, the worker busy
  tail after running-map removal, and an expired resource drain/reader deadline.
  Existing engine/Host prerequisite witnesses remain unchanged.
- Submission unwind and replacement blocking while a rejected reader remains
  retained still need coverage through Runtime's rejection return. Include
  emptied Sync readers and other reader roles; retain the existing primitive
  checks.
- Exact policy-stale-versus-Runtime error precedence under simultaneous final
  failure, and a post-effect complete error with its required follow-up, through
  the new whole-submit/resource entry points.
- Swift setup failure during required complete/cancel/finish (including actual
  typed Cancelled versus panic/no-body), staged-file retention through failing
  completion, and the full adapter activity matrix: empty pass, app cancellation,
  activity authority refusal and producer-before-pause. Retained tests must run
  against this conversion; no new acceptance is inferred from their old receipts.

These are proof gaps, not spec changes. No contract gap was resolved by inventing
semantics. All old retained retry/grant tests, assertions and timing limits are
preserved and must be included in root's verification.

## Bounded fixer handoff

Fixer session: `s-20261103T133750Z-f276f58e`.
Root supplied repair base `509662ed`, original source `21c5e02e`, and the
unchanged batch base above. This filtered snapshot reports HEAD
`d3359df36b7e4e9afae5a0025548ff188567ec35`; the prepared seed was already present.

Root reports combined lint `87339` stopped at three new test compiler errors.
The replacement worker and its swap callback now own their captured values.
Both adapter snapshot comparisons still compare the complete values, using
Boolean assertions so production Snapshot needs no Debug implementation.
The reported new comment, identifier and prose regions were rewritten without
reading guard patterns or private inputs. Root must scan the final source.

Independent final review confirmed one P2: the activity compact hook called
ordinary Host ownership validation under inner locks. That hook now checks
immutable command ownership and issued generation, after the caller's existing
captured original liveness check, then validates committed Driver/SDK facts.
Ordinary validation and synchronous exports retain their previous checks.

One additional Rust witness gates activity after Driver projection and before
its effect. A feature-gated Host helper holds the actual close-state mutex.
Started and required Ended jobs must finish while that mutex remains held;
release, join and bounded waits remain explicit. The existing nextest filter
reserves an idle runner for this test. It has not been executed here.

Verification follows below.

## Integration evidence

R2 independent review: `build/cloud-asset-lifecycle-r2-independent-review-20261109.md`.
R3 wording review: `build/cloud-asset-lifecycle-r3-delta-review-20261109.md`.
No P0–P2 source finding remains in those reviews.

Core check `20261109T183028Z-94415`: 15 selected tests pass.
The earlier `94094` failed compilation after restoring source with old timestamps.
Refreshing the changed Rust source timestamps forced the correct dependency rebuild;
no assertion, limit or feature change was required.
Lint `20261109T183140Z-94837`: three checks pass; formatting the renamed test remains required.
Native checks, additional resource cases, guard scans and the original batch gate
remain pending. No unit acceptance is claimed.

Native CoreBridge `20261109T183311Z-96248` passes: 30 selected Swift tests;
37 total receipt checks, no skipped or failed checks. This includes all five
new resource-failure cases. Their independent review is recorded in
`build/cloud-resource-swift-failure-review-20261109.md`.
Similarity `20261109T183523Z-97982` passes 20 files, with one excluded path
and no exceptions. Privacy `94817` passes source files; its two prose findings
were removed, and log check `20261109T183214Z-95431` passes. The new Swift
failure file passes its separate guard check.
AppModel `20261109T183616Z-98370` stops at a test helper's file-only access;
no AppModel execution is claimed. That test-only repair and the broader resource
matrix remain pending, together with the original batch gate.

Final focused lint `20261109T183738Z-99886` passes all four checks.
