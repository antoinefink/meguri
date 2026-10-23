# RF3: Collection-only file export

## Scope and starting point

- Starting master: `64701ca6eaf6d660eb4246b6faa31a955fddb659`.
- Deliver the latest-format whole-collection `.colpkg` file without media through native Mac Save and iPhone/iPad Files. Reuse the existing live package writer, review flush barrier and bounded job ownership. File export remains available offline and without an Apple or server account.
- This covers parts of U124 and U162. Media, legacy formats, deck/selection options and the rest of those units remain open. No `Unit:` acceptance is claimed for a partial delivery.
- Explicit self-hosted Anki-compatible import/export remains next after file export. Routine CloudKit sync remains separate.
- Carry the original RF2 integration base `81c207573c20c337c8834e4ae8b4ee8f29603dbf` into the full native gate. The original RF2 full gate remains failed; its known failures have passing focused follow-ups and its required T1 checks pass. The next full gate must cover all pending RF2 and RF3 changes.

## Spec and implementation

- Spec-writer session: `s-20261023T001251Z-3706ebdb`, recorded on master as `c82e0aaa`.
- The new contract describes the current writer and job interfaces. Code work and its checks follow the spec commit.
- Spec committed as `a7267243`: `package/file-export#E1` through `#E6`. Privacy matches in new prose were removed by rewriting the complete sentences; the contract and scan rules are unchanged.
- Ready export artifacts must stay outside temporary namespaces swept on restart. Flush on the worker before snapshot acquisition; keep the FFI starter short. Retain existing reader leases through writer completion and drain them before replacement can close the collection.
- Keep export files on disk across FFI and native handoff. Do not materialize the package in Swift `Data`, `FileWrapper` or a `FileDocument` snapshot. Preserve existing destinations until verified atomic publication.

## Retained UI work

- RF2 critic findings remain open in U152/U166. Apply the small native iOS inline import-title correction with this UI work and obtain fresh evidence for the affected large-text cells. Keep the full title.
- Home shell evidence and the other full-screen findings remain recorded. No complete Home or import visual acceptance is implied by the local review journeys or this export delivery.

## Source delivery and independent review

- Implementer session `s-20261023T001829Z-cd534ca1`, source `ca13993d`, exported own-code patch in cleanroom run `20261023T001929Z-24460`. The runner passed its boundary checks; it ran no compilation or tests.
- Independent round 1 found five issues: reverse path overlap could put backups in owned output; capacity preflight followed directory creation; strict route expectations were missing; unused Ready output could block exports after restart; native Files selection needed replacement serialization.
- Fixer session `s-20261023T004303Z-09daf314`, clean source `da1b62ce`, used the first patch as seed in cleanroom run `20261023T004631Z-33999`. Added canonical overlap checks, reused writer preflight, strict route updates, durable ownership states, native handoff admission and focused regressions. A separate real-file memory test uses the existing sampler.
- Independent round 2 confirmed the original five source fixes. Two P2 issues remain: queued cancellation can leave native completion non-retryable; cleanup can lose its ownership evidence before a failed final sync. Preserve those findings until fixed and checked.
- Native rendering and actual Files/Save behavior remain pending. No parent unit is accepted by this source checkpoint.

## Focused checks on the first integrated patch

- Docker formatting `20261023T005912Z-37858`: pass.
- UI lint `20261023T005945Z-38518`: pass, 115 files.
- Docker focused export tests `20261023T005942Z-38271`: failed, 13 passed and 1 failed. Reverse-nesting coverage attempted to open a configuration already rejected by the host's collection/backup separation check. Preserve the earlier refusal and correct the test to assert its safety at that boundary; the other alias cases still require export-level refusal.
- This checkpoint is WIP with known findings. The full gate, native journeys and measured memory result are not yet claimed.

## Targeted round 2 source fixes (checks pending)

