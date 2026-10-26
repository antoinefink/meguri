# U162 — Offline package proof

Supplemental start: `7b35de76`. Original integration base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

## Bounded diagnostics

Filtered exports `cleanroom/20261026T084508Z-93203` and
`cleanroom/20261026T085821Z-2515` change only the verifier and its regression.
Independent R1 and R2 are clear. Exact comparisons, bounds, collation refusal,
cleanup and successful outputs stay unchanged. Failures identify fixed schema
positions and reasons. Only mismatched keys gain UTF-8 SHA-256 fingerprints;
raw keys, values, SQL and exception text remain absent.

| Check | Receipt | Result |
|---|---|---|
| Focused Docker regression | `selftest/20261026T084917Z-96389` | Pass |
| Final regression, including key digests and redaction | `selftest/20261026T090114Z-4704` | Pass |
| Final two-file privacy scan | `private/20261026T090114Z-4936` | Pass |

## File proof remains open

Original `package-interop-check/20261026T002734Z-14327` fails before a client opens.
New runs `20261026T085131Z-98116` and `20261026T090245Z-6336` retain that failure.
The first difference is a config-table key at table 11, row 12.

Digest matches against public Meguri constants identify expected `newSpread`
and decoded `localOffset`. The generated baseline omits `localOffset`.
The native day query must add it at the pinned non-UTC clock (day-clock D5).
Fixture preparation needs correction before launch; no comparison is waived.
Further differences remain unproven. This is active offline file compatibility,
not frozen server work. No Unit trailer or full gate acceptance is claimed.

Final tooling similarity `20261026T090115Z-5133` passes for both files.

## Prepared baseline and fresh Mac proof

Start `f60a1cf9`; implementation checkpoint `8f034ed5`. F11.1 prepares a fresh
run-owned fixture before launch and preserves its generated parent. Docker checks
the single clock-config write. Native capture and package verification use this
same frozen copy; exact comparisons remain enabled.

Filtered implementation `cleanroom/20261026T092045Z-25654` and test-only fixer
`cleanroom/20261026T095059Z-54307` retain exports. Independent R1 found two test
setup defects: overlapping expected/output directories and duplicate decoding.
R2 closes both without changing production guards or assertions. Reports:
`/tmp/meguri-u162-prepared-fixture-review-r{1,2}.json`.

| Check | Receipt | Result |
|---|---|---|
| Native preparation/capture wiring | `selftest/20261026T093740Z-39201` | Pass, 14 checks |
| Original clock/host regression | `selftest/20261026T093848Z-40536` | Fail; test setup corrected above |
| Final clock/host regression | `selftest/20261026T095208Z-56016` | Pass, 2 checks |
| Six-file privacy | `private/20261026T093847Z-40340` | Pass |
| Six-file similarity | `similarity/20261026T093845Z-40040` | Pass |
| Final test privacy | `private/20261026T095232Z-56893` | Pass |
| Actual Mac native export | `ui-test/20261026T094106Z-41714` | Pass, no skips |
| Independent fresh package proof | `package-interop-check/20261026T094529Z-47013` | Pass |

The fresh package matches 12 tables, 2,425 rows and all 270 media files
(4,650,232 bytes), before client opening. Stamp edits and client-open edits are
zero; timing also matches. Historical failures above remain evidence. This closes
the bounded Mac proof failure, not U162. Mobile/native intake, remaining unit
requirements and the original-base integration gate remain outstanding. No Unit
trailer or release acceptance is claimed.

Final repaired-test similarity `similarity/20261026T095230Z-56684` passes.

## Mobile native Files selector repair

Start `51e07b93`. Retained synthetic failure `ui-test/20261026T011056Z-33684`
shows one focused native Files TextView; the test searched TextFields.
Filtered fixer `cleanroom/20261026T101505Z-87607` uses the exact observed local
root and editor identifiers, with uniqueness and existing state/actionability
checks. The modal TextField path and nonce, Cancel, Save, Replace and destination
assertions remain unchanged. Independent R1 is clear; privacy
`20261026T101851Z-91646` and similarity `20261026T101852Z-91828` pass.

Actual iPhone execution and iPad execution remain unrun for this repair.
Original-base gate `20261026T095453Z-62360` was cancelled after a separate active
review-durability failure; it is incomplete. See `C8-isolation.md`.
No mobile pass, Unit acceptance or gate acceptance is claimed.

