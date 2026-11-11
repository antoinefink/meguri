# U8 native driver foreground cycle (R1965)

Prepared fixer patch; supplied clean master base `bae0b13b`, original batch base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. Filtered source starting commit:
`68192b57350a1926b4846968d673980d4128e6e3`.

Adds only the driver's literal-true `foregroundCycle` action, its physical
payload admission, parser/control regressions and action documentation. iOS
requires foreground, presses Home, waits at most 10 seconds for either running
background state, checks again before one activation, and waits at most 10
seconds for foreground. Failed background or stopped-process paths cannot
activate. macOS reports unavailable without input. FrameSampler excludes it;
captures use foreground Meguri only, with lowercase attachment names preserving
the existing physical export guards. Original-PID and installed-artifact continuity evidence remains mandatory for
physical kept-running inspection and refuses any restart. There is no explicit
launch fallback: a preactivation running guard precedes the single activation,
but `XCUIApplication.activate()` can race a process exit and may relaunch. The
guard and bounded waits do not provide an atomic never-relaunch guarantee. No
Home/background-screen captures are exported. Recording combined with any
`foregroundCycle` key is refused at the common `mg_drive__job` entry before
discovery, lease, build, runner or recording work, including malformed action
values; JSON/parser errors are suppressed and refusal prints no payload text.
Other launch, raw-argument, route and physical recording restrictions remain in
place. Scripts and source stay frozen while jobs run.

One source consistency inspection covers action switches, physical payload/input
validators and capture/export policy. No builds, tests or phone actions executed
in this runner; compilation, focused verification, independent review and
provenance/privacy/similarity checks belong to the orchestrator. This patch may
remain parked and supplies no new evidence about the collection-sync defect.

Review/fix log: independent review `151053Z-69702`, P2-1 and P2-2. This fixer
starts from the supplied seven-file seed on filtered commit
`777a526eca64bd5e73153baf863e53f7a59a545f`; the real base remains `bae0b13b`.
P2-1 adds only the common recording guard and one negative Bats regression in
`physical_device_ui.bats`, covering CLI and direct common-entry paths on
nonphysical iPhone/Mac destinations, malformed values/extra keys, escaped action
keys, refusal before discovery/build/runner/recorder, and no sentinel leak.
P2-2 corrects the action and handoff documentation about activation and recording.
All seeded Swift and prior test lines are preserved; no production data, sync,
assertion or limit changes. One source consistency check only; no builds, tests,
other agents or native execution in this fixer. The prior 15 DriveScriptTests and
two policy tests passed under MG for the unchanged Swift/existing new action;
they do not verify this new recording guard. Its focused Bats verification and
required integration checks remain with the orchestrator after export.

## Integration checkpoint

Author `150503Z-59100`, first independent review `151053Z-69702`, fixer
`151407Z-75199`. VM `ui-test/20261111T151128Z-70587` passes all 15 parser and
control tests with required lint. Swift bytes are unchanged by the fixer.
Docker `selftest/20261111T151810Z-78559` passes all three policy tests.
Privacy `151814Z-78866` and similarity `151817Z-79146` pass for the repaired
tooling; unchanged Swift files passed `151135Z-71969` and `151131Z-71558`.

Second independent review `151758Z-78142` finds no P0/P1 and one remaining P2:
multiple JSON documents can let the recording guard see only the last result.
The P2 is unresolved. The physical script contains a single JSON object without
recording; the existing physical path independently refuses recording. This WIP
checkpoint permits that bounded diagnosis, not acceptance or a full gate. The
recording guard will be repaired while that physical comparison runs.

No grade, replacement, SDK reset or synthetic lifecycle notification is part of
this script. Neither native execution nor data convergence is verified.
