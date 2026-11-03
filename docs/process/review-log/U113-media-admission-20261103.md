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