On clean `0bc98afa`, UI lint `20261026T102715Z-96947` passes 128 checks with
zero findings; static checks `20261026T102714Z-96936` pass all three checks.
These source checks do not supply native mobile execution evidence.

## Fresh mobile results

On clean `d5a0db5a`, iPhone native Cancel/Save/Replace
`ui-test/20261026T123937Z-10988` passes one test, zero skips. Independent
`package-interop-check/20261026T124442Z-16972` passes against that run's frozen
prepared source: 12 tables, 2,425 rows, 270 media files, zero stamp edits and zero
client-open edits. Timing matches. The actual destination file is retained.

iPad `ui-test/20261026T124445Z-17228` fails one test, zero skips. New Folder was
tapped; the following naming-state query failed with an AX waiter interruption.
No native destination or package pass is claimed. Its earlier picker attachment
does not show the failed naming state. Diagnose that state before repair.

The checkpoint audit also identifies an unproved chain: media-bearing file
import, a new review, retained answers after relaunch, then re-export. Existing
native review-import tests use a collection-only package; the media-export test
launches a fixture directly. These separate passes do not establish the combined
workflow. This gap, iPad export and the original-base gate remain open.

## Bounded iPad naming-state diagnostic

Start `9b265083`. Filtered fixer `cleanroom/20261026T130424Z-38175` adds one
iPad-only generated-screen attachment before AX queries, followed by incremental
exact native-root/editor facts. Editor data is limited to two elements, 256
value characters and 2,000 description characters. Independent R1 is clear;
privacy `20261026T130829Z-41745` and similarity `20261026T130831Z-42020` pass.
All guards, interactions, assertions and ten-second limits are unchanged.
Runtime is pending. Extra observation can affect timing; a passing diagnostic
run alone must not be described as a repair.

## Native editor admission repair

Diagnostic `ui-test/20261026T131026Z-43150` still fails; it does not prove export.
Its bounded attachments prove one exact local root and one native TextView:
enabled, hittable, focused with selected text, and no software keyboard.
Alerts and sheets both count zero. The software-keyboard guard excluded this
valid native naming state. The single generated picker screen was inspected.

Start `61b5c2ab`. Filtered fixer `cleanroom/20261026T131816Z-49901` removes that
proxy guard. Exact owner, identifier, uniqueness and actionability checks remain.
Return now requires this exact editor to disappear plus the existing actual-child
or navigation proof. All actions, nonce checks and limits remain unchanged.
Independent R1 is clear; privacy `20261026T132217Z-54957` and similarity
`20261026T132217Z-54958` pass. iPad and shared iPhone execution are still pending.

## Cancellation readiness

`ui-test/20261026T132410Z-57679` fails before folder naming. In the cancellation
phase, the Save button's AX hit-point query fails. That control is not used to
cancel. The folder-editor repair therefore remains unverified by this run.

Filtered fixer `cleanroom/20261026T134059Z-73426` makes root readiness require
the actionable overflow control, the actual next cancellation action when
Cancel and Back are absent. It removes Save only from that helper's readiness
and diagnostic list. Real Cancel, Back and overflow actionability, navigation
bounds and final Cancel checks remain; real Save/Replace assertions elsewhere
are unchanged. Independent R1 is clear. Privacy `20261026T135222Z-86956` and
similarity `20261026T135219Z-86690` results are retained; native checks are pending.

## Native export proof on all three platforms

On clean `85969338`, iPad `ui-test/20261026T141305Z-4872` and iPhone
`ui-test/20261026T141724Z-9854` each pass the actual native Cancel/Save/Replace
test with zero skips. Return still requires the same editor to disappear.
Independent iPad proof `package-interop-check/20261026T141724Z-9729` and final
iPhone proof `package-interop-check/20261026T142152Z-14415` both pass against
their own frozen pre-launch source copies. Each proves 12 tables, 2,425 rows,
270 media files, matching timing, zero stamp edits and zero client-open edits.

The earlier Mac native/export proof remains retained above. This closes the
bounded three-platform export gap. Historical failed receipts remain failures.
It does not establish media-bearing file import/review/relaunch, full U162,
the original-base integration gate or milestone acceptance. P16 implementation
is now in progress; the public media-input refusal remains until verification.

## Media-chain input contract

