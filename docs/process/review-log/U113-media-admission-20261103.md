# U113 media admission

Status: WIP; functional regression is red. No unit acceptance.
Unit start:1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start:fa8051b1.
Batch base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer:s-20261103T133750Z-f276f58e.

## Review and witness

The source audit finds a possible quiet exit: media selects an upload, send
admission returns none while another original handler remains active, and the
same queued name then ends the media round. Export39754 makes no changes: the
existing seams cannot prove that admission attempt.

Export44092 adds a DEBUG Boolean observation after actual Core send admission,
and a two-file media test. Independent review finds one P2: requiring two
rejections before releasing the handler excludes a valid wait after the first.
Export47897 requires one rejection and retains bounded Boolean evidence. The
other assertions, ten-second limit, original callbacks and cleanup joins stay.
Independent recheck clears the source.

Swift54495 does not finish cleanup. A bounded sample of its own test process
finds a Query reader held by the smoke hook while it opens the media service,
which borrows Query again. The test worker is stopped; MG reports a failed exit
with zero completed tests. This is not a media scheduling assertion result.

Export56231 opens the service before retaining Query, with the same basename
checks, snapshot and observation fields. The Swift seed is unchanged. Independent
review clears the repair. Format57527, Lint59813, Private60077 and
Similarity60398 pass.

Swift57998 then completes in 1.611 seconds and fails both final Saved checks.
Both actual admissions are false before handler release. The original handler
is released and joined; no fresh policy wake is supplied. Guard, ownership,
SDK count, deadline, cleanup and error/deferred checks pass. The media round
returns without an error, but neither generated file reaches Saved.

This supplies the intended scheduling regression. A production repair and
passing focused checks remain required. No failed run is reclassified. The
full batch gate and live cross-device round trip remain open.

## Repair evidence

Export61288 waits on the original sent handler and rechecks admission.
Independent review finds a P2 cancellation cleanup gap after successful
admission. Export68365 finalizes that exact call before propagating the error.
Independent recheck finds no new P0-P2; the original witness stays unchanged.

Swift74246: four tests pass, but a compiler warning fails the MG run.
Export76113 fixes that warning and rewrites one comment. No assertions change.
Private scan passes. A separate inherited cleanup gap remains recorded in
build/notes/media-post-admission-cleanup-followup-20261103.md.

Swift77452: all four tests pass; MG still fails the Bool-optional macro warning.
Export76113 does not resolve that diagnostic. Both failed runs remain retained.

Export79429 uses an explicit unwrap; independent review clears it.
Swift80509 passes all four tests with no compiler warning. Prior failures stay
failed. The inherited cleanup follow-up and full batch gate remain open.

## Cleanup evidence

Export82283, review clear. Swift85502: one test fails two assertions.
No runtime changes.

Export88798: common finalizer; Swift89635 passes eight tests. Review finds one
P2: cleanup failure with no original error must propagate. Export91368 applies
that correction; independent review clears it. Swift92121 then passes seven
of eight tests. The original wake test fails three assertions; its cause is
not established. No unchanged rerun. Safe failure evidence is needed next.

Export94446 adds bounded failure tags and call counts to the original test.
Independent review clears it; assertions and limits remain unchanged.
Swift96587 passes the diagnostic pair. Swift97781 passes all eight selected
tests. These passes do not establish the cause of Swift92121. Retain its
failure and the diagnostics; do not repeat unchanged checks to seek a pass.

Signed Mac Debug build95028 passes at4e6f1024, with zero build warnings.
A real review starts, reveals, records one rating and advances. Normal quit
and reopen retain the reduced review count and one recorded answer. Media
remains pending; no two-device convergence or unit acceptance is claimed.
