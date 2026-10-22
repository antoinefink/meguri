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


Added LocalReviewAccessibilityTests for real Home, deck, question and answer.
Use tools/mg a11y with this suite on each platform. All audit variants apply.
The suite uses the existing system audit and target helpers. Results are pending.
The integrator removed two optional comments after the privacy hook rejected them.
Swift test statements are unchanged. No Unit acceptance.


Review 5: separate waiting and exhausted session checks.
Failure retained: UI test `20261022T074304Z-63305`.
The new tests require waiting after four ratings plus undo, and Done after
four Easy answers on a fresh collection. Both verify records and counts after
an unseeded restart. Shared Mac inputs use the existing activation helper.
Fix session: `s-20261022T075202Z-cdf77908`. Execution remains pending.

Sidebar integration fix after review 5, filtered source base `a294c4eb`;
original RF1 base remains `5d651707`. Replaced only HomeSidebarDecks' outline
with native List rows, sorted by full name and ID among siblings and flattened
parent before child. Every returned deck keeps its full verbatim path and
SidebarItem.deck tag. Each row has one named accessibility element and deck ID;
the decorative SF Symbol is hidden and child elements are ignored. The existing
List selection owner and HomeDeckTree are unchanged. No disclosure controls are
added; full U152 collapsible sidebar work remains unfinished.

One source/selection/accessibility consistency pass only; no local builds,
tests or commits. iPad `20261022T074942Z-71349` and Mac
`20261022T075302Z-76344` completed all 18 real review actions, but their sidebar
hierarchy checks failed. The integrator must verify the replacement through
real iPad/Mac sidebar selection, full review, system audits and required gates.
The running iPad system audit and waiting/exhaustion/restart tests have no
claimed result here. No Unit or full-gate acceptance; safe collection entry
and all remaining full-plan units remain required.

Validation repair after review 5; filtered source base
`1e4236b0a50d60b4a026240cbf22edede5f72321`; original RF1 base remains
`5d651707`. Actual iPad system audit `20261022T075553Z-79417` failed on
unsuppressed header hit regions, progress hit region and dark-default action
contrast. Earlier sidebar contrast/clipping findings also remain recorded.
No audit policy, tests or suppressions were changed.

Close, Back and Redo keep native actions and focus.
Their labels fill a 44-point target. Each has one accessibility element.
The actual progress layout has a minimum 44-point height and one accessible
element with its remaining value, Session Progress label and
`review.session.progress` identifier. Study Now and Show Answer use the existing
bordered/large/textPrimary pairing, headline emphasis and growing text labels.
Flattened sidebar rows explicitly wrap/grow and use textPrimary, preserving
full-path labels, unique deck identifiers and native List selection tags.
HomeDeckTree and review responder/lifecycle/card focus code are unchanged.

Source review completed. Runtime checks remain required.
iPhone `20261022T080215Z-85434`: 2 passed.
Both tests verify records after restart.
Image-card drive `20261022T080537Z-89843` passed all 10 steps.
The captured answer includes the generated image and four rating controls.
Keep failed audit `20261022T075553Z-79417`; the repair needs a new audit.
No Unit acceptance. Safe collection entry is next; the full plan remains.

Third bounded layout pass; filtered source base `fce53fcb` (original RF1 base
`5d651707`). Actual iPad audit 2 `20261022T080826Z-95510`, after `98c105e6`,
passed all touch-target checks and the Close/Back and Study/Show Answer contrast
checks. Remaining failures: progress AX bounds stayed 896×4 in all four variants;
largest-text question audits still saw hidden rating strings `<10m`, `Again`,
`1.4mo` and contrast failures; sidebar labels were leaf names, included an
unlabelled child, and failed contrast against dark material.

Progress now has a visible title, remaining text and bar.
Its combined element retains the current value and identifier.
Question-side ratings use .hidden() with the same reserved layout.
Sidebar paths join ancestor names in core order. Rows use the surface token.
Native selection and main Home hierarchy remain.