F11.2 adds one small media-bearing native import/review/export case. It freezes
source, expected states and declared runtime inputs before launch; it validates
actual card/rating admission and supplies only the declared elapsed test input.
Real timing clocks and every deadline remain unchanged. Valid PNG and WAV data
are required; the old recipe's random audio bytes cannot prove playback.

The spec writer's patch and independent clear R1 are retained in the intake
directory. Expected Rust replay checks delivery/persistence, not independent
scheduler correctness. Existing backend/parity checks and strict package/client
comparison remain required. The export-only and collection-only cases remain
unchanged. Implementation and the complete native media chain are pending.

Privacy findings required a prose-only rewrite of six F11.2 paragraphs. The
final independent semantic review is clear; all input, media, timing and proof
requirements remain. Privacy scan `20261026T145803Z-44709` passes. No guard or
exception changed. These are test contracts, not implementation or acceptance.

## Generated media-chain source

Start `f588201c`; filtered implementer `cleanroom/20261026T151920Z-61316`
adds the internal pinned-decoder source preparation and focused tests. It keeps
one generated card and its history, adds valid PNG/WAV and unused media, and
records exact changes and frozen parent/source identities. Existing recipes,
goldens, defaults and F11.1 remain.

R1 found cleanup that could delete files without proving ownership. Filtered
fixer `20261026T153444Z-67695` adds creation/binding checks and collision tests.
R2 is clear. Privacy required another neutral comment/test rewrite, supplied by
`20261026T154401Z-71860`; final semantic review is clear. Focused NativeClock and
HostCompatibility checks pass in `selftest/20261026T154723Z-74214` with zero
skips. Privacy `20261026T154633Z-73601` passes.

This is source preparation only. Real generated-parent execution, packaging,
declared runtime inputs, Rust expected-state replay and the full native chain
remain pending. No Unit or milestone acceptance is claimed.

## Frozen Rust replay

Start `02eed10c`. The filtered replay work adds strict declared inputs,
pre-launch source/import/review states, 18 frozen artifacts and an external
freeze identity. It runs the real import, answer, end and reopen paths; the
expectation checks delivery and persistence, not independent scheduling parity.

Independent review closed the dependency and enabled positive-test findings.
The compile repair and final wording-only closure preserve the schema, bounds
and validation order. Final closure report is retained in the U162 intake.
Focused `core-test/20261026T182027Z-57470` passes 3 tests with zero skips;
privacy `20261026T182025Z-57218` and similarity `20261026T182029Z-57708` pass.
The failed results are still recorded. App execution and the batch gate
are not yet complete.

## Native input owner and public media entry

Start `02eed10c`. Filtered implementation and fixes add the guarded frozen-input
owner, actual inspection/reopen seeds, one-use answer admission, completion and
close evidence. The public FFI media refusal is removed after backend checks;
`core-test/20261026T182430Z-63753` passes 5 selected entry tests, including real
initial media activation (76 unrelated tests not selected).

Review found missing initial-reset and iOS orderly-close paths, then the wrong
empty-fixture path. Round 3 closes those findings. The final privacy check
`20261026T184128Z-83593` passes. The audio pair forwards existing backend
observations; its independent source review is clear and playback is not yet
proved by that API alone.

Swift `20261026T184130Z-83850` compiles production code but fails the new test
fixture: nine missing awaits and one ReviewHead/ReviewHeadItem mismatch. Earlier
compile failures are retained. Those test repairs, hook-target compilation,
similarity and the complete native workflow remain pending. No Unit acceptance.

The test repair preserves every check. Independent review is clear.
`swift-test/20261026T184731Z-89874` passes 60 tests with zero skips, covering
input validation, reviews, launch setup and audio. App execution remains pending.

Similarity `20261026T184935Z-92670` passes for the replay and native input work.
Core bridge tests `20261026T185013Z-93287` pass 8 tests; the new seed-provider
test is in EventTests and passes separately in `20261026T185103Z-94660`.
Both runs have zero skips. The actual native media case remains pending.

Team-signed Mac Debug app build `20261026T185336Z-98180` passes, including the
guarded launch/close code. The native media test and its orchestration are in
progress. This build does not establish imported image/audio behavior.

## Native media case

Start `4bbe5060`. The filtered implementation adds the exact native media
import/review/reopen/export case and reuses the existing picker, destination and
strict package proof. R1 found the retained Rust work directory was rejected,
and old model fixtures lacked the new media conditions. R2 closes both: the
work directory is checked separately, and production surface admission requires
the actual renderer and media proof. Unit stand-ins retain the dispatch, token,
elapsed, failure and uptime checks and cannot provide native media proof.

