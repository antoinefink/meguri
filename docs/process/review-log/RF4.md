# RF4 — explicit self-hosted server transfers

Starting commit: `480fc88681143a143a2ee146378fe260512dc961`.

Delivery: manual Import from Anki / Export to Anki, with preview, confirmation,
safety copies, recovery, cooldown and explicit media scope. No AnkiWeb contact.
Full first-version media support remains required. Parent units stay open.

Reuse: U105/U106 code, prepared U109 contracts, existing backup and replacement.
Requirements: R118–R121, R126, R1548, R2060–R2064.

RF3 full gate: required; host setup is clear. RF4 acceptance: pending.

## Contracts and engine prerequisite

Canonical manual/one-way contracts: `87672292`; prepared media tracking/sync
contracts: `72bb8d6a`. Spec-writer session `s-20261023T081747Z-7505bb3d`.
Prepared U109/U110/U111 authorship is retained in those commit messages.
Privacy passes: `20261023T083517Z-18494`, `20261023T084059Z-22559`.
Contract static/provenance check: `20261023T083618Z-20050` passed.
Catch-up: `20261023T083554Z-19580` passed, no queued entries. Disk: 68 GB free.

Engine work starts at `87672292`; recorded implementer session
`s-20261023T083533Z-1769cf18`, filtered source `6eac53cc`, run
`20261023T083545Z-19227`. The patch separates package stamps from publication
decisions: FullDownload requires a fresh pending decision; FullUpload preserves
the prior decision. Verified manual transfers refuse active review sessions.

Focused replacement/recovery tests `20261023T084454Z-24357`: 24 passed, no skips.
Privacy `20261023T084457Z-24600`: passed. Similarity
`20261023T084509Z-24867`: 5 files, no flags or allowlisted matches.
Formatting `20261023T084434Z-23939`: passed. Lint
`20261023T084528Z-25281`: failed on a complex test evidence tuple; repair open.
Independent round 1 review follows below. No UI changed in this prerequisite.

Round 1 found the test tuple lint and a misleading admission comment. The public
check is valid after drain, but exclusive entry does not call it automatically;
transfer callers must repeat it before preparation or remote mutation.
Fixer session `s-20261023T084708Z-2448b237`, isolated run
`20261023T084720Z-26476`, corrects the comment and names the same test evidence.
Round 2 review found no P0–P2 issues. No assertion or executable guard was reduced.
Privacy `20261023T084854Z-27836` passed. Final full engine-package verification
and lint are pending; the earlier failed lint receipt remains retained.

Final engine checks passed:

- `core-test 20261023T084851Z-27593`: 720 passed, no failures; the existing ignored
  `undo_holdback_props::ten_thousand_random_histories` remains ignored for its
  separate U25 run. The enabled 10,000-session prediction stress test passed.
- `core-lint 20261023T085540Z-31118`: formatting, clippy, layering and binary scan
  passed; the earlier type-complexity finding is closed.
- `similarity 20261023T085543Z-31361`: five files scanned, no flags/allowlist.
- `static-checks 20261023T085617Z-31900`: passed.
- `catch-up 20261023T085646Z-32650`: passed, nothing queued. Disk: 63 GB free.

The engine prerequisite has no unresolved P0–P2 review findings. RF4 acceptance
and the required full integration gate remain open, including the host setup
block already recorded for RF3. No milestone or parent unit is accepted here.

Next isolated patch: typed transfer facts and staged-upload storage preparation.
Starting commit `8f227ed0dfead33aab8ebedb05a63a9fddbf9a04`; implementer session
`s-20261023T085242Z-6179d829`. It does not mutate the checkout while running.

Readiness `20261023T085837Z-34619` reports no required setup missing. Host first
launch is no longer blocked. VM is healthy; its older Xcode remains a recorded
warning for native verification. Resume the full integration gate from the
original RF3 base `81c207573c20c337c8834e4ae8b4ee8f29603dbf`, covering this committed
engine prerequisite too. Do not apply the isolated storage patch during it.

Gate `20261023T090045Z-36621` was cancelled after a missing host dependency
prevented the native durability build; all 2752 Docker Rust tests passed first.
Locked host dependencies are now available, and native check
`20261023T092220Z-65494` passed all 25 tests without skips or changed limits.
No passing full gate is claimed. Disk headroom is 42 GB after inactive Meguri
cache cleanup; a backup snapshot appears to retain deleted blocks, so Antoine
has been asked to handle that snapshot or free space before larger Apple work.

Storage prerequisite: implementer `20261023T085302Z-29621`, then fixer
`20261023T090756Z-56474` using `s-20261023T084708Z-2448b237`. Review found signed
SQLite count decoding and dedicated Sync-reader requirements; both are fixed.
The spec writer confirmed KTD3/C6 already require Sync: no contract was relaxed.
Round 2 source review has no P0–P2 findings; focused compile/tests remain pending.
The public package snapshot APIs remain Maintenance-only. Preserve the five tests.

The next collection-transfer backend run is `20261023T091316Z-60091`, reusing
implementer session `s-20261023T085242Z-6179d829` and the reviewed fixed seed.
Source base is `ff62b9b4d60f863117fb21a23258592c4a26c645`. It works in isolation
on manual collection Import/Export and policy. Exact accepted-byte activation
must bypass a second transforming preparation while retaining verification/R6.
No new code from these isolated runs has been applied to master yet.

Antoine removed the backup snapshot; free space returned to 88 GiB. The
workstation step is clear. Backend export `20261023T091316Z-60091` contains 29
core paths, including the preserved storage seed. Formatting
`20261023T095926Z-85746` passed. Initial compile/lint
`20261023T095942Z-86364` failed because the package crate lacked its SHA-1
dependency. Privacy scan `20261023T095946Z-86609` reported six new-source regions;
they need whole-region rewrites without inspecting private tokens.

Backend round 1 independent review found P1 read-only Export activation files
and P2 absent-collection Import recovery that can retain a permanent operation
guard. Both require fixes and regressions. No other findings were confirmed.
The formatted code is preserved for the fixer in the owned stash
`7254a3ae99cf5185981746dd54c9a86ed470d2b0` and an exported own-code seed patch.
No commit hook, check or acceptance boundary was bypassed. Focused tests and
the full gate remain pending until these findings are resolved.

Fixer session `s-20261023T100422Z-89ddfab6`, run `20261023T100541Z-93375`:
both findings are fixed in source. The second review found no P0–P2 issues.
The live file uses mode 0600; the saved file uses 0400. Initial Import recovery
now checks ownership and current file state. Tests cover errors and process
exit before the rename, with the imported cloud decision still pending.
Format and privacy checks passed, including `20261023T101319Z-98773`.
Compile result `20261023T101316Z-98513` reports E0117 in the test transport and
an unused_parens warning. The tests have not run. Fix these checks while
keeping the existing test cases and assertions. No acceptance is claimed.
Server rig `20261023T100850Z-95458` passed eight setup checks. Free disk space
is now over 250 GB after the backup snapshot was removed.

Compile fixer `s-20261023T101509Z-4b27a32b`, run
`20261023T101648Z-729`, adds a local test transport type and removes the warning
without changing the recovery condition. Round 3 found no new P0–P1 issues.
Privacy `20261023T101846Z-2450` passed. Focused test result
`20261023T101843Z-2208`: 49 passed, two failed, five helper tests ignored.
The two failures are final upload freelist count and the initial Import error
assertion. Preserve both failures and assertions for the next repair.

Rig test `20261023T102305Z-3958` failed before login: the endpoint guard refused
the container host address produced by the current rig helper. Keep the guard
unchanged and use the existing local test routing facilities. Lint
`20261023T102308Z-4204` found three style errors; these join the same fix batch.
No runtime/rig acceptance is claimed while any check remains failed.

Check repair run `20261023T105836Z-9862`, fixer `s-20261023T103018Z-bbae3d27`:
the final compaction and replacement-error changes pass the original assertions.
Focused result `20261023T110808Z-15779`: all 51 passed; five subprocess helpers
remain ignored for direct runs. Privacy `20261023T110811Z-16023` passed.
Round 4 found no new P0–P1 issues. The endpoint guard and its rules are unchanged.

Rig compile `20261023T110920Z-17250` and lint `20261023T110924Z-17502` found
private field access in the new test helper. Similarity
`20261023T110927Z-17760` flagged the facts record at storage/one_way.rs25–44.
Rewrite that owned record from the spec and fix test access through the public
API; no scanner or guard exception. Runtime and full gate acceptance stay open.

