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

Native execution of the repaired ordinary lane remains pending. The separate
ordinary Mac signing problem also remains open; neither is treated as fixed
by routing self-tests. Dedicated signed runtime acceptance remains required.