`selftest/20261026T192616Z-17940` passes 5 orchestration/helper tests.
`swift-test/20261026T193535Z-25962` passes 62 tests with zero skips. Final wording
closure is independently clear; privacy `20261026T193529Z-25426` and similarity
`20261026T193532Z-25687` pass. Earlier failed scans remain recorded.

The first runner reported a renderer search that printed the existing public
jQuery file. Its hash matches the pinned public artifact. No excluded tree was
present and no copied material is reported. The source-access review is retained
in the intake. Future searches must exclude the vendor directory.

Actual prelaunch preparation and native execution remain pending. No Unit or
milestone acceptance is claimed.

## Preparation ownership repair

Start `949a6e67`. `ui-test/20261026T193948Z-29128` fails before app launch:
the decoder rejects Docker-created ancestors owned by root. No test ran.
Filtered repair `20261026T194957Z-33990` supplies fresh owned mount points,
keeps the parent read-only and runs derivation as the host owner. A bounded,
exclusive single-link copy of the compiled CLI is executed and checked before
and after derivation. Compiler caches and existing input guards stay unchanged.

Independent source review is clear. `selftest/20261026T200734Z-45432` passes
all 8 focused tests with zero skips. Privacy `20261026T200744Z-45725` and
similarity `20261026T200758Z-46280` pass. The earlier failure remains recorded;
actual Docker preparation and native execution still require a fresh run.

`ui-test/20261026T200831Z-47176` completes Docker preparation and the real Rust
replay, then fails before Import Collection. Focused VM drive
`20261026T201147Z-50808` exposes the launch refusal: the checkout URLs differ
in directory hints even though their paths match. It is diagnostic evidence,
not a passing native workflow.

Filtered fix `20261026T201256Z-52664`, starting at `3d42ea79`, uses the loader's
existing exact file-URL path rule for initial reset. The canonical path, link,
ownership, receipt and frozen-input guards remain. Added cases cover both URL
representations and refusal boundaries. Independent review is clear; focused
Swift execution and a fresh native workflow are still pending.

`swift-test/20261026T202255Z-72349` passes all 16 selected tests with zero
skips, including both URL representations and all retained refusal cases.
Privacy `20261026T202020Z-65904` and similarity `20261026T202019Z-65553` pass.
The fresh native case remains required.

`ui-test/20261026T202419Z-74303` imports the actual package, verifies the media
journal, selects the deck and reveals the answer. It then fails waiting for
media-ready evidence. No image/audio or complete workflow pass is claimed.

Filtered repair `20261026T202854Z-79540`, starting at `a91af770`, propagates
reveal errors into native evidence and reports the existing review notice at
the same bounded wait. Independent review is clear. Focused coordinator run
`swift-test/20261026T203249Z-85975` passes 17 tests with zero skips; privacy
`20261026T203250Z-86382` and similarity `20261026T203251Z-86539` pass.
Renderer and playback behavior are unchanged; the next native case must expose
the precise failure before further repair.

`ui-test/20261026T203359Z-88425` now refuses promptly and reports
`media renderer binding`, before DOM decode. Filtered diagnostic refinement
`20261026T203738Z-91216`, starting at `98bc9766`, preserves each predicate and
its order but gives it a fixed refusal reason. No content or path is included.
Independent review is clear. Focused Swift `20261026T204125Z-96426` passes
16 tests with zero skips; privacy `20261026T204041Z-95476` and similarity
`20261026T204057Z-95958` pass. This is diagnosis, not a playback repair.

`ui-test/20261026T204232Z-98073` identifies nonempty renderer diagnostics as
the failed condition; the earlier binding predicates pass. Filtered refinement
`20261026T204545Z-2130`, starting at `e3666e4a`, reports fixed category counts
for current versus earlier updates. It exports no raw diagnostic data and
still rejects every nonempty diagnostic set. Independent review is clear.
Swift `20261026T204858Z-3949` passes 20 tests with zero skips, including
privacy and size-bound cases. Privacy `20261026T204900Z-4576` and similarity
`20261026T204859Z-4331` pass. The actual category remains to be captured.

`ui-test/20261026T205017Z-6322` records current-answer `audioDecode` and
`bridgeRejected`, with no earlier-update errors. Read-only source diagnosis
finds that answer audio preparation can start while the question is shown;
the update label alone does not locate the time of failure. It also finds
that generic bridge rejection uses the current update for stale traffic.
The existing stale-message contract and tests remain required.