Round 5 found no new P0–P1 issues in the grouped facts and endpoint helper.
Focused result `20261023T111820Z-23403`: 51 passed, five subprocess helpers
ignored for direct runs. Similarity `20261023T111823Z-23655` passed. Privacy
`20261023T111826Z-23906` reports one new documentation paragraph for rewriting.
Lint `20261023T112157Z-26779` passed.

Live rig result `20261023T112131Z-25947`: the real host collection transfer
case passed, as did all four routing cases and three other rig cases. Two
metadata stability tests failed while the host test changed their shared
server account. Preserve those assertions and isolate the rig tests. Earlier
attempt `20261023T112116Z-25500` could not reach Docker from the command sandbox;
the authorized host invocation reached OrbStack. No product endpoint rule was
changed. Doctor `20261023T112144Z-26269` reports no required setup step missing.
Free disk space exceeds 250 GB after Antoine removed his backup snapshot.

The final check repair reuses fixer session `s-20261023T111328Z-4f11cfe5`
with the owned seven-file patch supplied as a seed. It rewrites the new comment
and serializes shared rig tests. The source remains unaccepted until the new
checks and required full gate finish.

Final repair `20261023T112248Z-27305` applied. Round 6 found no new P0–P1
issues. The final source change is the comment and a shared-resource test
override; production behavior and test assertions are unchanged. Privacy
`20261023T112439Z-28457` passed. Live local-server result
`20261023T112440Z-28672`: all 11 selected cases passed, including the host
round trip, existing status/login cases and four routing cases. Other tests
were outside this focused selection. The earlier failed receipts are retained.
Final similarity `20261023T112453Z-29141` passed.

Native adapter and Settings work starts from backend checkpoint `638d3580`.
Implementer session: `s-20261023T112525Z-07a57d21`. It will run in isolation;
master stays fixed during the integration run from original base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`. Native source remains separate
until that run ends.

## Native source and integration checks

Native implementer run `20261023T112606Z-32521` exported the adapter and Settings
source. First review found four issues: recovery was unreachable after restart
from a saved preview; final confirmation omitted required facts; cancellation
did not reach copy publication; history failure could lose cancellation cause.
Fixer `s-20261023T111328Z-4f11cfe5`, run `20261023T115706Z-80125`, repairs them
and adds regression sources. Round 2 found no new P0–P2 issues. The owning host
task now retains its actual admitted operation id before network requests.

Run `20261023T121314Z-6686` prepares this UI test. It has not run.

| Part | Intended check |
| --- | --- |
| Clock | System time |
| Import | Account A supplies the collection |
| Review | Save an answer and restart |
| Account A | Refuse another transfer before the stored deadline |
| Export | Account B receives the reviewed collection |

This case supplies no native result for A after the deadline. Round 3 source
review found no new P0–P1 issues.

Run `20261023T122515Z-21750` rewrites full sections without changing behavior.
Round 4 found no new P0–P1 source issues.

The privacy token cache dated October 19 was refreshed through the documented
job `20261023T122112Z-15765` against clean committed `891e6701`. All native
source remained outside HEAD. No scanner or exception changed; root read only
counts and results. Remaining wording failures were repaired. The integrator
replaced one complete README paragraph; scan `20261023T123323Z-27815` passed.
Earlier failed scans remain recorded.

The real Keychain test belongs in the VM. Run `20261023T124358Z-44033` moves
its complete body unchanged from CoreBridge's host tests to app-hosted tests.
The pure cancellation case stays in CoreBridge. Independent follow-up confirms
the move. Full native source privacy `20261023T124529Z-47832` passed, 41 files.
All source reviews remain separate from compilation and runtime evidence.

Baseline gate `20261023T112602Z-31273` completed Rust, native durability,
compatibility, Swift, configuration, snapshot, accessibility and UI phases
without a product failure. Rust main phase: 2,783 passed. Native durability:
25 passed. CoreBridge: 87 passed; optimized decoder: one passed; MeguriKit:
356 passed. Xcode check: 148 passed, one deferred to the Mac UI job. Snapshots:
659 passed, 846 states deferred to their remaining feature units. iPhone audit:
12 passed. Mac audit: 11 passed, one skipped. UI tests: iPhone 12 passed,
iPad 13 passed, Mac nine passed, with their platform skips retained.

The integrator stopped only that owned gate after the native candidate was ready,
during simulator unit checks. Cancellation and all completed receipts remain
recorded. This avoids finishing a gate over the older UI before repeating it.
The final combined gate must still use the original base above and pass every
required phase. No complete passing gate or unit acceptance is claimed.

Native candidate `20261023T124358Z-44033` is now applied on master. Formatting
`20261023T130700Z-71217` passed. Focused compilation, lint, privacy and similarity
checks are running. Controller tooling is reviewed separately; only its owned
paths may be combined, never its older native seed. Actual VM server journeys,
peer verification, fault and rendered evidence remain required.

Applied native checks: privacy `20261023T130742Z-72073` passed all 41 files;
similarity `20261023T130740Z-71727` passed 35 files without flags. UI lint
`20261023T130742Z-72036` passed 119 files. Rust lint
`20261023T130742Z-71965` reaches three large-enum warnings in the new FFI result
types. These require a focused fix; no lint suppression or acceptance is added.
Swift compilation and actual native execution have not run on this source yet.

Native WIP `2d351f9f` is committed and pushed. Swift bridge
`20261023T131239Z-76171` passed 95 tests. Focused Rust
`20261023T131914Z-83877` passed 51 tests. Swift Kit compile
`20261023T131447Z-80635` found an observed-property initialization error.
Fixer run `20261023T131821Z-82821` initializes both endpoint fields from one
local value and boxes the large FFI record using the existing UniFFI pattern.
Independent round 5 found no new P0–P1 issues.

The same run integrates the controller's reviewed source. Jobs are
`manual-transfer-check` and `manual-transfer-data`; their source filenames,
callers and test doubles use that family. Only their owned paths were combined.
The operator setting is `MEGURI_MANUAL_TRANSFER_SSH_OPTION_PASSTHROUGH`.
Root inspected the dedicated VM wrapper without printing its text or values:
it passes separate arguments directly to SSH after the target, without an
argument parser or inserted shell. Actual forwarding still requires proof.

| Check | Receipt | Result |
| --- | --- | --- |
| Privacy, integrated fixes | `20261023T132735Z-87917` | Passed |
| Controller tests | `20261023T132759Z-89698` | 15 passed |
| Similarity | `20261023T133147Z-91856` | Passed |
| Swift Kit | `20261023T132735Z-88283` | 362 passed; one recorded audio failure |
| Rust lint | `20261023T132735Z-87985` | Three type errors in the new peer helper |

All transfer model tests passed. The Swift failure is
`controllerQueueAdvancesAcrossMissingMiddleWithinBudget()`, still owned by U69
under approval `E-20261021T134124Z-a07a`. The failed result and hardware
diagnostics remain recorded. No limit or playback setting was changed.
This is not a repair or milestone closure.

The peer helper needs an owned closure capture, Option handling for its event
stream, and lossless duration conversion. These are compile failures; its
actual seed and peer comparison have not run. Native VM journeys, Keychain
integration, rendered review and the full combined gate remain pending.

Controller WIP is `36a12728`. Filtered fix `20261023T133449Z-94781` corrects
the three helper types without changing its checks. Rust lint
`20261023T133736Z-98118` passed. The actual Mac app and UI bundle build
`20261023T133658Z-96816` passed; screen documentation
`20261023T133657Z-96607` passed. Real VM execution and peer proof are next.

This is incomplete RF4 work, without a Unit trailer. Transfer orchestration,
native configuration/login/preview, collection/media execution and recovery,
focused server/native evidence and the required full gate remain open.

### Mac signing prerequisite and corrective checks

VM run `20261023T133904Z-99285`: one failed test, before Login.
The route checks passed and cleanup passed. The peer check did not run.
Mac render run `20261023T134530Z-6140`: Keychain returned `-34018`;
two route tests failed; 24 new images lack baselines. Neither run passed.

Filtered fix `20261023T135332Z-16766`, starting at `f046f949`, updates both
strict route lists and adds credential-free failure diagnostics. Existing
post-login screenshots remain, guarded against any secure field; raw hierarchy
attachments are removed. It also adds explicit `MEGURI_APP_SIGNING=team`
support to shared app builds without changing the default or Keychain policy.
Independent corrective review found no new P0–P1 issues.

| Check | Receipt | Result |
| --- | --- | --- |
| Signing option tests | `20261023T135605Z-19301` | 4 passed |
| Privacy, six corrective files | `20261023T135628Z-20711` | Passed |
| First team build | `20261023T135605Z-19390` | Failed: stale embedded ad-hoc widget |
| Team build after stale-copy cleanup | `20261023T135727Z-21644` | Passed |

The current widget product and parent had the team certificate, while the
embedded widget retained its old ad-hoc signature. Only that generated embedded
copy was removed before the passing build. No entitlement or signature check
was bypassed. The built team app still lacks an app-identifier entitlement and
embedded profile, so the Data Protection Keychain prerequisite remains open.
The existing Apple account and valid development identity were confirmed with
metadata-only reads. No account login or private-key export was performed.

Visual check: no new P0–P1 finding. Baselines and final UI review remain open.

Filtered fix `20261023T140028Z-26189` starts at `c8ca8406`. Team app
entitlements now declare only the app's private Keychain group; ad-hoc and
widget entitlements are unchanged. The two Settings Forms use the existing
background token. Independent review found no new P0–P1 finding.
Five focused tests passed (`20261023T140310Z-28186`); privacy passed
(`20261023T140310Z-28324`).

Team build `20261023T140311Z-28614` failed with Xcode's “No Accounts”
and no matching Mac development profile. The account/profile user step is
pending with Antoine. Neither a profile nor a Keychain pass is claimed.
The failed build remains recorded. Selected-media work continues while this
setup step is pending; RF4 and its parent units remain unaccepted.

Corrective source checks passed: static/provenance `20261023T140635Z-32693`
and similarity `20261023T140626Z-32326` (base `f046f949`).

iPhone run `20261023T140703Z-33525` failed before Login. Save Endpoint
was enabled but not hittable; the endpoint field remained visible while the
keyboard was present. Build, route and cleanup checks passed; no peer check ran.
Filtered fix `20261023T141222Z-38321` uses the containing Form scroll view,
table or collection view and clips iOS gestures above the keyboard. Existing
assertions and timeouts remain. Independent review found no new P0–P1 issue;
privacy `20261023T141501Z-40544` passed. Changed iPhone execution is pending.

After Antoine refreshed the account, team build `20261023T141605Z-42995`
passed. Metadata checks confirmed the app's private group, embedded profile
and profile authorization. The profile covers one Mac, without the test VM.
Antoine's device/profile step is pending; actual Mac Keychain proof remains open.

Selected-media storage starts at `d55cda0e`, recorded by session
`s-20261023T140500Z-c24c8246` before its isolated implementation. Its neutral
owner/reset contract was prepared earlier and integrated without a new protocol
implementation. Native and full-gate acceptance remain open.

### Selected-media journal foundation

The original filtered run `20261023T140605Z-31843` continued after the
Codex restart. Its host handle was lost. The owned container was checked live,
then observed to exit 0 with the validated patch exported. Its temporary auth
file was removed. No passing MG receipt is claimed for the interrupted wrapper.
The prepared patch was retained as the seed for fixer `20261023T143223Z-52317`.

Round 1 found two unsupported SQL count reads, a reset/check race and a shared
download-name transaction error. The fixer corrected those and added focused
regressions. It also rewrote two privacy-flagged regions from the neutral spec.
Round 2 found no remaining P0–P2 findings.

| Check | Receipt | Result |
| --- | --- | --- |
| First focused build | `20261023T142734Z-49762` | Failed on two count reads |
| Focused media tests after fixes | `20261023T155700Z-84707` | 54 passed |
| Privacy, corrected media source | `20261023T155703Z-84961` | Passed |
| Similarity, base `0704decb` | `20261023T155730Z-85332` | Passed |
| Rust lint | `20261023T155734Z-85563` | Four Clippy errors; repair pending |

This checkpoint adds the media journal and its durable commit boundaries.
Transport reconciliation, selected-operation completion and native media
consent/resume remain open. No Unit trailer or selected-media acceptance.

### Mac profile setup

Profile work has Antoine's 2026-10-23 approval. Chrome registered the VM;
the new local profile has both Macs and the app's own Keychain group.
Build `20261023T155213Z-78613` passed with the previous profile. A backup
of that cache entry is kept outside Git. Removing that entry let automatic
signing refresh it: `20261023T155353Z-80293` passed with both Macs covered.
VM runtime checks remain pending. Existing signing keys are unchanged.

Filtered lint fix `20261023T160101Z-87529` starts at `7b79baff` and changes
only shared borrows and equivalent let chains. Round 3 found no new P0–P1
issues. Rust lint `20261023T160317Z-89272` passed; focused tests
`20261023T160321Z-89531` passed all 54; privacy `20261023T160324Z-89783`
and similarity `20261023T160544Z-92858` passed.

`20261023T155447Z-81428`: Mac UI did not run; cleanup passed.
The VM rejected the runner's signing identity: its app identifier has no
profile. The app's own profile is valid for both Macs. No peer proof ran.

Filtered runner fix `20261023T160922Z-95060` keeps Mac UI tests ad hoc in
their own target. Review found no new P0–P1 issues; privacy
`20261023T161422Z-98692` and build `20261023T161419Z-97941` passed.
Signed products confirm the app retains its team profile and private group;
the UI runner has no team and its app identifier has no team prefix.

App-hosted run `20261023T160407Z-90264` and runner smoke
`20261023T161656Z-419` both failed before any test case ran. A one-second VM
sample found the UI runner waiting in `_libsecinit_appsandbox`, before main.
No recent Meguri crash report was found. After both jobs and their VM test
processes ended, the dedicated VM was restarted headlessly under Antoine's
existing permission. The original failed receipts remain; runtime proof is open.

### VM startup consent diagnosis

Candidate `f6d61f3f` ran the replacement full gate from original base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`. Core tests passed all 2,806
cases. Native durability, conformance, goldens, tooling tests, Swift Core
Debug/Profile, Swift Kit and both app builds passed. Xcode checks passed
all Mac configurations, hidden VM routes and iPhone/iPad Debug builds.
Mac Unit passed 437 cases, with zero failures and four skips.

