# RF1

Starting commit: `5d6517071681878d6ad7fd6f22cfa587961cf230`.

Antoine requested a faster path to a minimal review app on 2026-10-22.
`docs/process/review-first-delivery.md` records the revised execution priority.
This item connects existing production collection, scheduling and rendering APIs
through the app shell. It is a coherent part of the listed plan units; it does
not claim those units are complete or that local import already works.

The first source export is applied. RF1 is still awaiting fixes and verification.

Prepared U51 specs reused before implementation:
`f344504c` → `415775c5`, `0684fade` → `10913245`,
`dc8b780f` → `5727b42c`. Original authors, sessions and Prepared-Commit
trailers remain. The spec writer removed one optional provenance line and
rewrote the timer sentence after privacy-hook rejection; timing facts stay.
The clean prepared worktree was removed normally; its spec ref remains.

Spec-writer session `s-20261022T054036Z-254feb0f` added the bounded contract
`docs/process/review-first-contract.md`, commit `ccb933f1`. It identifies the
existing production APIs, two scheduling bridge seams and app-owned state.
No new Anki behavior or oracle scenario is needed. All source exclusions
remain mandatory for the filtered implementer and fixer.

Implementation session: `s-20261022T054653Z-58b4a14d`.
Its source base was `87f59a931dbf2ec9806e6dafc83b44140377ae17`.
The export connects the local review path described in the RF1 contract.
It still requires compilation, testing, review and rendered evidence.

The initial export deliberately refused media-bearing cards while its public
journal query was absent. Round 1 fixes below connect that production path.
Full R444 preloading/performance, platform lifecycle evidence and rendered
review remain unverified. The generated shell fixture still has four text cards;
media integration uses focused journal/resolver tests plus the existing recipes.

Integration run `20261022T062109Z-94041` failed Swift compilation:
`Review.swift` uses the iOS-only `magicTap` action on macOS.
Keep this failed receipt; the filtered fixer owns the platform correction.
Independent RF1 review round 1 covers correctness, contract and source separation.

Fix round 1, session `s-20261022T063936Z-90ddfbde`, retains RF1's starting
commit and original implementation session. The filtered runner made these
source changes without builds, tests, commits, branches or worktrees:

- Restricted magic tap to iOS; named accessibility actions and the native
  toolbar remain available on Mac. The failed focused receipt above is retained.
- Review keyboard admission now samples the current page's native responder,
  window/application activity and iOS keyboard focus, then DOM editing and IME
  composition. Close/Back and lost/idle-page focus cannot enter review shortcuts.
  Native focus is transferred only during a card presentation that actually
  owns it. The coordinator remains the sole synchronous answer gate.
- Inactive scenes, obscuring presentations, app resignation, hidden/occluded or
  non-key Mac review windows and detached review hosts pause input/audio/time.
  Inactivity pauses/persists without durable collection suspension; background
  still orders pause/persist, flush, suspend. Resume keeps input paused through
  core resume and authoritative head refresh, and rejects an intervening pause.
- Added a live, non-pending journal name/case-key query under one journal guard;
  no scan, hashing or Swift reconstruction. The admitted engine reader binds
  its generation and root, rejecting suspended, closed and replaced handles.
  Added a Result/callback-entry FFI export, hidden-binding Swift DTOs, an
  InteractiveLane call and the exact instrumentation registry entry.
- Connected collection media attachment and updates for media change sets,
  media arrivals (including immediate adds), generation replacement and resume.
  Old resolvers close before reattachment; stale generation/root responses and
  query failures fail safely. Existing pinned-root/O_NOFOLLOW lookup remains.
- Rewrote both flagged own source lines in ReviewCollection and ReviewPreviews;
  did not inspect private tokens or change any scanning policy.

Added focused source tests: journal spelling/key byte preservation, live/pending
filtering and no filesystem work; engine binding/inactive-handle rejection;
FFI arrival and Swift DTO conversion; resolver arrival/update/failure binding;
native Close/Back responder ownership; coordinator elapsed exclusion and
resume-before-input ordering. These tests have NOT run in the filtered runner.
One final source/API/ownership consistency pass was made, with obvious source
corrections applied. No success or unit completion is claimed.

Integrator commands (Apple jobs sequential; all UI/simulator jobs in the VM):

```sh
tools/mg core-format
tools/mg core-test -- --test media_journal -E 'package(meguri-storage)'
tools/mg core-test -- --test media_ops -E 'package(meguri-engine)'
tools/mg core-test -- --lib -E 'package(meguri-ffi) & test(media_arrival)'
tools/mg swift-test --package MeguriCore -- --filter 'MediaNameSnapshotTests|IntervalNamesTests'
tools/mg swift-test --package MeguriKit -- --filter 'ReviewCoordinatorTests|ReviewNativeFocusTests|ReviewMediaBindingTests|ReviewDayMonitorTests'
tools/mg fixtures local-review --seed 1
tools/mg ui-test --platform mac --suite LocalReviewJourneyTests
tools/mg ui-test --platform iphone --suite LocalReviewJourneyTests
tools/mg ui-test --platform ipad --suite LocalReviewJourneyTests
tools/mg gate --base 5d6517071681878d6ad7fd6f22cfa587961cf230
```