Runtime checks, critic, final review and the full gate remain required.
Mac `20261022T081123Z-99202`: two persistence tests passed.
The keyboard test failed: the raw AX helper could not find Back.
Retain this failure; native keyboard proof remains open.
No Unit acceptance. Full U152 and the remaining plan stay open.

Native AX helper fix: session `s-20261022T081539Z-a4262b3f`.
It checks trust and searches public window roots by exact identifier.
The walk has identity and depth bounds. Failures report AX error codes.
Keyboard activation and native focus assertions remain required.
Mac failure `20261022T081123Z-99202` is retained. New verification is pending.
iPad audit `20261022T081605Z-5320`: all four screens pass all variants.
Round 6: P1 Mac card content is absent from accessibility.
The visible question has no text descendant in the native tree.
Fix the host boundary and verify actual question and answer text.
Other P0/P1 findings: none. No Unit acceptance; the full gate is pending.

Round 6 host correction; filtered starting commit
`62570d0c0ea016df1780035efca804edb5573dde`; original RF1 base `5d651707`.
Mac now hosts the existing pair.container directly in NSViewControllerRepresentable,
matching the behavior route. The reveal action and card identifier belong to a
SwiftUI containing group. Appearance supplies the window attachment callback;
window-update observation, guarded one-time focus, key monitor and cleanup remain.
iOS retains its custom action and magicTap. Shared pair wiring is unchanged.
Mac readiness queries use app.staticTexts, preserving the real Question/Answer
number patterns and hittability checks; iOS keeps its WebView query.

One source consistency pass only; no builds, tests or commits in this runner.
Retain failed Mac drive `20261022T075302Z-76344` and audit
`20261022T081410Z-2897`. Actual Mac DOM audit, native keyboard checks, iPhone
audit and full gate remain with the integrator. No runtime result or Unit
acceptance is claimed; full U152 and the remaining plan stay open.

Final review 6 validation fix; filtered starting commit `44d41ead61e4c8fbe307d8eaa7f89f589d069a7e`;
original RF1 base remains `5d651707`. Retain iPhone audit
`20261022T082123Z-11802`: Home/deck passed all variants; review question/answer
failed native progress-title and rating-text clipping. Review.swift now uses
two equal-width rating rows (Again/Hard, Good/Easy) at accessibility text sizes,
with four columns otherwise. Interval/name text and the native progress title
grow vertically. Existing native buttons, IDs, actions, gates, real progress,
44-point header targets and hidden question-side rating reservation remain.
Mac host fix `4d8f4fe6` is unchanged; audit `59893` has no result claimed here.

One source consistency pass only; no builds, tests or commits. Runtime behavior
is unproven. Integrator must regression-audit iPhone and iPad after this change;
prior iPad audit `20261022T081605Z-5320` passed every screen/variant. Preserve
the reported iPhone/Mac persistence passes (two each). No Unit acceptance;
full gate remains pending.

Round 6 P1 narrow isolation; filtered starting commit
`1f14ff593cf8efa83ed0ede92949ea554fda8e50`; original RF1 base `5d651707`.
Removed the Mac card-only SwiftUI wrapper and all its AX modifiers. Show Answer
now belongs to ReviewLayout's existing containing session group; only the card
identifier is set natively on the unchanged pair.container. Native children,
focus/keyboard/lifecycle code, iOS actions and adaptive rating layout remain.
Retain actual Mac failure `20261022T082500Z-16034` and production-renderer control
pass `20261022T082719Z-20345` (one test, zero skips). Public AX read
`20261022T082611Z-18318` reports API disabled (-25211), trust false; user permission
is pending. Strict real DOM/hit assertions remain unchanged. One bounded source
consistency pass only; no builds/tests/commits. iPhone audit 2 remains running;
prior iPad pass predates the phone layout change. No Unit or full-gate acceptance.