The audio queue test passed in this run. This is not closure of R554;
its required repair and evidence remain open.

No Mac app-hosted case ran. Two stack samples of one PID, more than 30 s
apart, put main in the OS sandbox call. Its secinitd request waits in
`displaySharingConsentPrompt` and `CFUserNotificationReceiveResponse`.
VM consent is still needed. Product code and entitlements stay as they are.

The iPhone app-hosted run passed 62 cases and failed one, with two skips.
The failure is `ScreenSnapshots/screen(_:)`, with baseline validation
reported. It remains open; no native transfer pass is claimed.

After these failures, the owned gate was stopped cleanly to repair VM setup.
Gate `20261023T162740Z-8042` is cancelled/incomplete, not passing. The child
failures and completed checks remain recorded. A fresh required gate remains.

Runner-only test `20261023T162737Z-7272` passed one case after reboot.
The Mac run `20261023T162957Z-25363` and route check
`20261023T163855Z-32590` failed at app launch. Logs have a live app PID
throughout that wait. Neither run reached Login or the peer check.

Filtered setup helper `20261023T172454Z-8567` starts at `f6d61f3f` and
adds only an opt-in read of existing UserNotificationCenter windows. It
requires the dedicated VM account and hypervisor, withholds evidence for
secure fields, and performs no input. Independent review found no P0–P2
issues. Privacy `20261023T172812Z-10545` and UI lint
`20261023T172812Z-10757` passed. Its native inspection is pending.

Inspection `20261023T172834Z-11375` passed one case, without skips. Its
system-window image shows an Open Anyway choice for Meguri, allowing use
of earlier Meguri app data after the signing change. No button was pressed.
The opt-in marker was removed. Antoine's standing permission covers this
VM setup action. The next step is an exact-dialog action and fresh launch proof.

Exact action was prepared in `20261023T173817Z-17544`. Review found a
Touch Bar duplicate made its global button-count guard too strict. Fix
`20261023T174207Z-20344` removed only that global count; round 2 found no
P0–P2 issues. Privacy and UI lint passed. Native action
`20261023T174434Z-22539` stopped at its text/button match guard, before
input. The marker was removed. Full value inspection is next; consent and
native transfer proof remain open.

Full-value inspection `20261023T174957Z-27098` passed one case. Both text
values matched; the system buttons had empty labels and separate titles.
Fix `20261023T175205Z-29010` uses the observed title attribute with the
same exact identifiers and guards. Round 3 found no new P0–P1 findings.
Privacy passed. Consent action `20261023T175338Z-30152` passed one case;
its marker was removed. Fresh team app route `20261023T175439Z-31632`
passed one case. The pre-main consent blocker is repaired.