Integration must regenerate bindings, compile both Apple platform paths and run
privacy/provenance/similarity checks and the full Swift/FFI boundary gate. Native
keyboard activation (Close/Back Space/Return, Full Keyboard Access on iPad),
rendered visual quality, media-bearing production cards, lifecycle/failure and
restart evidence still need actual app/VM execution. These are pending evidence,
not passes. Safe local collection entry and all remaining full-plan work remain.

Validation after export 1:

| Job | Run | Result |
|---|---|---|
| core-format | 20261022T065513Z-13408 | Pass |
| core-test: storage and engine media | 20261022T065702Z-16354 | 20 passed |
| core-test: FFI media arrival | 20261022T065728Z-16805 | 1 passed; 40 excluded by selector |
| swift-test: review | 20261022T065550Z-14406 | Compile failed: actor isolation |

Review 2: P1 window test placement; P2 first-card focus.
Fix session: `s-20261022T065818Z-b2ccc5d3`.
Export: `20261022T065828Z-18087`.
The export moves window checks into the VM suite and adds initial focus.
It also changes callback initialization and rewrites the reported source lines.

Swift validation `20261022T070400Z-21748` failed: `_didFlushLocally` lacks
an initial value. Tests did not run. Preserve all three failed compile receipts.
The commit hook still rejects new Swift lines. The next filtered pass owns them.

Committed RF1 components: Rust media `9abfea4c`; Swift bridge `704eba7d`.
These are WIP. No Unit acceptance. Both commits are pushed to origin master.
Native journeys, renders and the full gate remain pending.

Further validation:

| Run | Result |
|---|---|
| swift-test 20261022T070738Z-24755 | 8 bridge tests passed |
| fixtures 20261022T070859Z-27593 | Four-card collection generated |
| swift-test 20261022T070856Z-26846 | Mac notification capture failed compilation |
| build-app 20261022T070956Z-28534 | Deprecated iOS focus calls failed compilation |
| swift-test 20261022T071332Z-32246 | Mutating test assertions failed compilation |
| drive 20261022T071421Z-33396 | iPhone build passed; collection open failed |
| swift-test 20261022T071717Z-37608 | Three static helper calls failed compilation |

Platform fixes: `c50f8693`. Assertion fix: `5ee8ecb6`.
The iPhone capture shows the error state. It proves no successful card display.

Review 3, P0/P1 scope: two P1 findings. App startup omits the backups directory
required by the core; prepare that directory before open. Three test helper
calls need `Self.head`. Keep a fresh-install regression through the real core.
Keyboard fixes passed source review; real keyboard and visual evidence remain.

Fix round 3 retains RF1 base `5d6517071681878d6ad7fd6f22cfa587961cf230`.
The filtered fixer prepares the resolved sibling Backups directory off the main
actor after the existing collection-file check and before real startOpen.
Filesystem failures still reach the owner's failed state; no collection is
created or replaced, and core path canonicalization/separation checks remain.
All three instance test calls now use `Self.head`; assertions and fixture
initialization are preserved, including the prior mutating-assertion fix.

Added headless `ReviewCollectionStartupTests`: copy the public synthetic
schema-18 fixture through an untouched export into a private temporary
Application Support/Collection, leave Backups absent, and call the production
owner startup. It requires real core readiness, deck reads and Backups creation,
then owner flush/close and noCollection; error cleanup also attempts core
flush/close. Source fixture and export bytes must remain intact. No fixture
globals, private files, fake open or UI are used. The suite needs the real Mac
core bindings/XCFramework supplied by swift-test; these are absent in this
filtered runner, so no initialization or runtime success is claimed.

One targeted source consistency pass completed. No builds/tests, commits,
branches or worktrees were run/created. Integrator verification, sequentially:

```sh
tools/mg swift-test --package MeguriKit -- --filter 'ReviewCollectionStartupTests|ReviewCoordinatorTests|ReviewNativeFocusTests|ReviewMediaBindingTests|ReviewDayMonitorTests'
tools/mg ui-test --platform iphone --suite LocalReviewJourneyTests
```

Keep all failed receipts above. Actual iPhone Home/card visuals, native keyboard
proof and required boundary/privacy/provenance/similarity gates remain pending.
No Unit acceptance; minimal safe collection entry and the full plan remain.


Review 4: P1 accessibility identifier propagation.
The captured iPhone question is legible. Required control identifiers are absent.
The patch contains the review accessibility group and scopes Home row counts.
It retains the detail counts identifier used after restart.
Mac build `20261022T073633Z-54476` requires an explicit CardRendering import.
Fix session: `s-20261022T073806Z-9aa3e97b`.
Focused suite `20261022T073556Z-52769`: 18 passed.
Drive `20261022T073306Z-46810`: real question rendered; identifier checks failed.
Keep these receipts. Answer, undo, restart and other platforms still need proof.
Root aliases also need a separate portability check. No Unit acceptance.
