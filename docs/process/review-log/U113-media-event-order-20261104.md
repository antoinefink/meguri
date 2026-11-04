# Media preparation and SDK events

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: 619856bb.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Native97307 retains the media wake failure seen earlier in Native92121.
Later passes do not establish those failures' cause. Source inspection finds
that media can prepare after sent-handler return, then reach final validation
while the original SDK's didSend event is active. The lifetime guard returns
CancellationError before SDK entry.

Export8298 uses existing gates to force that order and adds four DEBUG Boolean
observations. Independent review clears the actor ordering and cleanup. The
observer must stay before any future event wait. Saved, timing, call, owner and
finalization assertions are unchanged; all held callbacks are released before
joining. Its private check passes.

Native12067 compiles and fails those Saved assertions in1.57s. At the controlled
boundary the task is not cancelled, eventActive is true, generation matches and
the SDK is live. This proves a current event-contention failure, not the exact
earlier interleaving. The failed receipt and filtered witness remain retained.

The existing P2/P4/P13.1/J2.1.1 contract permits event waiting during preparation,
before a contiguous final validation and invocation on the retained SDK. No
retry, timer, second wake or relaxed guard is required. A filtered repair follows.
The separate Core14413 final-validation responsiveness failure stays open.