Transfer run `20261023T175644Z-33869` stopped at preflight because its SSH
attestation was omitted. The documented wrapper contract was checked again
without key output. Run `20261023T175750Z-34414` then reached native UI,
but failed on the Settings navigation target before its first checkpoint.
No peer proof ran. Its safe attachment shows a present, enabled sidebar
item outside the visible area, while Settings endpoint controls are visible.
The helper waits and scrolls before Mac activation; that ordering needs repair.

VM drive `20261023T180527Z-39495` passed both steps. Its generated empty
startup shows the production Settings row and its expected identifier.
There is no demonstrated Settings omission or fixture-install failure.
Final native UI review remains round 6; platform journeys and the combined
gate remain open.

Activation fix `20261023T181115Z-43521` starts at `e283b751` and moves
Mac activation before control waits/scrolling. iOS focus calls remain no-ops.
Privacy passed. Native run `20261023T181302Z-45345` reached Import
preparation, then failed on the missing confirmation control. Its cause
remains open; no complete transfer or peer match is claimed.

Local file-entry test `20261023T182902Z-53059` passed one case: the real
Mac picker imported a generated package, saved a review and preserved it
across restart. Local review suite `20261023T183147Z-55102` passed two
cases and failed the native keyboard/Back case on AX permission, with
`trusted=false` and `AXReadError(-25211)`. The passing cases cover ratings,
undo, wait, exhaustion and restart. Runner setup `20261023T183658Z-57749`
failed to restore trust. The old grant for this exact runner was reset in
the VM before a new setup attempt. Product review behavior is not changed
by that setup action. Final native UI review is still round 6 and pending.

VM policy metadata could not be read over SSH, including an admin attempt;
the OS returned PermissionError. Its content was not printed. Remote app
capture prerequisites and the Import preparation result remain unresolved.

VM runner accessibility setup `20261023T184308Z-62300` passed after a
bundle-specific reset and normal System Settings authorization. The temporary
marker and credential file were removed. Keyboard/Back review case
`20261023T184556Z-64607` then passed one case with no skips. Together with
the prior two review passes and file-entry pass, the focused Mac local-review
checkpoint is proven. The failed combined suite remains failed. A stable signed
preview is copied under `build/previews/mac-local-reviews-eedd4e85/`; it is a
local checkpoint, not full-plan acceptance.

Read-only Remote Login inspector `20261023T190525Z-75337` starts at
`eedd4e85`, owns only VMAccessibilitySetupTests and retains VM/account/marker
guards. Independent round 2 found no P0–P2 findings. It replaces an unstable
indexed child inventory after inspection `20261023T185357Z-69504` failed on a
rebuilding Settings hierarchy. Privacy passed. Corrected inspection
`20261023T191355Z-79348` passed one case, with no skips. It shows Sharing with
Remote Login already enabled; its Info sheet is not yet open. No setting was
changed. Its marker was removed. Supported Remote Login full-disk access
inspection remains pending for generated app-state/peer capture.

The iOS keyboard-toolbar fix is prepared in `20261023T185253Z-68569`, seeded
from `20261023T184503Z-64000`, both starting at `eedd4e85`. It adds a standard
Done action to dismiss field focus; Save Endpoint and Login remain explicit.
The test dismisses the keyboard before advancing. The private check passed.
Native checks and final UI review round 6 remain open. Transfer and milestone
acceptance are pending.

Media-wire draft `20261023T181204Z-44271` remains unapplied and unaccepted.
Independent round 4 found no P0–P1 findings, but actual private scans still flag
six locations. Baseline copy `private/20261023T190403Z-73968` used a clean
committed tree; the counts and findings stayed the same. No private match text was read and no privacy rule changed. A concrete
42-span public API inventory is awaiting Antoine's policy decision. The draft
has not compiled or run its focused tests.

Info navigation patch `20261023T192124Z-86465` starts at `b6719823`.
Independent OS-setup round 3 found no P0–P2 findings. It scrolls only the
observed right pane, then opens the exact Remote Login Info control; it
changes no permission. Private scan `20261023T192353Z-88441` passed.
Native inspection is pending.

iPhone transfer check `20261023T191918Z-82563` failed at the Login result
wait. The build reported zero errors or warnings; the keyboard no longer
blocked Save Endpoint. Login, transfer and peer proof are not accepted.
The controller confirmed cleanup. A safe result-category diagnostic is next.

iPhone local review suite `20261023T192549Z-91021` passed both cases, with
no skips: exhaustion and restart, plus ratings, undo, wait and restart. These
are production reviews over generated content; the real picker path is next.

Login diagnostic `20261023T192511Z-90396` starts at `5bfd9bca` and adds only
entry equality booleans, control state and fixed error categories. Its narrow
independent safety review found no P0–P1 findings; it preserves expectations,
timeouts and product behavior. Private scan `20261023T192837Z-94294` passed.
The final native plan/correctness/visual review remains round 6 and open.

Info inspection `20261023T192410Z-88778` reached the exact Info click and
failed its assumed sheet-title predicate. It changed no settings and its
marker was removed. The next inspection must capture the actual bounded
window state before assuming the Info surface's accessibility type.

iPhone picker check `20261023T192844Z-94566` passed one case without skips:
native file selection, validated collection import, saved review and restart.
This proves the local-entry review checkpoint on iPhone as well as Mac.

Info evidence correction `20261023T192603Z-92129` starts at `5bfd9bca`.
OS-setup review round 4 found no new P0–P1 findings. It captures bounded
Settings windows after the existing exact Info click, even when the surface
type differs from the prior predicate. It adds no input or permission change.
Private scan `20261023T193127Z-97133` passed. Runtime evidence remains open.

iPad checkpoint `20261023T193135Z-97428` passed all three selected cases,
without skips: native file import followed by saved review/restart, exhaustion
and restart, and ratings/undo/wait/restart. The focused local review checkpoint
now has actual native proof on all three platforms. RF3 already retains its
separate file-export round-trip evidence; these focused checks do not close
RF4, the full gate or a milestone.

The earlier iPhone app-hosted snapshot failure is now localized: four missing
`settings.sync/*/dark-xxxLarge-iphoneLarge+hc` baselines on iPhone18,1-24A434.
The enabled test reported missing files, not image differences. Record only
these cells, review their images and verify before the fresh combined gate.

VM Info inspection `20261023T193625Z-960` passed one case with no skips.
Its actual tree and image show Remote Login on and its full-disk-access switch
off; allowed users remain Administrators only. The marker was removed.

Login diagnostic run `20261023T193858Z-3381` failed. Its input booleans are
both true. The failure report itself queried an absent result element before
attaching state; it does not prove a server or Keychain failure. Correction
`20261023T194335Z-7972` guards missing elements and reveals the result before
waiting. Independent narrow safety review found no new P0–P1 findings. It
preserves all expectations and timeouts; native execution remains pending.

The four missing iPhone cells were recorded by `20261023T194344Z-8351`.
Private scan `20261023T194717Z-13497` flags only one comment in the unapplied
VM FDA helper; the native diagnostic and four images are clear. Image review
and fresh comparison remain open. No privacy rule was changed.

The four new `settings.sync` image files total 1,125,517 bytes. Independent
rendered-image review found no visible P0–P2 issues in dark, high-contrast
XXXLarge. Error details and progress are below the captured first viewport,
so these images alone do not prove those indicators. Existing approved images
were not overwritten. A fresh selected comparison is still required.

FDA helper `20261023T193851Z-3052` starts at `bbf45f6e`. OS-setup review
round 5 found no new P0–P1 findings. The helper targets only the exact observed
FDA row, verifies Remote Login and allowed users, permits one enabling click,
and refuses unknown authorization. It reads no credentials. Comment-only
export `20261023T194905Z-14865` removes the two-line flagged comment; the
remaining patch is identical. Actual scan `20261023T195528Z-20027` passed.
The opt-in action is running; no successful setting change is claimed yet.

iPhone `20261023T194914Z-15207` failed after Login succeeded. The safe
attachment reports `resultCategory=logged-in`; the next saved-login row was
absent above the viewport. It confirms the down-only lazy-row search limitation,
not an authentication defect. A bounded two-direction search is in preparation.
No Import/Export or peer-comparison pass is claimed.

The prepared wire patch `20261023T181204Z-44271` compiled locally. Focused
Docker run `20261023T195937Z-23275` passed all 17 tests, with no skips,
including streaming heap bounds, cancellation, corrupt ZIPs and send drain.
Formatting passed. Lint `20261023T200228Z-25600` failed only on one nested
if in the shared test helper, reported through two targets. The formatted
nine-file draft is preserved outside master as a patch with SHA-256
`0ef26dd052df6de756a8240cb9ff43db15ab968b7670ae488d204df145e93727`.
It was removed from the checkout after testing. Privacy approval remains
pending; no wire code or guard exception was committed.

