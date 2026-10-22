# RF1: local review integration contract

Anki: none. This is Meguri app integration over existing core contracts.
Starting commit: `5d6517071681878d6ad7fd6f22cfa587961cf230`.
Authority: `review-first-delivery.md`; existing plan units remain incomplete.

## RF1.1. Scope and ownership

Connect the local path from collection open through answering and safe close.
Use the existing scheduler, session, renderer and storage; Swift owns no scheduling rules.
RF1 covers parts of U51, U151–U153, U175, U176, U178 and U179.
CloudKit, editing, statistics and advanced settings are outside this delivery.
Missing providers have explicit unavailable states; they never report success.
Generated `-MeguriFixture` collections prove integration only in hook builds.
Safe user collection entry is the next required MVP item: preserve the source,
stage and verify the input through the planned package/replace primitives.
RF1 must not offer raw database replacement or claim that import is delivered.

## RF1.2. Collection admission and Home

Requirements: R14, R15, R28–R40, R45, R597, R702–R704, R324.
One collection owner lives above layout changes and mirrors core lifecycle/generation.
Open only the resolved app-private collection location through the real open job.
Subscribe once to ordered core events and fan out changes; retain commit failures.
Opening, unavailable, failed, empty and ready states have distinct honest displays.
Apply U51's `sched/app-coordination` before study: day check and FSRS admission,
including the first/re-enabled sheet and awaited enable job where required.
Unsupported presets block study with the R704 explanation; no substitute scheduler.
Home displays the returned deck tree, hierarchy and New/Learn/Due counts unchanged.
Select the real deck id; start study through its core session operation.
Refresh counts on relevant changes, foreground and the supplied next-change instant.

## RF1.3. Card presentation and answering

Requirements: R325–R327, R330–R332, R346–R360, R414, R443–R451.
One session owner above the shell layout owns two persistent CardPagePair pages.
Phases: loading, question, answer, transition, waiting/finished, ended, or recoverable error.
Take the card, state token, render key, options and interval labels from the core head.
Render through renderSessionHead; build CardPayload from its RenderedCard.
Prepare the idle page and present it; reveal through showAnswer with no scheduler call.
Offer Show Answer, then four controls: Again, Hard, Good, Easy, with core intervals.
Touch controls meet the existing hit-target rules; all controls have accessible names.
Space/Return reveal; answer shortcuts enter the same gate and respect text-input ownership.
Reserve bottom-bar space on both sides. Keep the card and side through layout changes.
Only the presented answer card accepts a rating. Set transition before awaiting anything.
Capture input time and capped active elapsed time; exclude background/invisible time.
Send exactly one answer with that card's token. Ignore repeat input during transition.
Apply the returned head diff and prepare the next card without waiting for durable commit.
A stale/not-at-head result refreshes the authoritative question and never replays input.
Invalid states disable answer inputs until refreshed; prepared stale pages are discarded.

## RF1.4. Persistence, undo and lifecycle

Requirements: R361–R371, R440, R441, R88, R123–R127; engine/lifecycle.
Accepted and durable are separate states. Consume commit failures even after advancing.
On commit failure, show the original question and error.
Remove the failed answer from the record. Clear dependent prepared pages.
Persist session records atomically off the main thread, including undo/redo changes.
Show Back even when disabled; use session-scoped core undo status and its reason.
Undo once, await the actual restore, then render its authoritative question/head.
Finish distinguishes waiting-for-learning from exhausted/empty using core study state.
Keep the core session open while its finish presentation permits undo; end it exactly
once on dismissal, or immediately when no such presentation is shown.
Background order: pause timer/audio, persist record, flush, then suspend with deadline.
Handle Deferred explicitly. No unavailable cloud provider may prevent local flushing.
Foreground order: resume, apply its generation/day report, refresh head, then enable input.
Day/zone/significant-time events use U51's one-shot scheduling, never polling.
Close flushes/ends the real session and releases the pages; reopen reads durable state.
A process restart never resubmits a prior accepted answer merely to restore the UI.

## RF1.5. Existing API inventory and missing seams

| Purpose | Existing production API |
|---|---|
| Open/control | CoreCollectionService.startOpen; CollectionService.lifecycle; CollectionControlService.resume/flushForBackground/suspend |
| Decks | DeckService.treeWithCounts, selectDeck, studyState; CountedDeckTree.generation/nextChangeAt |
| Review | Core.startSessionJob, sessionHead, refreshSessionCard, renderSessionHead, answerSession, endSessionJob |
| Undo | CoreUndoService.peekUndo/startUndo; CoreUndoStart.restored or job; corresponding redo APIs |
| Events | CoreEventStream.start/events; lifecycle, changes, head/current changes, commitFailed, waitingUntil, finished, day events |
| Pages | CardPayload(RenderedCard); CardPagePair.load/prepare/present/showAnswer/suspend/resume/close |

Missing app attachments: collection owner/event fan-out, review coordinator/record store,
Home and review views, lifecycle hooks, and ShellFeatures.standard feature replacement.
Missing public Swift wrappers: checkRollover, fsrsPolicyStatus and enableFsrs job tracking;
the corresponding Rust FFI exports already exist. Keep generated types internal.
Resolve app-private paths and media resolver through existing location/storage contracts.
Retain attach points for later backup, widget, sync and session-summary providers.

## RF1.6. Acceptance evidence

Use a generated fixture through the real app shell: open, select deck, show real HTML,
reveal, exercise all four ratings/labels, advance, undo, finish and reopen.
Prove one input produces one answer, stale input cannot answer, and a delayed commit
allows advancement. Inject commit failure and verify recovery plus record correction.
Background/foreground and process-restart tests prove durable answers survive exactly once.
Prove FSRS refusal/enable, day rollover, empty/waiting and open failure states.
Check session/card/side continuity across compact/regular layouts and keyboard focus.
Focused Rust/Swift checks and the required integration gate remain mandatory.
Rendered UI evidence covers iPhone, iPad and Mac through tools/mg in the test VM.
VM-unavailable evidence stays queued; final acceptance requires it to pass.
