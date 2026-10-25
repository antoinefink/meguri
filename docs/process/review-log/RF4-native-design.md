# RF4 native design preview

Starting commit: `1bb4916106cbfa81a56f3c9a0440fd9f0b3902f9`.

Antoine authorized this bounded implementation and opening the Mac app on
2026-10-24 after the HTML preview. The full application goal stays paused.
The agreed design is `docs/design/native-interface-contract.md` (N1–N10).
Spec session: `s-20261024T111732Z-b3754a3a`.

Scope: native Settings with Connection/Collection sidebar; one Connection
details disclosure containing real sanitized logs; inline connection test;
quiet-corners review, bottom answer controls and native menu shortcuts; simple
deck workspace and empty state. Preserve actual collection and transfer guards.
Do not apply the held file-media export patch or claim media-file export works.

Antoine explicitly deferred detailed verification until the native design is
reviewed together. This is a WIP preview, not RF4 or milestone acceptance.
Run lint, the focused affected Mac build and a bounded smoke/render check now;
retain broader suites, audits, privacy/provenance/similarity and full integration
gate as required acceptance work after visual agreement.

Implementation, independent review, fixes and actual receipts will be recorded
below. No completed-unit trailer is authorized by this checkpoint.

Implementation session: `s-20261024T112011Z-6ad2db2e`.
Filtered run `cleanroom/20261024T112121Z-31871` passed its boundaries and
exported 23 app files. Parent applied the reviewed patch without conflicts.
`build-app/20261024T113555Z-40409` passed UI lint and the team-signed standalone
Mac Debug build. `swift-test/20261024T113705Z-42383` passed 20 focused tests.

Round 1 independent review found no P0/P1. Three P2 items remain: plain export
labels with honest detail copy; transfer decisions in Collection rather than
the Connection setup page; proper pane-height centering. The compact Mac Study
and sidebar composition is now explicit in N2. The existing Sync Logs heading
was confirmed; it is not missing. These changes are WIP pending the fixer and
the running native preview. Full acceptance checks remain deferred as above.

R2 filtered fixer `cleanroom/20261024T114346Z-48138` closed the source review
items. `build-app/20261024T114842Z-53154` passed lint and the team-signed normal
Mac build with zero warnings. Independent R2 source review was clear of P0–P2.
Privacy retained one explanatory-comment match; filtered comment-only run
`cleanroom/20261024T115258Z-58286` removed those two comment lines without
behavior changes. No guard exception was added.

Antoine approved normal quit of the two older previews. Both quit through their
normal application handlers. The new checked product was copied to a stable
preview path; app and widget hashes matched its receipt. PID 59001 opened that
exact product. Direct inspection of its Settings found a duplicated URL label,
an unchanged-address Save action and too much initial transfer setup content.
Antoine requested ongoing direct visual inspection and Apple-quality results.
No server request was made during inspection. Full acceptance is still pending.


The next filtered visual pass was `cleanroom/20261024T120433Z-66625`.
`build-app/20261024T121202Z-73382` passed lint and the signed standalone build
with zero warnings. The copied preview opened by its exact product path.
Live inspection confirmed one URL label, a hidden unchanged-address Save
action, regular buttons and compact Collection action rows. Independent source
review found no P0/P1. Three P2 corrections remain in the next filtered pass:
URL text alignment, account action identity and visible terminal transfer
outcomes with a Details path after the workflow closes.

`snapshots/20261024T120514Z-67427` stopped at an app/widget signing mismatch.
The explicit team-signing run `snapshots/20261024T121239Z-74789` built and ran
in the dedicated VM. It rendered six question/answer/overflow cells, including
light and dark appearances. The images show bottom answer controls and no
sidebar. These fixtures use sample Text content, not a live collection or the
WebKit card renderer. All six differ from the old baselines; none was recorded
or approved. The suite also found an obsolete R110 assertion that forbids the
new native Settings scene. That assertion needs to follow N1/N5 while retaining
the rule against opening a second main window. Full acceptance is pending.

Filtered runs `cleanroom/20261024T121438Z-79319` and its seeded follow-up
`cleanroom/20261024T121741Z-81556` address those three findings and Antoine's
row-spacing, sidebar-toggle and oversized-import feedback. Independent source
review found no remaining P0–P2 correctness issue in that patch. The normal
standalone Mac build and lint passed. Native inspection and cross-platform
visual checks remain in progress. Antoine chose system SwiftUI styles
and asked to use them across all Apple platforms. Shared state actions,
review button sizing and transfer action styles are the remaining bounded
source findings for that visual pass.