Storage draft `20261023T193933Z-6158` has base `bbf45f6e`. Review round 1
reported two P2 findings: disk payloads survive a crash, and file hashing holds
the journal mutex needed for card-media lookup. Its 17 checks await the fixed
export. The component stays unaccepted.

Spec-writer session `s-20261023T200204Z-089c9b2b`, ledger `21ed1c8f`,
clarifies two technical defaults in `c4cb3a4f`. Zero-byte downloaded members
reject the whole answer as `downloadFiles` bad response before publication.
An existing exact-spelling regular file takes precedence over other spellings;
those other files and journal entries stay intact. Privacy
`20261023T200624Z-27919` passed. Parent units remain open.

FDA action `20261023T195539Z-20344` stopped at OS authorization after its
single switch click. It entered no credentials. Its deferred evidence did
not run after the XCTest failure. The marker was removed. Auth draft
`20261023T195907Z-22763` is unapplied: final OS-setup review round 6 found
one P1, because resumed auth was not bound to the underlying Remote Login
Info state. Its scoped correction is in progress. No credential input is
authorized by the tool implementation until that finding is resolved and
checked; Antoine's standing VM permission is retained.

Fresh snapshot comparison `20261023T201028Z-30289` passed all four selected
cells. The exact missing-file failure is repaired; the full app-hosted suite
and combined gate remain required.

Round 6 P1 correction `20261023T200901Z-29202` binds OS authorization to the
unique underlying Remote Login Info sheet, On state and allowed-user snapshot
before credential use and before Modify Settings. The reviewer confirmed the
P1 closed, with no new P0–P1 findings. Actual scan
`20261023T201514Z-36360` cleared this file; its sole finding was a comment in
the separate native navigation patch. The credentialed VM action is pending.
The refused source copy `20261023T200609Z-27517` ran no implementation while
spec changes were dirty; the later clean run is the correction cited above.

Observed-purpose correction `20261023T202135Z-40696` requires both actual
Remote Login authorization messages, while retaining the verified Info,
account and owner guards. Review found no new P0–P1 findings. The earlier
bound action `20261023T201701Z-37735` refused the different prompt before
reading credentials; its facts confirmed the underlying Info match.

Action `20261023T203715Z-48356` passed one case, no skips. The supported OS
setting is now enabled. SSH can read the generated app policy: one complete
Import, no active operation, and its one cooldown has expired. No receipt,
clock or deadline was edited. Temporary credential/marker removal and empty
VM clipboard were confirmed. Full Mac peer capture can now run.


Storage review round 2 (`20261023T201012Z-29936`) closed both P2 findings,
with no P0–P2 findings remaining. The exact-name rule is implemented.
Combined checks: `core-test/20261023T202212Z-41540` passed 57, no skips;
`core-test/20261023T202956Z-45117` passed 428, with six existing ignored
cases. `core-lint/20261023T202452Z-43228` passed. Fuzz
`20261023T202451Z-43011` passed 104,936 sync_json inputs in the 30-second
smoke run, seed 20261023. Full scheduled fuzz suites remain required.

Similarity `20261023T202450Z-42539` passed 14 source files with no flags or
allowlist matches; three manifests were outside its code scope. Comment export
`20261023T203644Z-47562` changes only the three requested comment regions.
Actual privacy `20261023T210317Z-60697` clears all five storage files and the
new wire-test comment. The unchanged six media_io findings remain open.
Storage similarity `20261023T210417Z-61250` passed after the comment edit.
This lands the storage component only; RF4 and U111 remain unfinished and
require their integration gate. Wire/reconciler code is not accepted here.

Native patch `20261023T210219Z-60131`: wait for FSRS completion; visible
navigation target required. Review: no P0–P1. Privacy:
`20261023T212335Z-71569`, pass, two files. iPhone check: pending.
Native round 6: pending. Platform journeys: pending.

Doctor `20261023T212306Z-69351`: VM ready, disk 188 GiB.
Catch-up `20261023T212325Z-70393`: pass.

Public API draft `20261023T212316Z-69853`: no P0–P2 after review;
Docker scanner tests `20261023T212637Z-77357`: pass.
Privacy `20261023T212640Z-77626`: fail, six media_io regions unchanged.
Draft reverted; no scanner change landed. Wire patch retained.
The existing fixed-span rule in `tools/AGENTS.md` permits public compiler
facts. No Antoine approval is claimed. No private values were read.

Reconciler `20261023T204936Z-56052`, round 1: five P2 findings.
Sanity response race; stale download records; capacity errors;
interruption errors; deletion counts. Fix `20261023T212318Z-70100`: pending.
CoreHost fixture seam: authorized in the worker's scope.

| Check | Receipt | Outcome |
| --- | --- | --- |
| iPhone manual transfer | `20261023T212333Z-70740` | PASS: native case, peer comparison, teardown |
| Wire focused suite | `20261023T213219Z-80882` | 18 passed, zero skipped |
| Wire round 6 | `20261023T212841Z-78688` | No P0–P1 findings |
| Reconciler focused suite | `20261023T213430Z-82255` | Compile failure: OpSource::Raw is unavailable |
| Reconciler round 2 | `20261023T212318Z-70100` | One P2: mutable download eligibility bypasses repair |

Reconciler correction `20261023T213704Z-83672`: pending.
Native iPad check: pending. Native Mac check: pending. Final native review:
pending. Selected-media integration started at `4d6a4c3f` in
`20261023T213146Z-80044`; no completion claim.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Reconciler round 3 | `20261023T213704Z-83672` | No P0–P1; earlier P2 closed |
| Scanner round 2 | `20261023T213730Z-84602` | No P0–P2; qualification fix checked |
| Scanner focused tests | `20261023T214718Z-93536` | PASS |
| Formatted media/privacy scan | `20261023T215933Z-3938` | PASS, 29 files |
| Focused Rust checks | `20261023T215647Z-2105` | 273 passed; 707 excluded/ignored |
| Explicit 19k case | `20261023T215834Z-3417` | One passed; 36 excluded |
| Rust formatting | `20261023T215544Z-1346` | PASS |

Prefix fixture `20261023T214945Z-95017` uses current change stamps and
captured ZIP members. Independent verification found no weakened assertions.
The production code is unchanged. The earlier failed receipts remain retained.

Only three fixed Tokio paths landed in `c6514363`; the 42-path trial remains
reverted. Review covered comment trivia, decoded data and staged contents.
The scanner correction in `20261023T214426Z-91329` changes only a comment
and test construction; its production Python AST matches the reviewed draft.
Wire correction there changes only its test; production wire bytes match.
Wire final round 6 has no P0–P1 findings. Lint, similarity, fuzz, host/native
integration and the combined full gate remain pending. No Unit is accepted.

| Native check | Receipt | Outcome |
| --- | --- | --- |
| iPad | `20261023T213755Z-85257` | FAIL; wrong column |
| Mac | `20261023T215005Z-95784` | FAIL; scroll=nil |
| Helper | `20261023T215516Z-817` | No P0–P1; runtime pending |
| Final review | — | Pending (round 6) |
| iPhone | `20261023T212333Z-70740` | PASS at recorded source |

| Media attachment | Receipt | Outcome |
| --- | --- | --- |
| Initial draft | `20261023T213146Z-80044` | 15 files |
| Round 1 | `20261023T213146Z-80044` | Three P2 findings |
| Correction | `20261023T215356Z-99859` | Pending |

Publication binding, unknown counts and exclusions need correction.
RF4/U111 acceptance and the full gate remain pending.

Composition base: `0637da0f`. The prepared source is integrated without
replacing the committed wire/storage/reconciler work.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Attachment review (4) | `20261023T224050Z-31294` | No P0–P1 |
| Tooling review (2) | `20261023T222619Z-21493` | No P0–P2 |
| Tooling focused tests | `20261023T224358Z-33097` | 22 passed |
| Complete source privacy | `20261023T225938Z-39716` | PASS, 26 files |
| Rust checks | `20261023T230045Z-40314` | 354 passed; eight skipped |
| Bridge tests | `20261023T230209Z-41020` | 97 passed |
| App-model tests | `20261023T230254Z-42636` | 90 passed |
| Strict Rust lint | `20261023T230245Z-42280` | All four checks passed |

The earlier complete Kit run `20261023T223402Z-27811` passed 365 tests and
retained its known issue. This is not an audio repair or final acceptance.
The failed compile and lint receipts remain retained.

The prepared-decision cancellation API now follows the existing cancellation
convention. It checks captured identity and publication proof; persisted
Declined, partial results and cooldown rules are unchanged. Seven bridge
calls remain covered. Both M8 evidence requirements now have controller
checks: observed requests and Cancel/restart/resume. VM execution is pending.