Round 6: the reviewer withdrew the production accessibility finding.
Later Mac captures contain actual static text values with empty labels.
The test now matches label or value with the same exact fixture patterns.
DOM existence, hittability and all audit assertions remain required.
Keep failed audits `20261022T081410Z-2897`, `20261022T082500Z-16034`
and `20261022T083209Z-26869`; the test queried the wrong property.
iPhone `20261022T082856Z-22245`: all screens and variants passed.
iPad `20261022T083354Z-28853`: all screens and variants passed.
Mac validation remains pending. Native AX permission is a separate user step.
No Unit acceptance. The full gate remains required.

Native controls now declare the button trait after the accessibility boundary.
Close, Back and Redo retain their actions, identifiers and hit regions.
Mac audit `20261022T083719Z-32772` found missing roles on Close and Back.
Actual DOM checks passed. A new audit must verify the role repair.
iPhone `20261022T082856Z-22245` and iPad `20261022T083354Z-28853` passed.
Native AX permission remains a user step.
Correction: snapshot run `20261022T083849Z-34554` selected zero images because
bare subject filters did not match full subject/state/cell paths. Mac's 71
passes were tests, not renders. Retain this failed receipt as a syntax/selection
error; no image coverage or tooling bug is claimed. Reviewer-confirmed globs for
both filter and record are `home.deckList/*`, `review.question/*`,
`review.answer/*`, `review.congratulations/*`. A valid-selection run is pending.

Round 6 actual validation fix; filtered starting commit `e84f6cf08b9f49271593ad50c7324a0978076d46`;
original RF1 base remains `5d651707`. Actual Mac audit
`20261022T084150Z-40457`, after `447f5c58`, confirms button roles and 44-point
targets, plus real Question/Answer DOM checks in both variants. It still reports
unsuppressed ActionMissing on Close/Back (type button 9, 44×44): the ignore
boundary lost the native press action. Close/Back/Redo now expose standard
SwiftUI default accessibility actions invoking the existing close/undo/redo
closures. Back/Redo actions guard canUndo/canRedo; disabled follows the full
accessibility modifiers to cover the semantic wrapper and native button.
Labels, IDs, help, button traits, hit geometry and native keyboard actions remain.

One bounded source consistency pass only; no builds/tests/commits. iPhone
`20261022T082856Z-22245` and iPad `20261022T083354Z-28853` passed all four
screens and variants before this fix; the shared press bindings need runtime
verification. New Mac action proof and the full gate remain pending. Raw native
AX -25211/user permission is independently open. No Unit acceptance.

Snapshot planner fix: session `s-20261022T085214Z-0964537f`.
Run `20261022T084430Z-44116` recorded 140 cells; small-target plan output was absent.
The small target has no selected key variants. Existing PNGs: `d3288846`.
Plan initialization now creates an empty file and preserves existing rows.
IO errors remain failures. A wholly empty selection fails the job.
Tests cover empty plans, append preservation, IO errors, unknown selection,
deferred cells and a queued VM. Focused execution remains pending.
Mac audit `20261022T084951Z-52240` passed all four screens and both variants.
Small-phone drive `20261022T085341Z-56302`: 18 actions passed; 29 issues.
Its largest-text capture shows overlapping rating labels and a nested button.
That actual layout finding remains open. Native AX permission is separate.
No Unit acceptance. The full gate remains pending.

Viewport repair: session `s-20261022T090340Z-dd61f004`.
Drive `20261022T090043Z-64494` passed 18 actions with zero hierarchy issues.
Its capture still showed too little card space. Keep that P1 open until verified.
At accessibility sizes, progress now follows the control row.
Four full-width rating rows retain intervals, names and native actions.
Question and answer still reserve the same control height.
The queued-VM fixture now sets its required reason before calling the helper.
Selftest `20261022T090050Z-65301`: 13 passed, one failed on that fixture.
Native planner run `20261022T090420Z-69184` passed.
Existing review baselines need new captures. All audits must cover the new UI.
No Unit acceptance. Native AX permission and the full gate remain open.

