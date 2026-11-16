# Ordinary Snapshot repairs

Repair batch start: `d527b7e13ef805e1a6196bf29c3fa04549dbf5ff`.
The pending integration gate keeps base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
No unit or full-gate acceptance is claimed here.

## Method selection

The retained Mac result in `snapshots/20261116T033204Z-55003` confirms three
dedicated Swift Testing runtime methods failed in the ordinary Snapshot run.
Its native input-validation method passed. The excluded XCTest account-setup
suite had no executed cases. The Snapshot plan already names the dedicated
suites in its exclusions; this repair reinforces that existing boundary.

Filtered author `cleanroom/20261116T184556Z-23731` added exact method-level
CLI exclusions in the ordinary Snapshot helper: the signed soak method on
both platforms, and the two Mac SDK observations on Mac only. Existing
only-testing selections and the input-validation method remain intact.
Result-only calls and dedicated controllers keep their existing selections.
No app source, plan, assertion, deadline or result policy changed.

Independent filtered review `cleanroom/20261116T185059Z-80598` found no
P0–P2 findings. Its exported patch exactly matches the author's patch.

Focused checks on the applied source:

- `selftest/20261116T185242Z-84533`: 17 passed, zero failed or skipped.
- `private/20261116T185243Z-85156`: all three files passed the unchanged guard.
- `static-checks/20261116T185241Z-84227`: three passed.
- `similarity/20261116T185242Z-84610`: pass; zero eligible files, three shell
  files out of scope. The filtered implementer report was read.

The unchanged native input-validation method then passed in
`mac-hosted-test/20261116T185700Z-87337` on clean `f03cc280e`: exactly one
execution, zero failures or skips. This selected result verifies the
result-only path, not ordinary snapshot selection or live SDK behavior.

## Mac signing

This source step started at `f03cc280e`. While its filtered export was
retained, the separately verified server batch merged as `480df4526`.
The candidate still applied; the reviewed final contents of all five owned
files were checked against the retained export on that merged baseline.

Filtered author `cleanroom/20261116T185624Z-86740` selects team signing only
for the ordinary Mac Snapshot build and admits exactly that producer to the
existing app-only profile scope. It keeps a local binding and checks it
after building and before shared VM delivery. Missing authority, changed
profiles and build failures remain failures. No credential test, app source,
entitlement, profile, signing validator or global gate signing mode changed.

Independent filtered review `cleanroom/20261116T192622Z-80307` found no
P0–P2 findings. Focused checks on the applied source:

- `selftest/20261116T193010Z-81722`: 46 passed, zero failed or skipped.
- `selftest/20261116T193324Z-83619`: the 15 remaining affected existing
  cases passed, zero failed or skipped.
- `private/20261116T193011Z-82059`: all five files passed the unchanged guard.
- `static-checks/20261116T193009Z-81033`: three passed.
- `similarity/20261116T193009Z-81186`: pass; zero eligible files, five
  files out of scope. The filtered implementer report was read.

The earlier supplementary filter run `selftest/20261116T193147Z-82582`
selected zero cases and supplies no verification evidence. The corrected
selector above required all 15 cases. Empty filtered self-test selection
should be refused by the runner; that separate guard remains to be added.

## Ordinary native follow-up

The ordinary selected Mac run `snapshots/20261116T193604Z-86176` ran on
clean `d8d94115b`. It did not require a global team-signing override.
Its native result contains 138 passed, two failed and two skipped tests.
The unchanged credential integration method and native input-validation
method both passed. The three dedicated runtime methods have no executed
case in that result. This verifies the repaired ordinary selection and
credential authority together.

The job remains failed: Mac settings-search native identity and native
replay accessibility still fail. Its two rendered images pass; the MG
summary counts those images, not the native test cases. No baseline image
was recorded or replaced. This selected Mac run does not clear the full
snapshot matrix, integration gate or dedicated signed runtime acceptance.

## Empty self-test selection

This follow-up started at clean `d8d94115b`. The retained zero-case result
`selftest/20261116T193147Z-82582` exposed a runner defect: a filter selecting
no cases could report success. Filtered author
`cleanroom/20261116T194010Z-90318` adds a refusal only to selftest's formerly
successful zero-case branch. Counts, skipped cases, runner failures,
incomplete TAP and other jobs retain their behavior.

Independent filtered review `cleanroom/20261116T194851Z-92055` found no
P0–P2 findings; its patch exactly matches the author export. On applied
source, `selftest/20261116T195033Z-94454` passes all six selected regressions
with zero skips. The controlled producer exercises counts, selectors and
failure categories through the real MG boundary. Two actual Bats calls
also verify the original failure mode:

- `selftest/20261116T195112Z-95975`: a real no-match selector is refused
  with `no-tests`, zero counts and nonzero exit.
- `selftest/20261116T195117Z-96337`: an unsupported selector is refused
  with the same category, counts and exit.

Those two failed receipts are expected negative checks; they are not
passing test executions. Privacy `20261116T195052Z-94848` passes both source
files, static checks `20261116T195055Z-95184` pass all three checks, and
similarity `20261116T195100Z-95566` reports zero eligible files with both
tooling files out of scope. The filtered implementer view was read.

## Native settings-search reader

The source step began at `9388a64ef`. Diagnostic native run
`mac-hosted-test/20261116T200342Z-9600` ruled out CF accessibility elements
for the opaque native children. The first legacy observation failed to build
in `20261116T201208Z-19694` because typed deprecated selectors met the
existing compiler error policy. Only their public selector references were
corrected. Native run `20261116T211238Z-77355` then observed actual button
role, matching native description and a known static identifier through
AppKit's public attribute getter. The same objects lack the newer getters.
The original reader therefore omitted a real native button. No production
metadata defect was established.