Cache refresh `20261023T223035Z-24022` ran on clean `0637da0f` only:
87,721 candidates, 4,773 already in the repository, 82,948 tokens. No private
values were read. The unchanged candidate still failed afterwards; the
source corrections above resolve it without another scanner-rule change.

Parent final review, rendered UI, accessibility, all three native journeys
and the combined full gate remain required. No Unit trailer is added.
Gate base remains `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

Mac Settings reachability; start `663f1a53`.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Geometry probe | `ui-test/20261023T231739Z-54407` | FAIL; null app frame |
| Window clipping | `ui-test/20261023T232026Z-57316` | FAIL; Form did not scroll |
| Grouped Form | `ui-test/20261023T232439Z-60650` | PASS; one test |
| Privacy | `private/20261023T232452Z-61817` | PASS; two files |

Filtered patch `cleanroom/20261023T232316Z-59775` retains both failed
receipts. Independent source review found no P0–P2. The production Mac
Form now scrolls; the test reaches history and returns to the endpoint.
No login or server request occurs in this test. Final review remains round 6.
The real media journeys, rendered checks and combined gate remain open.

VM evidence bridge; start `2b7b92a5`.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Mac media | `manual-transfer-check/20261023T232610Z-63483` | FAIL; direct file read denied |
| iPhone media | `manual-transfer-check/20261023T233046Z-67479` | FAIL; direct file read denied |
| Tool tests | `selftest/20261023T234157Z-78664` | PASS; 23 checks |
| Bridge tests | `swift-test/20261023T233949Z-75364` | PASS; 97 tests |
| Model tests | `swift-test/20261023T234003Z-76336` | PASS; 90 tests |
| Native compile | `ui-test/20261023T234313Z-79526` | Compiled; one route test passed, two fixture checks failed |

Patch `cleanroom/20261023T233219Z-71131` replaces direct reads with fixed,
authenticated VM reads. Tooling review round 3 has no P0–P1. Its two lower
priority notes remain recorded: file signatures across the full snapshot,
and a stricter consent revision comparison. A later comment-only correction
is in `cleanroom/20261023T234549Z-81833`.

The fixture failures concern its Form lookup. Native media proof, visual and
accessibility checks, final native review round 6 and the full gate remain
open. No Unit is accepted.

Prepared media rendering fixtures; source base `2b7b92a5`, saved on
`c28699fc`. Filtered final export: `cleanroom/20261023T234905Z-86366`.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Fixture review 1 | `231545Z-53043` | Four P2 findings |
| Fixture review 2 | `232607Z-63231` | One P2 finding |
| Fixture review 3 | `233335Z-71906` | No P0–P1 |
| Package checks | `233949Z-75364`, `234003Z-76336` | 187 passed |
| Mac fixture runtime | `ui-test/20261023T234313Z-79526` | Two failed; Form lookup |
| Privacy | `private/20261023T235038Z-87611` | PASS; nine files |
| Similarity | `similarity/20261023T235048Z-87910` | Eight passed; one excluded |
| Inventory | `design-docs/20261023T235058Z-88247` | PASS |

Five media routes use synthetic values and inert wrappers. Only consent
adds the overflow matrix cell; supplemental captures cover the other
states. Mac keyboard action checks are implemented but unproven. The iPad
keyboard skip remains a final device-evidence gap.

This is WIP. The next fixture run records numeric Form lookup facts.
Rendered evidence, audits and final native review remain required.

Replacement admission; start `d627a9f6`.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Initial regression | `core-test/20261024T004336Z-19694` | FAIL; overlap retained |
| First matrix | `core-test/20261024T010521Z-34325` | 10 passed; Import fixture failed |
| Corrected matrix | `core-test/20261024T011351Z-82387` | 11 passed; all 20 cells |
| Host manual transfers | `core-test/20261024T011845Z-88774` | 37 passed |
| Affected packages and R6 migration | `core-test/20261024T012559Z-94377` | 366 passed |
| Bridge | `swift-test/20261024T011239Z-75400` | 98 passed before internal renaming |
| App model | `swift-test/20261024T012722Z-95399` | 90 passed |
| Strict Rust lint | `core-lint/20261024T013106Z-1560` | Four checks passed |
| Complete source privacy | `private/20261024T011902Z-89111` | PASS; 14 files |

Filtered source: `005255Z-24811`, `010752Z-36373`, `011524Z-85071`.
Independent reviews: round 1 found the Import fixture defect; round 2 had
no P0–P2; round 3 had no P0–P1. Source commit: `86dd295a`.
The retained replacement chain has no pruning; review records it as follow-up.
No success receipt or cooldown is created for a superseded partial operation.
The full engine run `core-test/20261024T012007Z-90480` was cancelled while
its unrelated 10,000-session test ran. Its cancellation receipt is retained;
the required full integration suite is still pending.

VM fixture preservation; start `d627a9f6`.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Staging/archive focused | `selftest/20261024T004513Z-20477` | 31 passed |
| Initial archive | `manual-transfer-check/20261024T005319Z-25298` | ERROR; preconditions blocked |
| Corrected guard focused | `selftest/20261024T010430Z-33521` | 33 passed |
| Actual archive | `manual-transfer-check/20261024T010643Z-35402` | ERROR; controller Bash 3.2 parsing |
| Host contract checks | `selftest-host/20261024T011001Z-41613` | 29 passed |

The actual archive returned SSH exit 0 and a successful preservation receipt.
Read-only verification found the source absent and archive present, with matching
policy hash, directory device and inode. The controller error remains recorded;
the archive was not repeated. The bytes remain in the dedicated VM. This is
environment repair, not M8 transfer evidence. Guard review round 5 and portability
review round 6 found no P0–P1. Commits: `3c458085`, `a4701955`.

Supplemental rendering remains open. Mac run `ui-test/20261024T000613Z-98350`
passed; independent review covered 34 synthetic captures. iPhone largest-text
runs `005947Z-27969`, `010816Z-37609`, `011420Z-82867` failed in the helper.
Captured geometry led to window-bar clipping and offscreen travel corrections;
visibility assertions and the 24-gesture limit remain enabled. Helper commits:
`d627a9f6`, `245ddd9a`, `81e7fa88`. The corrected focused run
`ui-test/20261024T013040Z-99521` passed. Independent visual review covered all
six largest-text captures and found no P0–P1 issues.
Explicit audits, all three real media journeys,
parent native review round 6 and the combined gate remain open.
Gate base: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`. No Unit is accepted.

VM runner registration; start `81e7fa88`.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Refresh | `ui-test/20261024T013537Z-4157` | One passed |
| Fresh process trust | `ui-test/20261024T013718Z-6392` | One passed |
| Mac keyboard actions | `ui-test/20261024T013912Z-8511` | One passed |
| Helper privacy | `private/20261024T013320Z-2898` | PASS; two files |

Independent review rounds 1 and 2 found no P0–P2. The two jobs recorded the
same executable SHA-256; trust changed from false to true. Read-only checks
confirmed all temporary authorization and credential files absent. Source
commit: `5e7cb073`. Start, Cancel, Resume and Keep Media for Later were
activated with Space and produced their asserted fixture states.

The local privacy cache was refreshed on clean `81e7fa88` only, after the
implementation passed its scan and was committed: `private/20261024T012706Z-95025`.
The 87,721 candidates and 82,948 tokens were unchanged. No private values were
read and no exemption was added. The final neutral contract passed its privacy
check and independent review round 3 found no P0–P1. Earlier contract candidates
and their failed receipts remain retained; no scanner exception was introduced.

Mac native M8; source `619459f7`.

`manual-transfer-check/20261024T014136Z-11499` passed. Native completion and
cleanup are true. Peer comparison covers all logical cells, saved review and
scheduling data, exact device/server media bytes and both real-hour cooldowns.
Import retains its unknown prior effects after cancellation/restart/resume;
Export completes under the second account. No controller evidence is substituted
for the native actions.

The iPhone run `manual-transfer-check/20261024T014935Z-17500` failed at its
first native-state read, before media consent. The reader returned unavailable.
The failed reader stage was not recorded. Investigation continues. The
corrective tooling keeps the fixed device and bundle, and checks the frozen
container identity. Review and runtime proof remain pending. Both failed focused receipts are retained: `selftest/20261024T021510Z-43189`
and its diagnostic reproduction `selftest/20261024T021711Z-44074`. The latter
exposed a test attribute shadowing unittest's run method.

Standalone preview; start `619459f7`.

