# Mac Settings accessibility container

Start: `7245b93e9e7d40e7d053472518d1d78a57161136`.
Fixer session: `s-20261024T174115Z-511e3ad6`.
Filtered export: `cleanroom/20261024T204408Z-21606`.

Settings opened through Command-comma, but its root identifier was absent.
`ui-test/20261024T202640Z-6505` recorded two windows, all three known child
controls and no root identifier. The explicit native accessibility container
preserves its children and exposes that root. Subsequent preflights passed the
unchanged shortcut/root assertion and reached the endpoint and details controls.

Independent source review found no P0–P2 findings. UI lint and compilation
passed in `ui-test/20261024T204528Z-22479`. Privacy passed in
`private/20261024T204538Z-23470`.

The whole preflight remains failed: clicking the details label did not expand
the disclosure. Bounded facts show one enabled, hittable disclosure, a finite
label frame, collapsed state and no history children. This remains open.

Earlier visibility and window-anchor experiments did not change that result.
Their failed receipts `20261024T203443Z-14079` and `20261024T204102Z-18832`
and excluded patches remain available. Neither experiment is in this commit.
The bounded diagnostic records only fixed categories, counts and geometry.

This is WIP, without a completed Unit trailer. RF4 and the shared integration
gate from `81c207573c20c337c8834e4ae8b4ee8f29603dbf` remain unaccepted.

The next source-only capture helper is `cleanroom/20261024T205155Z-27393`.
Independent review found no P0–P2 findings. It publishes only a bounded crop
of the collapsed header, after identity, state, geometry and field checks.
`ui-test/20261024T205451Z-28859` compiled and retained the preflight failure.
The single named crop was exported and inspected. It contains the native
chevron and fixed header text only. A center click lands on the label; the
chevron lies within the leading portion of the reported control frame.
The original pointer action and assertions are still unchanged here.

## Workflow identity diagnosis

Start: `122cceb8`. Filtered diagnostic export:
`cleanroom/20261024T222521Z-85874`. Independent round 1 found no P0–P2.
Privacy, similarity, lint and native compilation passed. Run
`manual-transfer-check/20261024T222653Z-87057` failed at the unchanged
workflow-root assertion. Import and review had not started.

The failure-only attachment proves that Import opened one real app-owned
sheet: Done and Prepare Import each occur once inside it, while the workflow
root identifier occurs zero times. Export is absent, as expected. Bounded
export `workflow-facts-export/20261024T222925Z-90876` contains only fixed
identifier counts and finite geometry. The root identifier, rather than the
Import action, needs repair. Keep the original actions and assertions.

Dedicated-VM Xcode alignment and signature verification passed in
`vm-xcode-align/20261024T211341Z-40341` and
`vm-xcode-align-verify/20261024T211929Z-42687`. Runner registration was repaired
in the VM. Fresh-process trust passed before and after normal product copying
(`vm-ax-trust-frozen/20261024T213657Z-51493`,
`vm-ax-trust-frozen/20261024T214200Z-54688`). A native alert Cancel click passed
in `ui-test/20261024T214306Z-55451`. Do not repeat setup without new evidence.
The original collapsed Connection details failure remains separately open.

Workflow repair start: `23d903ea`; filtered export
`cleanroom/20261024T223058Z-92507`. The workflow now declares an explicit
accessibility container before its identifier, retaining native children.
Independent review: no P0–P2. Privacy: pass. Similarity: pass.
Native verification: pending. Shared full gate: pending. Unit acceptance: pending.

The outer-stack attempt failed in
`manual-transfer-check/20261024T223545Z-96642`: the sheet and children existed,
but the root was still absent. Filtered export
`cleanroom/20261024T224114Z-3821`, base `5b882a56`, moves the single container
and identifier to the rendered Form. Independent review: no P0–P2.
Privacy and similarity passed.

`ui-test/20261024T224236Z-4715`: one pass, one failure, zero skips. The new
native Import/Done test proves one sheet, one workflow root, the Done control
and actual dismissal. The original Settings disclosure test remains failed.
Bounded facts identify a native disclosure triangle (type 13), no child nodes,
enabled/hittable state and a collapsed value. No history control was exposed.
Pointer delivery and disclosure behavior still require diagnosis. The full
gate remains pending; no unit is accepted.

## Expanded details boundary repair

Start: `66a266da8a021edf6b77e695493a4bebc6f09905`.
Fixer session: `s-20261024T174115Z-511e3ad6`.
Final cumulative export: `cleanroom/20261025T001728Z-56616`.

The excluded pointer probe `vm-settings-frozen/20261024T235445Z-48683`
expanded the real native disclosure, then failed history reachability. Its
bounded numeric attachment showed 185 occurrences of the details identifier
and zero history identifiers, although the history action existed by label.
A single SwiftUI modifier scopes the parent ID. Native layout, state and
child controls stay intact.

The original preflight now also requires one details identifier, one history
identifier and an enabled, hittable native history button. Its single pointer
action resolves the native disclosure header within the unique container and
targets the measured chevron. VM, app identity, owning window, finite stable
geometry and collapsed state must hold before input. There is no input retry.
Existing scroll and workflow assertions remain.

Independent R1 found a missing collapsed guard and unsupported process API.
R2 reviewed the final cumulative patch and found both closed, with no P0–P2.
`build-app/20261025T002154Z-59761` passed UI lint and native compilation.
Focused privacy `private/20261025T002329Z-62077` passed.
Fresh runner authorization `vm-ax-refresh-private/20261025T002211Z-60766`
and fresh-process trust `vm-ax-trust-frozen/20261025T002326Z-61677` passed,
each with one test and no failures or skips. The temporary input was removed.
`vm-settings-frozen/20261025T002339Z-62765` passed the original preflight,
Its ID counts, button roles and scroll checks all passed: one test,
zero failures or skips. Similarity
`20261025T002334Z-62439` also passed. The broader gate and unit acceptance
remain pending.

## Prepare action investigation

Start: `a05e6aa61a6e98ea8db7ffdb42e7e0a64ef29a0a`.
The first full Mac run `manual-transfer-check/20261025T002532Z-64603`
stopped at synthetic media staging, before any UI test. A read-only probe
verified two synthetic files against saved test recipes. Their recorded
container identity differed from the current one.

A temporary VM repair helper moved only those two files into a new exclusive
test archive, verified their bytes and file identity, and synced directories
and its receipt. Review R1 found a durability gap; R2 found it closed.
`fixture-preflight-facts/20261025T002949Z-69840` records the repair.
The temporary helper was removed. The staging rules were unchanged.

`manual-transfer-check/20261025T003000Z-70265` passed staging and real login,
opened the workflow and clicked Prepare Import. The preview did not appear.
Failure-only facts retain the prior login result and no progress. This does
not distinguish a missed pointer action from an early preparation refusal.

Filtered diagnostic export `cleanroom/20261025T003800Z-75516` adds only
bounded control geometry before the existing click and after a missing
preview. It retains the existing query, input and assertions. Independent
review found no P0–P2. Build `20261025T004215Z-77257`, focused privacy and
similarity `20261025T004227Z-78392` passed. Native execution remains pending.
No unit or full gate is accepted by this investigation.
