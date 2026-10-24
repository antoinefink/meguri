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