| Check | Receipt | Outcome |
| --- | --- | --- |
| Focused build tooling | `selftest/20261024T020921Z-38645` | Eight passed |
| Privacy | `private/20261024T020919Z-38398` | PASS; three files |
| Normal signed Mac build | `build-app/20261024T021052Z-39318` | PASS; zero warnings |

Independent review round 1 found no P0–P2. Intake corrected the core stamp's
existing 32-character format before verification. Commit: `de3c2f89`.
The standalone product uses normal sandbox entitlements and isolated products;
the copied app and widget hashes match the verified build. Preview:
`build/previews/mac-reviews-and-anki-transfer-619459f7/Meguri.app`.
The earlier `build-app/20261024T015625Z-31579` was a test build and is not the
standalone preview. All remaining native, audit, visual, review and gate work
stays open. No Unit is accepted.

Mobile reader correction; first source `619459f7`, corrective source `de3c2f89`.
Filtered exports: `020331Z-35316`, `021940Z-45395`. Round 1 found one P2
namespace race; round 2 found no P0–P2. The initial resolver and full read now
share a before/after namespace check, with the original context and marker
rechecked. No second resolution or fixture restaging is allowed.

`selftest/20261024T022235Z-47152` passed 33 focused checks, including the socket
observer and deterministic namespace changes during resolution and reads.
`private/20261024T022232Z-46896` passed all five files. Commit: `72eac584`.
The actual mobile run is still required; the historical unavailable error is not
claimed resolved. The native M8 assertions remain enabled and unchanged.

Mobile runtime corrections; start `1700ed8a`.

The next iPhone run, `manual-transfer-check/20261024T022658Z-49372`,
failed before media consent. Its bounded diagnostic identified the marker
reader stage (503, child exit 1, script line 525). It did not identify an
underlying errno. The initial collection import had completed. This failed
receipt remains retained.

Commit `c1fc0a29` moves the mobile fixture bootstrap to the first app launch,
before login or import. The fixed leased simulator and exact app bundle are
resolved once, their namespace is checked before and after setup, and that
context is frozen for subsequent reads. The reader cannot restage or rebind.
Replay, concurrent setup and protocol traffic during setup fail closed.
Only reserved synthetic media and its ownership marker are staged; collection,
policy, clock and cooldown data are unchanged. Mac staging guards remain.

Filtered exports: `024216Z-56968`, final `025513Z-62899`. The latter deletes
two comments only; AST and executable tokens match and the other six files
are unchanged. The two preceding no-op handoffs, `025206Z-61232` and
`025318Z-62034`, remain retained. Focused checks:
`selftest/20261024T025031Z-60441`, 34 passed. Privacy:
`private/20261024T025634Z-63654`, seven files passed. Independent component
review round 3 found no P0–P1.

The resulting iPhone run `manual-transfer-check/20261024T025707Z-64165`
passed bootstrap, collection import and the initial native-state read,
then started media and activated Cancel. The partial result appeared and
progress disappeared. The subsequent readiness check failed because history
was absent from the virtualized Form. This is not proof of a transfer defect.

Commit `8127b428` adds the existing bounded reveal call before the two
post-Cancel and completion history readiness checks. All persistence, drain,
hash, counter, receipt and cooldown assertions remain enabled. Independent
review found no P0–P1; privacy `private/20261024T031042Z-80985` passed.

Parent integrated native review round 6 is clear of P0–P1 source findings at
`8127b428`, against `81c207573c20c337c8834e4ae8b4ee8f29603dbf`. It covers
plan conformance, correctness and clean-room boundaries, using the previously
independent 34 Mac and six iPhone largest-text visual assessments. Native
mobile proof, explicit audits, critic and the combined full gate remain
verification requirements. No Unit is accepted by this review.

Run `manual-transfer-check/20261024T031104Z-81436` reached Cancel, the
partial result, history and durable-state checks. It failed the observer's
zero-error assertion at the subsequent abort control. Cleanup passed.
History traversal had delayed that control until test time 442 seconds;
the observer permits a held request for 90 seconds. The failed result is
retained and is not counted as native acceptance.

Correction starts at `8127b428`, filtered export `032030Z-87710`, commit
`09b0bfe9`. The two post-Cancel history calls now follow abort, drain and
the cancelled checkpoint. Before abort, the test still requires the partial
result, no progress or cooldown, durable Cancelled state, matching operation
identity, no receipt, retained unknown effects, zero file counters, absent
server file and unchanged device bytes. The observer, its error assertion
and every timeout are unchanged. Targeted independent review found no
P0–P1. Privacy `private/20261024T032140Z-88499` passed. Actual runtime proof
is required before treating this correction as verified.

iPhone native M8; source `09b0bfe9`.

`manual-transfer-check/20261024T032159Z-88906`: PASS.

| Evidence | Result |
| --- | --- |
| Native completion and cleanup | true; one UI pass, zero skips; 1,132 seconds |
| Peer collection comparison | all logical cells, identity, saved Easy review and scheduling match |
| Media in each direction | exact device and server bytes; typed counters; empty file excluded |
| Stored cooldowns | real hour, Import and Export |
| Unknown prior effects | retained by resumed Import; absent for fresh Export |
| Recovery | real Cancel, durable partial, passive restart, renewed consent and Resume |