- Fixer session `s-20261023T010207Z-73726269` resumed the supplied formatted implementation and round 1 fixes. The starting master and required full-gate base above remain unchanged; no build, test, scan or commit ran in this filtered session.
- Native completion now captures a terminal guard that restores retryability when its worker is cancelled before entry, dropped, rejected, fails or unwinds. Durable safe ownership still precedes releasing native admission. Swift retains failed completion leases and exposes Retry Export Cleanup without changing a completed save into a failed or cancelled publication.
- Artifact cleanup retains its validated directory identities and narrow unlink/sync phase through errors. Retries recheck protected paths, reject substituted directories, symlinks and unknown files, and can finish the parent sync after their owned directory has disappeared. Added focused regression source for queued cancellation/suspend, completion error/unwind, unlink/sync retry, idempotent discard and a subsequent export.
- Reverse-nesting coverage now expects host admission to refuse backups containing the collection; all other alias configurations still reach export admission. Both boundaries preserve source/user content and assert no owned output creation. The two reported own-source comment regions were rewritten without consulting scanner inputs or changing scan policy.
- Integrator verification remains required. Preserve the earlier focused failure and privacy failure until fresh checks resolve them. Full/native/RSS evidence and unit acceptance remain pending; the full gate must start at `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

## Integrated source checkpoint

- Independent review round 3 confirmed both round 2 findings fixed and found no new P0–P1 issues. Actual native behavior and rendered visual review remain pending.
- Focused check-correction session `s-20261023T011343Z-a99f7798`, cleanroom `20261023T011403Z-45572`, corrected three event borrows in regression tests and removed an optional Swift comment. Executable Swift statements are unchanged.
- Docker format `20261023T011133Z-43852`: pass. The first fresh test build `20261023T011202Z-44223` failed on the three test borrows; that failure is preserved.
- Docker focused export suite `20261023T011520Z-46607`: pass, 18 tests. Includes real backup preservation, capacity refusal, canonical aliases, captured reviews, native admission, cancellation/unwind, restart ownership and cleanup retries.
- Standalone memory check `20261023T011632Z-47651`: pass, one test with no skips. A real sparse 1 GiB file traversed the production destination preflight/copy/hash/publication path. The sampler enforced less than 128 MiB peak growth; length and digest matched. This is Rust file-save evidence, not native Files-provider memory evidence.
- Privacy `20261023T011506Z-46313`: pass for staged source. Earlier failures `20261023T010121Z-39447` and `20261023T011205Z-44480` remain recorded; optional own-source comments were rewritten or removed. No scanner rule changed.
- Similarity `20261023T011523Z-46857`: failed, one structural flag in `core/crates/meguri-engine/src/lifecycle/state.rs`, lines 84–108. Only the sanitized failure was read. Rewrite this region from Meguri's lifecycle contract; no exemption or acceptance is claimed.
- Native bridge checks are in progress. This is a WIP source checkpoint; no full gate or parent unit acceptance is claimed.

## Native compilation and lifecycle correction

- Native checks `20261023T011717Z-48107` and `20261023T011841Z-49969` failed compilation. The first used an array where generated digest metadata requires 32-byte Data. The second caught explicit task-capture and returned-switch syntax in the export coordinator. No native behavioral pass is claimed for those runs.
- Fixer `s-20261023T011846Z-a530e25d`, cleanroom `20261023T011917Z-51403`, corrected those three source issues and rewrote the flagged lifecycle region from Meguri's contract. Operation declarations now keep names and admission flags together through an exhaustive match; all names and flags are preserved.
- Independent round 4 reviewed the small correction and found no P0–P1 issue or deterministic check blocker. It confirmed admission semantics and Swift callback ownership are unchanged.
- Docker format `20261023T012214Z-52791`: pass. Lifecycle tests `20261023T012227Z-53096`: pass, seven tests with no skips.
- Similarity `20261023T012227Z-53095`: pass against the original RF2 base. The earlier similarity failure is resolved without an exemption; no reference source was read.
- Privacy check `20261023T012325Z-54661` passed. Native bridge run `20261023T012322Z-53917` passed its focused tests. Mac and simulator code compiled with no warnings; the tests covered metadata, cancellation, destination lifetime and save outcomes.
- Settings/AppModel tests, actual native destination journeys, rendered accessibility/visual checks and the full gate remain pending. Full gate base remains `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

## AppModel and native adapter follow-up