Herdr delegate `t1dq` prepared the bounded repair. Its first restricted
Docker probe refused before authoring (`cleanroom/20261116T211649Z-35277`).
The same prompt then ran through the authorized host Docker route as
`cleanroom/20261116T211902Z-38856`. The delegate inspected the export;
independent filtered review `20261116T212155Z-41414` found no P0–P2
findings, and its exported patch exactly matches the author's.

Only the existing test reader changes. It reads the actual public legacy
attribute when a corresponding modern getter is unsupported. Supported
modern nil, empty or wrong-type results remain unchanged. Exact labels,
identifiers, uniqueness, same-node checks, both remounts, geometry ownership
and retained text remain asserted. Expanded diagnostic traversal is removed.
Production controls, compiler checks, deadlines and plans are unchanged.

On applied source, privacy `20261116T212313Z-42627` passes the file, static
checks `20261116T212314Z-42623` pass all three checks, and similarity
`20261116T212317Z-43331` passes one eligible file with zero flags or
exceptions. The filtered implementer view was read.

Native verification `mac-hosted-test/20261116T212345Z-44063` on clean
`5b6a12759` passes exactly one test with zero failures or skips, including
both remounts and every original identity/text assertion. This is a focused
repair pass. Native replay, the complete Snapshot matrix and the original
integration gate remain incomplete.

## Bounded replay observation

This remains diagnostic work, not a native replay repair. Herdr delegate
`t1dr` supplied the public-API audit and bounded author prompt. Filtered
export `cleanroom/20261116T212400Z-45925` adds one first-question native
subtree observation after the original CF attempt, including failure unwind.
A source review caught direct deprecated calls before compilation; seed
correction `20261116T213145Z-52067` changes only those six calls to guarded
public selector reads. Independent complete-candidate review
`20261116T213531Z-55716` found no P0–P2 findings; its exported patch matches.

The original CF error and every assertion, identity check, deadline and
native activation remain. Actual returned native values stay in memory;
logs use fixed classifications. Nil children stop the extra traversal as
incomplete. An early leaf can end the walk before the other links. This
observation cannot establish playback, a repair or full convergence.

Normal privacy run `20261116T213854Z-58910` flagged the candidate and is
retained. Under `provenance.md#routine-privacy-admissions`, independent
reviewer `incoming_contract_review` read all 393 lines and 23,768 bytes,
including comments, literals, graphics, paths and output. It classified the
complete file as public operational test code and synthetic fixtures, with
no actual private values or new licensing concern. Its provenance basis
includes the filtered authors/review and the original Meguri graphic in
`render/display-transform#D4`.

The reviewed path is
`app/MeguriSnapshotTests/NativeReplayAccessibilityTests.swift`; its SHA-256
is `48452d45977876bb07b2a55deb8197ef2c0693ff35661bec2e301ca0bee607ac`.
The integrator checked those exact bytes and added only that path/digest to
the existing public-file table. Matcher logic, thresholds, tokens and
private-data access are unchanged; future bytes require a fresh check.

Scanner regressions `selftest/20261116T214235Z-61985` pass their selected
case with zero skips. Privacy `20261116T214239Z-62333` passes both files;
static checks `20261116T214239Z-62332` pass all three checks. Similarity
`20261116T214256Z-63448` passes both eligible files with zero flags or
exceptions, and its filtered implementer report was read.

Native run `mac-hosted-test/20261116T214628Z-67510` on clean `aa17e03e`
compiles and executes one failed test with zero skips. The original CF error
remains. The extra walk reads one returned native object; its public legacy
getter is available but returns nil role, identifiers and children. The walk
stops as incomplete before reaching links. This result supplies no complete
native tree or causal repair. This bounded diagnostic branch is stopped;
required identity, names and all native activations remain unverified.

## Affected iPhone simulator follow-up

Selected Snapshot run `20261116T220046Z-78844` passes on clean `cf61cbc9`.
It verifies four `home.deckList/typical/*` images across the two phone sizes.
The retained native bundles report:

| Device group and phase | Passed | Expected failure | Failed | Skipped |
|---|---:|---:|---:|---:|
| iPhone plan | 3 | 0 | 0 | 0 |
| iPhone render workload | 113 | 3 | 0 | 2 |
| Small iPhone plan | 2 | 0 | 0 | 1 |
| Small iPhone render workload | 104 | 0 | 0 | 14 |

The three expected-failure method groups belong to `HostedSnapshotTests`:
token-color difference reporting, missing-baseline reporting, and startup
I/O error reporting. The integrator checked their actual results and existing
`withKnownIssue` blocks. These are intentional negative harness checks,
not a product-failure waiver. All skipped results remain retained.

This selected render workload does not replace the complete Snapshot or
Xcode matrix, prove live CloudKit exchange, or close the original gate.
No baseline was replaced.

## Affected iPad simulator follow-up

Selected Snapshot run `20261116T220938Z-87949` passes on clean `92026772`.
Its four selected `home.deckList/typical/*` images pass across both iPad
sizes, with no baseline replacement. The MG count describes images; it
must not be read as a native-test count. The full matrix remains pending.

The required iPad accessibility run `a11y/20261116T221528Z-93150` also
passes on clean `92026772`: all 12 screen checks pass, with zero failures
or skips. This verifies that platform's current required audit workload;
it does not clear the other failed jobs or the integration gate.
