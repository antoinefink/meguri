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