- `20261023T012419Z-55531`: failed test; entry kept its prior stage. `20261023T012521Z-57306`: failed build; missing CoreBridge import. No UI test ran in that job.
- Session `s-20261023T012731Z-5caf16a7`; cleanroom `20261023T012744Z-60066`: added CoreBridge import. Test now checks no new entry, same stage, generation and lease, refused close and later saved output. Entry code is unchanged.
- MeguriKit `20261023T012918Z-60871`: pass, four tests. Coverage includes actual export/reimport/review, blocked replacement, no collection, and all three cleanup retry cases.
- An earlier session append `20261023T012611Z-58616` failed its privacy hook and was never used by a runner. Only that uncommitted append was removed. The fresh minimal session record above was committed through the normal tool; existing ledger rows and privacy rules were preserved.
- Next: Mac Save, iPhone/iPad Files, accessibility, image review and full gate from `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

## Native Settings checks

- Mac `20261023T012954Z-61946`: failed before export; Settings row click did not open the section. Fixer `s-20261023T013335Z-6b330f3f`, cleanroom `20261023T013356Z-66886`, added real section Buttons and kept shared navigation, tags and selected state. Independent round 5 source review found no P0–P1 issue; images remain pending.
- iPhone `20261023T013332Z-65174`: failed before Settings. Real import, review and restart passed. Native tabs exposed labels with no app IDs after restart. R1964 remains open for the shell. Strict fixture ID tests are unchanged.
- Commit `2d04309c`: native navigation test correction. R1964 open.
- Mac `20261023T014104Z-70595`: Settings opened, but Form put the root ID on its child buttons. No export had started. Fixer `s-20261023T014415Z-259c723e`, cleanroom `20261023T014438Z-73254`, added a container around the Form children; labels, actions and layout stay the same.
- Source includes real route readiness, existing folder selection in Mac Save, and expected saved-file checks. Save/Files, image review and full gate are still pending. Do not accept U150, U152, U162 or U166 from this source checkpoint.

- Mac `20261023T014555Z-74003`: real Save panel cancellation passed. Save failed and retained the prepared export. VM sandbox logs identify denied export temporary sibling creation; destination staging needs an Apple-platform correction. No native save pass is claimed.

## Sandbox publication correction

- Fixer `s-20261023T015555Z-2b662a2e`, cleanroom `20261023T015640Z-79691`, source master `4e67eb02ba54778083c557aebbd83bc77d55375c`. Boundary checks passed. Native checks remain pending.
- Spec `52e7316e` permits authorized staging on the destination volume. Rust checks staging and final paths. Native publication creates a new file exclusively or replaces an existing file through Foundation. Uncertain results retain recovery files.
- Fixer session is recorded in `4e67eb02`. Cleanroom boundary checks passed; native checks are pending.
- Independent round 6 source review: no new P0–P1 findings or deterministic blockers. Native and visual evidence remain pending.
- Docker format `20261023T020920Z-84629`: pass. Focused export tests `20261023T020954Z-85161`: pass, 19 tests. Privacy `20261023T021045Z-85798`: pass after whole-sentence log rewrites; earlier failure `20261023T020954Z-85121` is preserved.
- Native `20261023T021111Z-86248`: production compiled; test compilation failed on an async thread assertion. Fixer `s-20261023T021214Z-66c2fd6c`, cleanroom `20261023T021235Z-88520`, removed that single assertion. Synchronous publication checks remain. Native results are pending.
- Similarity `20261023T021132Z-87399`: pass against the original RF2 base.
- Native CoreBridge `20261023T021528Z-91146`: pass, 12 focused tests plus seven interval tests. Includes real Foundation staging and replacement, exclusive creation, cancellation, uncertain results and coordination lifetime.
- MeguriKit `20261023T021717Z-92930`: pass, four tests including real saved-file reimport and subsequent review.
- Mac `20261023T021236Z-88708`: Save, Replace and Cancel passed. Import did not open. Native follow-up pending.
- Mac `20261023T021906Z-94909`: pass, one test and no skips. Actual import/review/restart, native export cancellation, Save, Replace, reimport, another review and restart all passed. iPhone/iPad, rendered accessibility, critic and full gate are pending.
- iPhone `20261023T022347Z-97809`: failed before Cancel; native picker exposed that control as Other. Import, review, restart and export preparation passed. Fixer session is recorded on master; cleanroom `20261023T022801Z-1529` corrected Cancel and folder-title selectors. Native Files behavior remains pending.
- Mac accessibility `20261023T022820Z-1981`: failed on the indeterminate spinner role expectation. Cleanroom `20261023T023039Z-5686` changed that single assertion to activityIndicator. Labels, target sizes and system audits remain required; the fresh result is pending.
- iPhone `20261023T022952Z-3748`: Cancel existed but readiness failed. Cleanroom `20261023T023520Z-9657` adds post-wait picker capture and native control attributes. This is diagnosis; no Files pass is claimed.
- Mac accessibility `20261023T023306Z-7416`: spinner checks passed; system audit reports contrast for sidebar No decks in all six export cells. This finding remains open.
- Cleanroom `20261023T023805Z-12686`: sidebar No decks now uses textPrimary. Native contrast verification is pending; no audit exception or token change was added.
- iPhone `20261023T023737Z-10822`: post-wait capture shows a loaded picker. Save and More are enabled and hittable; Cancel is enabled but not hittable. Native cancellation remains unproven.
- Mac accessibility `20261023T024040Z-14051`: the primary-text candidate still failed on the background sidebar text. No acceptance is claimed.
- Cleanroom `20261023T024441Z-16867`: bounded native Back navigation seeks a visible Cancel; actual delegate completion must restore Ready. Mac modal background leaves the AX tree, with explicit foreground assertions. Reverted the unsuccessful text-color candidate. All native follow-ups are pending.
- Mac snapshots `20261023T024654Z-17865`: 18 export baselines recorded, 30 comparisons passed, 312 cells deferred outside this focused run. Native-window context, image review, mobile renders and critic remain pending.
- Independent round 6 follow-up: no new P0–P1 findings or deterministic blockers in modal ownership and bounded Files navigation. Native audits and rendered acceptance remain pending.
- Mac accessibility `20261023T025133Z-21775`: pass, six export cells and no skips. Foreground labels and controls remain accessible; covered shell content is excluded while the modal is active.
- Mac drive `20261023T025618Z-25947`: pass, 17 steps. Actual export sheet shows native title and frame; Prepare, Discard and Done work, and Settings actions return after dismissal.
- Mac snapshot verification `20261023T025953Z-29597`: pass, 48 comparisons; 312 cells outside the focused filter remain deferred.
- iPhone `20261023T025315Z-23574`: failed in the test observer before Back navigation. Indexed navigation bars changed while Files settled. No cancellation result is claimed.
- Fixer `s-20261023T025922Z-be2bb59e`, cleanroom `20261023T025932Z-29106`: uses direct native controls and bounded observed Back transitions. No production change. Actual Files cancellation, Save, Replace and reimport checks remain required.
- Independent round 6 test follow-up on `05931fb9`: no new P0–P1 findings or deterministic blockers.
- iPhone `20261023T030509Z-33509`: actual Back navigation and Cancel passed; native delegate returned Ready. The test failed after reopening Files for Save when an indexed folder query lost its remote element. Save and replacement remain pending.
- iPhone export accessibility `20261023T030913Z-38005`: pass, one test and no skips.
- Fixer `s-20261023T030832Z-0a17baf4`, cleanroom `20261023T030902Z-37194`: direct folder targets, native provider readiness and observed folder transitions replace indexed Save navigation. Native Save awaits hittability. Production and cancellation code are unchanged. Earlier runner `20261023T030854Z-36929` refused an incorrect session id before work; the recorded session above ran successfully.
- Independent round 6 follow-up on `44565a3a`: no new P0–P1 findings or deterministic blockers.
- iPhone `20261023T031156Z-40577`: Cancel and first native Save passed. The next Choose action was tapped while native completion still disabled it; screenshot shows Saved with Choose disabled. The replacement picker did not open. Wait for the actual action to become enabled before the next selection; retain the production cleanup guard.
- Fixer `s-20261023T031550Z-fb258a85`, cleanroom `20261023T031612Z-45293`: test waits for the actual Choose button to become enabled and hittable before each Save attempt. The bounded wait fails with evidence rather than tapping again. Production completion and its guard are unchanged; native liveness is still required.
- iPad export audit `20261023T031552Z-43929`: fail, 26 unsuppressed findings. Default screenshots show dimmed shell text behind the sheet; largest-size contrast targets lie below the current scroll viewport. The export fixture lacks the existing import fixture's neutral presenter. Foreground reachability and scrolled audits are required; no acceptance is claimed.
- iPhone `20261023T031830Z-46893`: failed before export. The initial Study Now control existed but was disabled after activation; the test tapped before action readiness. Export cleanup liveness remains unverified by this run. Preserve the assertion and require enabled/hittable study admission.
- Recording `20261023T032259Z-50585`: PASS. recorded=232; captured=23; deferred=472. Comparison pending.
- Review round 6: all 20 phone AX4/AX5 import titles are complete. Export images show no new P0–P1 issue. Scrolling, critic and Home/U152/U166 remain pending.
- Token checks `20261023T033143Z-64955`: pass, 13 tests covering declared contrast pairs and token variants.
- Fixer `s-20261023T032133Z-c04d6162`; exports `20261023T032255Z-50306`, `20261023T032923Z-60497`. Added sheet fixtures, Study readiness and scroll checks. Audits pending.
- Same round 6 review caught the first export's paragraph-fit assertion before integration. The second export allows text to span viewports while requiring observed beginning-to-end coverage; buttons still need full frames. Mac text value handling needs its platform correction before the new Mac audit.
- Privacy `20261023T033355Z-66932`: pass. Earlier `20261023T033257Z-66194` flagged new log prose; whole sentences were rewritten. Public images and scan policy are unchanged.
- Final same-session export `20261023T033604Z-68430`: Mac exact text uses its native String value; four exact contrast entries retain initial audit evidence. One new reason was fully rewritten after the privacy hook; scan policy is unchanged. No native pass is claimed yet.
- Independent round 6 follow-up on the paragraph correction: no new P0–P1 finding or deterministic blocker beyond the Mac value correction above. Initial paragraph-fit issue is resolved in source; runtime proof remains pending.
- iPhone `20261023T033607Z-68711`: actual review, restart, Cancel, first Save, completed cleanup and picker reopening passed. Second Save reached native In progress; no app-owned Replace button appeared within the test wait. Capture native conflict ownership before claiming replacement.
- iPad `20261023T033943Z-71908`: FAIL. Default typical light/dark pass. AX5 stops after Ready: no scroll movement. Other states pending.
- Mac export audit `20261023T034317Z-75306`: PASS, one test and no skips, including added scroll/content proof.
- Fixer `s-20261023T034234Z-c79a0d01`, export `20261023T034313Z-75039`: iOS uses native slow swipes and captures failed movement. Replacement still requires the exact native Replace action, now checked in the app and system alert owner with bounded diagnostics. No production changes or replacement pass are claimed.
- Fix21 export `20261023T034723Z-78395`: standard native swipe calls; optional comment removed. Privacy `20261023T034837Z-79134`: PASS. Earlier `20261023T034626Z-77702`: FAIL on new source. Checks pending.
- iPhone `20261023T034854Z-79513`: PASS, one test and no skips. Actual import/review/restart, native Cancel, Save, Replace, reimport, another review and restart all passed.
- Independent round 6 follow-up on `0042e125`: no new P0–P1 finding or deterministic blocker. Actual native Replace remains mandatory and scroll coverage is unchanged.
- iPad `20261023T035313Z-82363`: PASS, one test and no skips. Actual import/review/restart, native Cancel, Save, Replace, reimport, another review and restart all passed. Native round trips now pass on Mac, iPhone and iPad. Mobile audits and the required combined gate remain pending.
- iPad `20261023T035747Z-85354`: FAIL. Whole-view swipe dismissed the sheet before complete AX5 coverage. Footer padding was counted as visible content space. Frame proof needs the actual footer boundary and fresh geometry after each audit. Native file round trips remain passed.
- Fixer `s-20261023T040137Z-82dbf292`, export `20261023T040251Z-89276`: added footer frame anchor, refreshed audit geometry, bounded content pans and one evidenced Discard contrast reason. Button ownership, appearance and native journeys are unchanged. Checks pending.
- Catch-up `20261023T040307Z-89697`: PASS, nothing queued.
- iPad `20261023T040534Z-90977`: FAIL. Footer and close Button share the same AX frame. Revert that annotation; preserve the proven button and use a conservative audit viewport. No new native pass is claimed.
- Fixer `s-20261023T040945Z-1941ce7d`, export `20261023T041001Z-94415`: restored the proven footer Button. The audit uses a conservative 48pt inset, fresh geometry and overlapping paragraph coverage. No production appearance change; checks pending.
- Independent round 6 follow-up on `4db82cf3`: no new P0–P1 findings or deterministic blockers. Native audit proof remains pending.
- Privacy `20261023T041419Z-96560`: PASS. Run `20261023T041429Z-96967` could not access Tart from the sandbox; no test ran. Doctor `20261023T041503Z-98751` confirms VM SSH, Docker and 46 GiB free with host access.
- iPad `20261023T041457Z-97877`: FAIL. Paragraph coverage completed, but 24 short pans did not reach Choose/Discard after system audits changed scroll position. Scrolled audits also report explanation and Choose contrast while those targets are outside the safe viewport. Preserve findings and correct the bounded test movement; production remains unchanged.
- Same round 6 runtime review confirms the short-pan limit; no product P0–P1 defect established. Require longer measured pans, bounded movement and final control visibility, plus exact evidenced offscreen contrast entries.
- Fixer `s-20261023T041832Z-45dff316`, export `20261023T041859Z-1962`: pans span at most 60% of the safe viewport, with 48 gestures and 128 observations maximum. Two exact contrast entries cover the retained Explanation/Choose findings and are removed for full-frame audits. Production is unchanged; native checks pending.
- Independent round 6 source follow-up on `7c444fa6`: no new P0–P1 findings or deterministic blockers. Privacy `20261023T042046Z-2822`: PASS.
- iPad export audit `20261023T042057Z-3184`: PASS, one test and no skips. Ready, error and loading passed in both appearances at default and largest accessibility text sizes, including continuous paragraphs and full-frame controls. iPhone/Mac freshness and integration gate remain pending.
- iPhone `20261023T042829Z-7018`: FAIL. Paragraph height comparison failed at AX5. Contrast findings: none. Numeric diagnostics and a bounded comparison remain pending.
- Fixer `s-20261023T043102Z-ce426e11`; exports `20261023T043144Z-10524`, `20261023T043321Z-11763`. Height baseline retained; limit 1e-6pt; diagnostics attached. Privacy failures `20261023T043253Z-11382`, `20261023T043400Z-12325` retained. Native checks pending.
- iPhone `20261023T043429Z-12984`: FAIL, native audit timeout Code -56 in error/light/AX5. Ready passed. Height deltas are +/-1.1368683772161603e-13pt. No contrast finding. Keep the failed receipt; reduce redundant supplemental audit calls while retaining coverage proof.
- Independent round 6 on `d9c32b08`: no new P0–P1 findings or deterministic blockers. Privacy `20261023T043417Z-12637`: PASS.
- Round 6: no new P0–P1. Fixer `s-20261023T044129Z-529508e0`; export `20261023T044151Z-17068`. Supplemental audits require new full frames. Initial audits, paragraph coverage and touch checks retained. Native checks pending.
- iPhone `20261023T044306Z-18435`: FAIL, Code -56 at the scrolled Prepare control in error/light/AX5. Ready passed. Same timeout after fewer calls; audit type diagnosis pending. Round 6 source follow-up on `457fab46`: no new P0–P1. Privacy `20261023T044254Z-18083`: PASS.
- Fixer `s-20261023T044919Z-ded73a83`; diagnostic exports `20261023T045021Z-22567`, `20261023T045140Z-23299`. Error runs first. Each audit type gets frame evidence; failures remain failures. This diagnostic cannot pass acceptance.
- Diagnostic `20261023T045237Z-24347`: FAIL, contrast-only Code -56. Prepare and Done frames are stable, fully visible and hittable. Round 6: no product P0–P1 established. Keep initial all-types audits; only this supplemental iPhone error/light/AX5 contrast call needs the documented R1977 limit. Other checks remain required.
- Fixer `s-20261023T045659Z-527598d8`; export `20261023T045731Z-27884`. Diagnostic removed. Only supplemental iPhone error/light/AX5 Prepare omits contrast; initial all-types audits remain. Other types, variants, reachability and screenshots retained. Exact R1977 reason updated. Checks pending.
- Same fixer final export `20261023T045944Z-29138`: Mac uses an immutable audit type; error sign is -56. Privacy `20261023T045912Z-28692`: PASS. Final native checks pending.
- iPhone `20261023T050054Z-30109`: FAIL, same Code -56 at dark/AX5 Prepare. Light/AX5 now passes, with initial all-types and other supplemental types intact. Dark initial all-types passes. Same full-frame control; extend the recorded limit to both appearances for this exact iPhone error/AX5 supplemental case.
- Round 6 source follow-up on `84d19d35`: no new P0–P1 or deterministic blockers. Privacy `20261023T050037Z-29693`: PASS.
- Same fixer export `20261023T050437Z-33910`: exact iPhone/error/AX5 supplemental limit now covers light and dark. Round 6 approves; initial all-types, iPad contrast and other checks remain enabled. Native checks pending.
- Round 6 on `be125276`: no new P0–P1 or deterministic blockers. Privacy `20261023T050544Z-34858`: PASS.
- iPhone `20261023T050558Z-35247`: PASS, one test and no skips. All states and variants passed with the exact R1977 limitation recorded above. Final Mac/iPad freshness, gate and critic remain pending.
- Mac `20261023T051301Z-39885`: PASS, one test and no skips. Final source matches the passing iPhone input. iPad and integration checks remain pending.
- iPad `20261023T051450Z-42197`: PASS, one test and no skips. All three final export audit inputs match `a334c64ce2fab37bc3e3d05f90d385a789fda89966847d8e5bc7821074060b5e`. Native file round trips remain passed on all platforms. Required gate base: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
- Doctor `20261023T052133Z-48396`: PASS, 48 GiB free. Catch-up `20261023T052134Z-48660`: PASS, nothing queued.
- Gate `20261023T052151Z-49236`: CANCELLED after core-lint FAIL. Test lines 1177, 1238, 1280 need lint fixes. Static, pins, canaries and core-deny PASS. Core tests cancelled. Full gate pending.
- Fixer `s-20261023T052622Z-323314c8`; export `20261023T052647Z-66016`: three test let-chains. Assertions and event filters retained. Focused checks pending; run the full gate before refreshing critic inputs.
- Round 6 on `999a5e28`: no new P0–P1 or blockers. Privacy `20261023T052733Z-66996`: PASS. Core lint `20261023T052747Z-67411`: PASS. Focused Rust export `20261023T052821Z-67938`: PASS, 19 selected tests. No production change.
- Gate `20261023T052918Z-68611`: ERROR 141 after the foreground output connection ended. Self-test `20261023T054434Z-94859`: ERROR 255. Passing prefix receipts retained. No complete gate claimed.
- Gate `20261023T054916Z-1052`: CANCELLED before mac-ui. Rust 2749, native durability, conformance, tooling, Swift core/Profile, builds, 659 snapshots, default audits, native card/navigation suites and both simulator unit suites passed. MeguriKit and Xcode mac-unit failed on the same export reimport assertion. T1 remains pending.
- Fixer `s-20261023T052622Z-323314c8`; export `20261023T061902Z-38304`: generation check in canSave; invalidation before host adoption; stronger real reimport assertions. Same round 6: no new P0–P1 or blockers in the patch. Runtime checks pending.
- Privacy `20261023T075128Z-92008`: PASS. Full MeguriKit `20261023T075144Z-92407`: PASS, 356 tests; reimport regression passed. Native Mac round trip `20261023T075528Z-95171`: PASS, one test, no skips, zero build warnings.
- Local Mac preview from `bf7f2434` staged at `build/review-preview/bf7f2434/Meguri.app`; no host launch. Collection-only latest .colpkg scope remains. Self-hosted transfers and final acceptance are pending.