Actual viewport fix verified: small-phone drive `20261022T090714Z-72607`.
All 18 actions passed, zero hierarchy issues. The reviewer inspected question
and answer captures: readable card content and separate rating rows. P1 closed.
Focused Docker planner checks `20261022T090719Z-73422`: 14 passed.
Final real-screen audits passed every screen and variant:
Mac `20261022T090951Z-76478`, iPhone `20261022T091101Z-78124`,
iPad `20261022T091337Z-80854`.
Current source: `b8340cfb`. New baseline capture, critic and full gate remain.
Native AX focus permission is a separate open user step. No Unit acceptance.

Final matrix `20261022T091701Z-83713` passed all 140 selected cells:
56 iPhone, 56 iPad and 28 Mac. Updated images are committed in `d8202f38`.
The small target has no selected key variants; its actual largest-text review
is covered by drive `20261022T090714Z-72607`, not a claimed matrix render.
The independent critic is checking these captures and the final audit receipts.
Antoine authorized VM permission setup directly, then confirmed ongoing
permission to modify this dedicated VM. Host settings remain out of scope.
The strict native AX focus case still requires an actual passing execution.
No Unit acceptance or full-gate pass is claimed here.

Critic `20261022T092442Z-93102` reported 22 high findings.
Independent review reduced four notice findings to one confirmed defect.
Eight overflow findings concern a ScrollView; still images do not prove it
cannot scroll. Four Home findings omit the actual shell shown in drive evidence.
Six completion findings come from bare preview content. Their transparent Mac
images were 336 pixels high. Fix the fixture rather than infer a window defect.
The generated ledger update failed privacy checks on its own prose. Its report
and receipt remain intact; only that uncommitted ledger update was restored.
No critic pass or design approval is claimed.

Filtered fix `a6cacbb3`, session `s-20261022T093301Z-1df0122e`, preserves notice
height and uses the full review layout for completion fixtures. The reviewer
found no remaining P0/P1 in that patch. Rendered closure remains pending.
VM helper build `20261022T093210Z-4572` failed on two SDK/compiler issues.
The fix uses tableRows and a preconcurrency framework import.
Run `20261022T093549Z-13025` built, but executed zero cases due to plan exclusion.
Its nominal pass does not prove setup. Selection and result validation need fixes.

Follow-up result validation rejects zero passing tests, invalid counts and failed
outcomes. Focused Docker checks passed: three new cases in
`20261022T094042Z-21303`, then all 27 selected UI dispatch/result cases in
`20261022T095029Z-34684`.

VM setup attempts remain failed evidence, not application failures or grants:
`20261022T094042Z-21488` could not identify the runner row;
`20261022T094710Z-29439` raced a broad indexed row query;
`20261022T094956Z-33013` reached the OS administrator dialog.
Native exact-name queries replaced the indexed search. Antoine explicitly
authorized entering the dedicated VM credential and modifying this VM.
`20261022T101411Z-69285` authenticated successfully, but the helper failed to
recognize the application picker. Its controls use native identifiers and empty
labels. `d4bdfb7b` corrects those identifiers. A real public-AX trust check and
strict keyboard execution remain required; no credential belongs in this log.

The initial notice expansion exposed a second viewport problem at the largest
text size. Filtered fix `8a99a47d`, session `s-20261022T100053Z-8c899035`,
caps the scrolling notice at 40 percent of the middle region and reserves the
remaining space for the card. Header and rating controls remain separate.
Review matrix `20261022T101801Z-73748` passed all 90 selected cells. Only the
four expected phone error images changed; the other 86 were identical.
Images are committed in `093a7ced`. Independent visual review confirmed readable
cards, separate ratings and stable question/answer control space. The visual P1
is closed; actual scrolling of a long notice still needs runtime proof.

