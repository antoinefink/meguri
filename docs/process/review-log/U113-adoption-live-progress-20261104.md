# Live adoption evidence

Status: WIP; no unit acceptance.
Unit start: 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a.
Component start: 3e74305a.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer: s-20261103T133750Z-f276f58e.

Signed Mac build 46777 and physical launch 53711 use the same clean source.
Mac status: Syncing Media, with pending files decreasing.
Media completion and two-device convergence remain unverified.

Physical drive 59000 passes seven steps, then fails its unchanged 300s wait for
initial download to finish. Preserve mode leaves that attempt running.
Read-only drive 65026 still observes download. Later drive 71550 observes a
tracking failure at adoptionPageFailureResult. These observation passes do not
turn the failed download into a pass. No phone collection is accepted.

The existing stage tag resets on each work call and has no cumulative count.
The generic follow-up error also replaces the original diagnostic in the
single latest-value display. Export 66873 adds bounded, memory-only DEBUG
counters scoped to one adoption session. Native 69267 finds an actor-isolation
compile error; export 71809 captures the Sendable session outside the closure.
Reset stays after successful confirmation in the existing Maintenance call.

Native 74952 compiles and runs 23 tests; 22 pass. Its saturation test compares
UInt64 data against an integer inferred as Int by the assertion macro.
Export 77278 makes that expected type explicit without changing the bound.
It also retains the first exact typed job failure within the same diagnostic
session, so later generic failures cannot erase its stage and categories.
Late sessions are rejected; confirmation and validated attachment clear the
counters and retained failure together. Only existing DEBUG accessibility
values expose these transport counts and fixed categories. No cursor, record
identity, content, raw error, timing limit or production behavior is changed.

Source inspection finds no established cursor loop. It does find repeated
recursive staging-size checks and media-wide ledger traversal. Those costs
are not yet attributed to the live delay; no performance repair is claimed.

Independent review finds no P0–P2 issue in the final candidate, including exact
job/session ownership, both drain paths, observer reentry and Release parity.
Native 78882 passes all 45 selected probe and diagnostic tests. The five source
files pass the private-output scan. The earlier failed receipts stay failed.
A new signed observation, the batch gate and actual review/media convergence
remain required.