Filtered app-hosted probe `20261026T205741Z-10648`, starting at `d66c3763`,
adds only a generated silent-WAV regression. Independent review is clear;
privacy `20261026T210342Z-13448` and similarity `20261026T210343Z-13672` pass.
VM snapshot run `20261026T210344Z-13911` builds and reports 83 passes, 2
failures and 1 skip: the old image comparison and the new backend regression.
Its normal private file URL opens with duration 2. Both descriptor URL controls
and actual backend preparation fail with OS status -54, including the WAV hint.
No playback occurs. The failing regression stays enabled and recorded.

The source diagnosis reports one broad search that returned the bundled public
jQuery file. It was unused; further reads were restricted to own source files.
No excluded Anki or private tree was read. The report is retained in the intake.

## Native audio handoff contract

Start `d66c3763`; spec-writer session `s-20261026T041939Z-a8f043ce`.
M11 adds the file handoff required by the reproduced -54 failure. Independent
review found P2 `M11-LOCK-PUBLICATION`; R2 resolves it, and final wording review
is clear. Privacy `20261026T212244Z-26603` passes after complete prose rewrites;
the four failed scan results remain retained. No scan exception was added.
Implementation, native playback, integration and timing evidence remain open.

## Bridge rejection details

Start `fb4d09cb`. Filtered work `20261026T212009Z-25220` adds fixed rejection
reasons while retaining all bridge checks and strict media refusal. The first
VM run `unit-sim/20261026T212753Z-46727` passes 7 and fails 1: existing privacy
coverage requires diagnostic names to remain empty. That assertion is unchanged.

Seeded fix `20261026T213039Z-49743` stores the reason in an optional typed field,
with nil defaults and separate deduplication. Names, indexes and script fields
keep their earlier shape. Independent R2 review is clear. Swift
`20261026T213455Z-51622` passes 21 tests; exact VM suite
`20261026T213557Z-53572` passes 9 tests. Both have zero skips. Privacy
`20261026T213458Z-52047` and similarity `20261026T213458Z-52244` pass.
The actual native rejection reason still requires the integrated media run.

## Private native audio copy — verification in progress

Start `cb92e0a6`; implementation export `20261026T212529Z-33802` follows M11.
R1 review found P2 `AUDIO-COPY-SHARED-AREA-STAT`: shared-directory metadata
changes from sibling copies must not invalidate its stable identity. Seeded
repair `20261026T215020Z-62604` checks both observations for owner, type and
0700 permissions with matching device/inode; source and regular-file checks
remain strict. Independent R2 review is clear, with deterministic positive
sibling-change and negative substitution/mode tests.

Initial Swift `20261026T214624Z-59664` failed compilation of two test tasks;
the repair keeps clips within Void-returning main-actor tasks and preserves
the expected-error assertions. Swift `20261026T220013Z-67973` compiles and
passes 44 of 45 tests, zero skips. Its new streaming test wrongly requires an
empty Data value at EOF, where FileHandle may return nil; the exact EOF check
is being repaired. Native preparation, pause/seek/stop lifetime, cancellation
and shared-area race cases pass in this run, but this is not sandboxed app
or playback-timing evidence.

Privacy `20261026T220016Z-68742` fails on test-only text. R3 replaces that
text; the scanner stays unchanged. Similarity `20261026T220019Z-69114` passes.
Swift `20261026T215943Z-66708` could not access Docker from its sandbox and
did not build. The authorized MG invocation above ran normally. These results
leave U124/U162 and their integration gate open.

Seeded R3 `20261026T220108Z-69939` changes only synthetic test names, the
redundant comment and that EOF assertion. Independent review is clear; the
other five seed files are byte-identical. Focused Swift
`20261026T220242Z-70853` passes all 45 tests, zero skips. Privacy
`20261026T220245Z-71584` and similarity `20261026T220300Z-72280` pass.

VM snapshots `20261026T220307Z-72642`: the app-hosted decoder test passes.
The descriptor controls still fail with -54; the production backend opens the
same generated WAV, duration 2. Test totals: 84 pass, 1 image comparison fails,
1 skip. The snapshot run remains failed. Limits, assertions and baselines
stay unchanged. Full native import, review, reopen and export still need proof.