Completion fixtures now render the full opaque review layout, waiting/finished
content and actions. Their 20 updated images are committed in `f8383519`.
The earlier focused render `20261022T095115Z-36875` covered 40 error/completion
cells, not 90. Four intermediate phone error captures were superseded by the
bounded notice fix; their original evidence is retained under ignored build data.

Real-screen accessibility suites passed every selected screen and variant:
Mac `20261022T095435Z-46345`, iPhone `20261022T095547Z-49079`,
iPad `20261022T095836Z-53073`. These precede the bounded middle-region change;
they are not claimed as matching the newest source digest. The RF1 full gate
against `5d6517071681878d6ad7fd6f22cfa587961cf230` remains pending.
No Unit acceptance or final design approval is claimed.

VM setup `20261022T102547Z-84593` passed one selected test with no skips,
including actual public accessibility trust. Temporary VM setup files were
removed immediately afterwards. Strict keyboard run `20261022T102715Z-86277`
then failed: trust was true and the focus write succeeded, but the immediate
AXFocused read returned noValue (-25212). Initial reveal/rating shortcuts passed
before that assertion. Native control focus and activation remain open; the
reviewer is checking this bounded failure before any source change.

Filtered test-only fix `b2f1dc2e`, session `s-20261022T102909Z-9c19149a`,
requires the native button role and waits for positive public focus evidence
after one focus write. Missing AX values alone cannot pass. Independent bounded
review found no P0/P1. Run `20261022T103203Z-94663` stopped earlier with
trust false: the rebuilt runner no longer matched its previous permission.
Setup `20261022T103333Z-96887` retained a stale enabled row without actual trust.
The integrator reset Accessibility for the exact test-runner bundle in the VM,
not the whole service. The VM's Keyboard Navigation setting changed from off
to on. These are authorized dedicated-VM changes, not host changes.

Setup `20261022T103448Z-98831` reached the path picker but failed its label-based
guard. The masked hierarchy identifies the native GoToWindow sheet and sole
PathTextField. Filtered fix `96626a3b` uses those exact identifiers, retaining
path, secure-field and authorization guards. Independent review found no P0/P1.
Fresh setup `20261022T103809Z-3051` passed its actual trust test with zero skips.
All temporary authorization files were removed after each attempt.

Native run `20261022T103938Z-5320` established positive native button focus and
completed the Space iteration, including Back, retained focus and Close.
It then stalled at the Return-to-undo expectation for over three minutes;
the integrator sent SIGTERM. Its receipt is cancelled/incomplete, not passing.
Independent review confirmed that expectation exceeds the platform contract:
Space activates a focused standard button; Return activates the default button.
RF1 does not require custom Return activation of focused Back/Close.
The bounded test repair will retain Space activation and verify that Return
preserves exact native ownership and review state. No product key remapping.

Latest smallest-phone execution `20261022T102842Z-88520` passed all 18 actions
at the largest text size with no hierarchy issues. This covers the bounded
notice source revision's normal review path; it does not claim long-notice
scrolling or substitute for the pending full gate.

Filtered native-test correction `de8f2863` follows standard Mac key semantics.
Independent review found no P0/P1. Setup `20261022T104825Z-10973` passed.
Native run `20261022T105001Z-13429` then failed a new intermediate test query:
the native progress wrapper did not expose a String value. It had already
established positive native focus and meaningful current-card text.
The reviewer approved removing that redundant representation assumption,
retaining exact card/side continuity, redo state, native activation and final
authoritative saved-answer/deck counts. Filtered fix `cc07f151` does only that;
its independent bounded review found no P0/P1. No product behavior changed.

Consent check `20261022T105326Z-17378` passed with public AX trust.
Its opt-in files are gone. Test `20261022T105500Z-19800` passed:
one case, zero skipped. Initial reveal and rating keys work. Back and Close
retain native focus after Return, with card and side unchanged. Space invokes
undo and dismissal. The final record has one answer and the deck has three due
cards. The keyboard finding is closed. No product key handling was added.
Fixer session: `s-20261022T102909Z-9c19149a`. Full gate pending; no Unit claim.