Shared-control run `cleanroom/20261024T122650Z-93034` keeps regular Mac
review/state buttons, native Form action styling and touch targets. Native
iPhone inspection found the same URL row issue; the shared fix also sets
the URL keyboard and removes repeated placeholder copy. The standalone
build `build-app/20261024T122840Z-95007` and privacy check passed. Review
found three Mac state actions needing outer target frames; the two-file
fix `cleanroom/20261024T123010Z-99856` closes that finding. Independent source
review is clear for the bounded patch.

Actual synthetic empty-collection drives ran on iPhone and iPad:
`drive/20261024T122416Z-88382` and `drive/20261024T122543Z-91440`. Their
Home and Settings images were inspected. The drive checks remain red for
missing cell identifiers and title/content overlap reports. No claim of an
accessibility pass is made. They released their simulator leases.

`snapshots/20261024T122911Z-96378` built both targets and returned native
Settings screenshots were returned for iOS and iPadOS. The inspected
iPhone URL row is aligned. Its filter selected more matrix variants than
intended. The receipt records 12 pass, 28 new and 60 deferred; Mac test-host
startup also failed before connection. Preserve this failed receipt and
all returned output. No baseline was changed and no full gate was run.
`similarity/20261024T123055Z-1894` scanned 28 files with zero flags.
`static-checks/20261024T123058Z-2222` passed the commit trailer check.

The standard iPhone/iPad answer render check
`snapshots/20261024T123659Z-12242` passed both selected answer cells. Their
matching baseline images were inspected; passing cells do not export a second
PNG. The touch header utility buttons still looked too large, so a bounded
touch-control correction follows. These are ReviewLayout samples, not a live
WebKit/core session or the whole iPad shell.

The latest incremental standalone product crashed on launch in
`EmptyStateView` actions, at `swift_retain`. The style helper's Mac opaque
result type had changed. A fresh Meguri-only standalone build
`build-app/20261024T124002Z-18726` with the identical source/input digest
`0570b7f1d9b3c345aeef1df57d7791e2dbf73144fcd83540f2cf1609970036f0`
opened normally. No app behavior change was needed. The failed product and
report remain available. The live fresh-build Settings view confirms aligned
rows, no sidebar toggle and a distinct Sign Out action identifier. Account
identity text is still combined with its Login label in the exported AX view;
its content remains readable, but its separate test identifier is not proven.
Keep that limitation for later native accessibility checks.

The final touch pass `cleanroom/20261024T124217Z-21732` moves interaction
minimums outside labels and uses regular borderless review utility buttons.
Independent source review found no new P0–P2 issue.
`build-app/20261024T124340Z-23123` passed and its exact copied product opened
as PID 27698. `snapshots/20261024T124512Z-24790` returned both changed standard
iPhone/iPad answer images. They were inspected: utility controls are smaller
and the answer buttons remain at the bottom. Both baseline comparisons fail
for the changed pixels; no baseline approval was recorded. The expanded
Connection details view still needs left-aligned log text. That layout
correction follows.

The final details pass `cleanroom/20261024T125048Z-32828` shares the same
history body between its Form section and the inline Mac disclosure. Source
review confirmed the extraction leaves data, remedies and guards unchanged.
`build-app/20261024T125320Z-34133` passed. Its copied app executable matched
the receipt, and the exact product opened normally. Direct native inspection
confirmed leading metadata/events and Sync Logs inside the single disclosure.
The app remains open in Connection settings. Source input:
`59f886ca77bff8a2324e08827c39e43cc842dc8832569959134c0b957ae6423a`.

No server operation was started by this visual pass. The larger goal remains
paused. Full native accessibility and regression checks, baseline decisions,
milestone gates and human acceptance remain pending. The exported Mac AX
view still gives history descendants their disclosure's identifier; preserve
this and the account-text identifier limitation for the later native checks.
This preview carries no Unit trailer and accepts no milestone or RF4 unit.

### Media scope defaults

Start: `702decb7`. Contract: `2ab924ae`. Implementation session:
`s-20261024T133024Z-54c532eb`; export `cleanroom/20261024T133044Z-61890`.
The scope picker is removed on every platform. Both directions request media.
Historical scopes, separate media consent, cancellation and partial outcomes remain.

Review round 1: P2 in `CoreManualTransferService.prepare` without a scope argument.
Its default excluded media. Fixer session `s-20261024T134036Z-6513bae7`, export
`cleanroom/20261024T134059Z-74317`, changes that default. Round 2 closed the
finding and found no remaining P0–P2 issues in this scope.

Focused Swift check `20261024T133915Z-69392` passes all 32 selected tests.
It covers both preparation directions, historical scope and media consent/recovery.
That adapter method has no offline test seam; review checked its one-line call.
Privacy checks and focused similarity checks pass.
Native visual checks and broader regression gates remain pending.
Collection-file media support remains separate, open work under N9.