Bootstrap similarity `20261024T030037Z-67794` and combined static checks
`20261024T030038Z-67991` passed. The tiny later history changes remain
covered by their independent reviews and privacy checks; the combined full
gate is still required against base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`. iPad M8, supplemental mobile
rendering, explicit audits and critic remain open. No Unit is accepted.

iPad native M8; source `21b61196`.

| Check | Receipt | Result |
| --- | --- | --- |
| iPad native and peer checks | `manual-transfer-check/20261024T034252Z-99322` | PASS; one UI pass, zero skips |
| Completion and cleanup | same | true |
| Logical cells, saved review and scheduling | same | match |
| Media | same | exact native and server bytes; typed counters and empty-file exclusion |
| Import and Export cooldowns | same | real hour |
| Prior effects | same | Import unknown retained; fresh Export false |

M8 now passes on Mac, iPhone and iPad. The required supplemental rendering,
explicit audits, critic and combined full gate remain open; no Unit is accepted.

Transfer accessibility correction; start `17be807f`.

All three native M8 receipts remain passing. The first explicit iPhone audit,
`a11y/20261024T040339Z-9892`, failed after 256 seconds while revealing the
diagnostic fixture marker in overflow. Its row frame began at y=168 and the
navigation-clipped viewport at y=176. The marker text was readable in the
boundary capture. The audit also collected contrast, Dynamic Type and endpoint
clipping findings. Neither iPad nor Mac explicit audit ran after this failure.

UI correction round 1: filtered `041156Z-14029`, commit `936c3c9a`.
Two files changed. Native control styling and semantic fonts, a local heading,
vertical endpoint input and a wrapped stored endpoint were added. Supplemental
traversal starts at real product text; the initial all-types audit still covers
the diagnostic marker. All real content, controls, limits and disabled guards
remain. Independent source review found no P0–P2; privacy
`private/20261024T041420Z-15231` passed.

Run `a11y/20261024T041520Z-15838` continued to report findings. The controller
stopped it with SIGTERM after repetition to measure the rendered screen. Its
ERROR/cancelled receipt and log remain; it is incomplete, not a pass.

Fresh synthetic capture `ui-tree/20261024T042117Z-19419` failed its structural
identifier check on the introductory text cell. Its screenshot and hierarchy
were retained. Save Endpoint was enabled; Log In was disabled. The visible
text was dark on light native button fills. No pixel ratio was measured.

Diagnostic probe starts at `936c3c9a`; final export `043128Z-25376`, commit
`cd479831`. It adds one iOS-only diagnostic test, with four fresh light app
launches: default and AX5, each with contrast and Dynamic Type separately.
It captures window images before auditing and bounded geometry for two fixed
controls. Findings remain failures; there is no acceptance screen report.
The invalid empty-route marker guard was removed before execution. Two fixed
diagnostic strings were rewritten after private scan `043028Z-24784`; no
private values were read. Independent review found no P0–P2.

`ui-test/20261024T043324Z-26727` failed after 36.6 seconds. The isolated default
contrast check reported Save Endpoint and Log In. The isolated default Dynamic
Type check reported those controls and the login notice. At AX5 the controls
were offscreen/absent; the body text visibly grew. This is not control-scaling
proof. All four probes ran; measurements remain diagnostic evidence only.

UI correction round 2; start `cd479831`, export `044253Z-32497`, commit
`dfad1944`. The native action pattern now matches the existing status actions;
extra label framing and foreground overrides were removed. The feature has a
semantic body font, stable informational IDs and a human endpoint label with
its full verbatim URL value. Independent review found no P0–P2. Privacy
`private/20261024T044543Z-33937` passed.

Sample calibration `a11y/20261024T044159Z-30658` passed all states and variants
in its selected case. The same four-case transfer probe
`ui-test/20261024T044601Z-34337` still failed. This did not justify a general
VM or SDK workaround, or a suppression.

One pixel analysis, `image-tools-selftest/20261024T050017Z-55256`, measured
the synthetic native default/light capture. The wrapper repeated its cached
calculation; those repeats are not independent coverage. Save Endpoint text
against its native fill measured 12.73:1. Disabled Log In text measured 1.55:1.
The latter remains a legibility defect. The report makes no dark-mode,
AX5-control-growth or overall accessibility-pass claim.

Snapshots `20261024T044917Z-37212` completed: 171 missing baselines, four
changed images, 490 deferred cells and 23 other captures. No images were
recorded as approved by this run. Critic `20261024T051821Z-68221` produced a
valid warning report: 14 high truncation issues, eight medium and three low.
Packet: 175 images. Coverage: partial, per the critic's notes.
Visual acceptance remains open.
The high issues concern the long preview navigation title. Production Settings
already uses the shorter Settings title; preview parity needs correction.
Medium findings cover form hierarchy, width, error presentation and Mac field
affordance. UI correction round 3 and fresh required checks remain open.

Approved exact API facts; start `cdc3ae09`.

Antoine approved the 42 exact spans from the prepared public-facts packet on
2026-10-24. Decision `E-20261024T045314Z-f655` retains the exact list and packet
digest. Commit `31dea575` adds only those spans to the existing three. No
matcher, decoder, corpus or token-set behavior changed. Independent security
review found no P0–P2.

| Check | Receipt | Result |
| --- | --- | --- |
| Vault synthetic checks | `private-selftest/20261024T050144Z-56051` | Seven passed |
| Scanner final | `selftest/20261024T051514Z-65014` | Unit-test wrapper passed; 63 registered tests |
| File scan | `private/20261024T051516Z-65241` | Four files passed |
| Historical candidate before | `private/20261024T045703Z-53056` | Failed; retained |
| Same candidate after | `private/20261024T050349Z-57333` | Failed; four flagged lines remain |

The historical module remains byte-identical to the packet digest and is not
accepted. No extra facts were inferred. The initial scanner failure
`050347Z-57078` and detailed reproduction `050558Z-58158` were a stale negative
expectation for the now-approved exact Semaphore constructor. The independent
42-positive inventory still covers it; other shortened names stay negative.
Private scan `051033Z-60540` required removal of an unnecessary comment and
renaming one custom test method. Bodies, assertions, registration count and
production bytes were verified unchanged by `051327Z-62628`. The rejected
approval-resolution prose remains in `escalation/20261024T051641Z-66369`;
the replacement records only author, counts, commit and result IDs. The
approval is resolved in `cb6c5cb2`. No Unit is accepted by these changes.

UI correction round 3; start `1feb8fac`.

Filtered `053226Z-75008` changed two Settings files. Preview uses the
production Settings title. The footer retains the setup explanation. Actions
use existing tokens, wrapping text and 44-point iOS or 28-point Mac minimums.
Mac fields use the native rounded border. Bindings, actions, consent,
disabled conditions and identifiers remain unchanged.

Independent review found one P2: the active replacement action used the
normal action token. Filtered `054045Z-79429` changed it to `destructive`;
every other source byte matched the prior export. Review then found no
P0-P2. Commit `f578cf60` is on master and origin. Privacy
`20261024T054219Z-80447` passed.

Probe `ui-test/20261024T053913Z-77983` was cancelled during VM preflight,
before any build or test. Its error receipt remains. The authorized VM
restart completed. Probe `ui-test/20261024T054305Z-81561` queued after
240 seconds without SSH. Antoine disabled the VPN; doctor
`20261024T055003Z-5825` then confirmed VM access. No measured UI pass is
claimed for this correction.

Gate `20261024T054737Z-83344` used clean `f578cf60` and original base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`. Completed runs passed:

| Check | Receipt | Result |
| --- | --- | --- |
| Static | `054738Z-83734`, `054755Z-93410` | Passed |
| Canaries | `054811Z-3394`, `054816Z-3696` | Passed |
| Core lint and dependency check | `054903Z-4692`, `054909Z-5023` | Passed |
| Core tests | `054913Z-5310` | Main suite: 2919 passed; C8: 225.100 seconds |
| Native durability | `055544Z-8711` | Passed |
| Conformance | `055648Z-9522` | Passed; pending/reference coverage remains explicit |
| Goldens | `060245Z-13908` | Passed |
| Harvest | `060259Z-14504` | Passed |

Antoine changed R2062 while the gate ran. The gate was cancelled at
selftest, before Apple checks, to combine the new policy with the final
integration run. The error/cancelled receipt remains; this is not a full
gate pass. The final gate still uses the original base above.

Custom-server decision and preview feedback (2026-10-24).

Antoine requires no 60-minute wait for custom servers entered by URL.
Spec-writer session `s-20261024T060421Z-a856f241` started from `399db61b`;
five-document contract commit `d1d1e172` passed independent review with no
P0-P2 and privacy `061403Z-25367`. The initial privacy finding
`061244Z-24073` was resolved by rewriting its complete public paragraph.
Backend work starts at `d1d1e172`. M6 retains old history, receipts and
active/uncertain-operation guards. M8 now requires the same-login
import, saved review, restart and immediate export cycle. Earlier M8 used
different import/export accounts and does not prove this new cycle.

The host preview found a 69-byte destination dated 2026-10-13. It matched
the public `app/Fixtures/TestHooks/single-file.fixture` byte for byte;
SHA-256 `cf43867b9ceda021478e02d2f55c46ceb7331417afc6fea3c84e39b7d62ccf6a`.
The guarded repair archived only that exact file under the development
container's `MeguriTestArchives/RF4-20261024-known-test-fixture/`.
Hash, size and inode were preserved; the collection file path is absent.
No other collection files or credentials changed. The user was asked to
retry or reopen. Unknown invalid user files must never be reset automatically.

Antoine requested these further changes:

- Log In saves and validates the entered server URL as part of that action.
- Collection file export includes media; remove its Without Media wording.
- Settings uses native groups and a readable form width.
- The empty deck sidebar explains how to start.
- A failed collection open shows useful copy and recovery actions.

Finish the same-account server review cycle first. Then implement media
package export and the UI changes. File export currently omits media;
its writer and reader require implementation before changing its label.
No Unit is accepted by this checkpoint.

M6 backend; start `d1d1e172`.

Implementer `061532Z-26802` removed the custom-server wait and preserved
legacy history. R1 found one P2: unknown stored fields could be lost during
an update. Focused check `062217Z-31994` also found a missing FFI test
dependency. Fixer `062519Z-34265`, from `9f0ab1e2`, closed both. Strict
decoding covers policy objects and nested account/publication metadata;
valid legacy defaults remain. R2 found no P0-P2. The new dependency is
test-only; tooling added one `serde_json` lock entry.

| Check | Receipt | Result |
| --- | --- | --- |
| Focused core tests | `062801Z-36296` | 57 passed; zero failed or skipped |
| Initial lint | `062908Z-37120` | Formatting failed |
| Formatter | `063048Z-38166` | Passed |
| Final lint | `063105Z-38745` | Four checks passed |
| Privacy | `063104Z-38515` | Eight files passed |

Native checks and the full gate are pending. The native proof helpers still
need the new receipt format and the same-login review/export check.

Login action; start `8f8e2b79`.

Implementer `063336Z-40689` added login from entered fields. R1 found one
P2 in failed credential removal after cancellation. Fixer `064514Z-47343`
now reports that failure and keeps explicit login retry available. R2 found
no P0-P2. The URL and login inputs are captured before normalization;
recovery bindings, cancellation and passive restoration remain checked.

| Check | Receipt | Result |
| --- | --- | --- |
| Initial Swift build | `064036Z-44170` | Test compilation failed; retained |
| Existing test with separate Save removed | `064804Z-49229` | One test failed at runtime; original login guard retained |
| Complete corrected settings suite | `064858Z-51233` | 17 passed; zero failed or skipped |
| Three-file privacy | `064858Z-51007` | Passed |

The initial test errors were a missing DTO equality conformance and actor
isolation in a test helper. Tests now compare existing fields and use
appropriate isolation. The runtime failure proves the unsaved-URL case;
the corrected suite includes retained login after restart, rejected URLs,
captured inputs and cancellation/credential-removal failures. Native proof,
the remaining UI feedback and the final integration gate are still pending.
