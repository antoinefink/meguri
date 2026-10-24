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