### Curated transfer errors

Start: `ba9dab71`; fixer session `s-20261024T135051Z-59428f7a`.
Exports: `cleanroom/20261024T135101Z-91474` and `20261024T135420Z-96101`.
Known failure ids select fixed messages. Unknown ids use the existing fallback.
No raw server text or address enters the result or its diagnostic events.
Review round 1 found P2: the timeout text assigned an unproven server cause.
Round 2 confirmed the corrected text and no remaining P0–P2 findings.
Privacy check: the flagged test line was replaced with other synthetic values.
The guard and no-exposure assertions remain enabled.
Focused Swift check `20261024T135613Z-97648`: 32 pass, zero failures or skips.
Privacy passes. Static checks `20261024T135106Z-91743`: three pass.
The earlier static failure was a pre-existing real hostname in a reject-all fake;
`ba9dab71` replaces it with reserved test data and preserves its assertions.
No backend, redirect or credential policy changes are part of this patch.

Final preview build `build-app/20261024T135715Z-99569`: PASS, no compiler warnings.
Input: `1616736dcfab36ebd05ed20c377bc1b1e29d5b179c42dffa9d1aba8f92507674`.
The copied app and widget match the receipt hashes. The exact Mac product opens.
Static checks `20261024T135735Z-1030` and similarity `20261024T135736Z-1254` pass.
The larger goal stays paused; no Unit or milestone acceptance is claimed.

## Touch address layout, 2026-10-25

The full app goal has resumed. Layout repair starts at
`3419ceba3f6111ed6682d979d31b9f9fb839c367`.
Implementer session: `s-20261025T024158Z-4e350e81`.
Filtered export: `cleanroom/20261025T024238Z-39959`.

The public Settings fixture in `snapshots/20261025T022726Z-26703` showed a
supported URL split across five short lines at XXXLarge on iPhone. Independent
review found a P2 in the three-control horizontal row. Touch platforms now put
the URL editor below an adaptive label/Test header, using its full row width.
The Mac row, controls, guards and input behavior are unchanged.

Independent source review found no P0–P2. UI lint
`20261025T024405Z-41252`, privacy `20261025T024406Z-41468` and similarity
`20261025T024406Z-41481` passed. Native renders, replacement baselines and the
shared integration gate remain pending. No unit is accepted by this repair.

Candidate generation `snapshots/20261025T025651Z-52051` completed on iPhone,
iPad and Mac: 175 recorded fixture cells, zero generation failures, 23 settings
probes captured, and 490 device-only cells deferred. Deferred cells did not pass.
Native review checked all five states, with small windows, large text, both
appearances and increased contrast on each platform. It found no P0–P2.
The URL field now has full width on touch layouts. Root inspected the repaired
XXXLarge iPhone and native Mac rows. These are candidate baselines, not human
design approval; comparison checks and final evidence remain required.

## Home fixture parity

Start: `f1a6e81f`. The Home preview kept Import as a separate action and exposed
it in populated lists. The real Home view already used the correct empty
composition. The reviewer marked this fixture problem P2.

Filtered patch `cleanroom/20261025T025952Z-58943`, session
`s-20261025T024158Z-4e350e81`, changes only the fixture. It uses the shared empty
content with an inert action, centers the empty Mac layout, and removes the
extra populated-list action. It never opens a real collection.

UI lint `20261025T030555Z-68979`, privacy `20261025T030556Z-69218` and similarity
`20261025T030554Z-68751` passed. Candidate run
`snapshots/20261025T030559Z-69788` compared 18 unchanged cells successfully and
recorded 58 changed cells, with no failures or skips. Those include the earlier
reviewed Mac native controls. Independent review found no P0–P2 in the new Home
images on all three platforms. Root viewed the Mac empty state.
Human baseline approval and the full integration gate remain pending.

## Mac sample action target

Start: `d6075d6c`. Gate `20261025T031426Z-94062` failed the Mac accessibility
audit because `sample.deckOverview.addNotes` measured 88 by 24 points, below
the unchanged 28-point minimum. The Mac UI suite reported the same defect.
Filtered patch `cleanroom/20261025T043049Z-31271`, fixer session
`s-20261024T174115Z-511e3ad6`, adds the existing minimum frame and hit shape
to that Mac button. It retains the native appearance, identifier and action.
Independent source review found no P0–P2. Focused audit
`a11y/20261025T050152Z-62285` passed its all-state, text-size and appearance
case, with no failures or skips. UI lint `20261025T045908Z-57866`, privacy
`20261025T050034Z-60566` and similarity `20261025T045908Z-57978` passed.
Updated render evidence and the required full gate remain pending.
