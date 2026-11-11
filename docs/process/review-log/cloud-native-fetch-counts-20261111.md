# Original fetch counts — 2026-11-11

Role: fixer. Starting commit: `f5ff71b9`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/policy#P12`, `sync/cloudkit-recovery#V3`,
`sync/cloudkit-records#K4`. Diagnostic work; no unit or sync acceptance.

The controlled device test remains red: three Mac changes were selected in a
successful exchange, but the phone's next exchange left its actual review count
at 43. Policy `incomingWaiting` counts blocked inbox entries; it excludes ready
entries. Zero waiting alone does not prove that the inbox is empty.

This Debug-only patch counts original native fetch starts, registered collection
zone starts, collection/root modifications before conversion, native fetched
refusals and failed zone results. It also counts collection records after the
original Core intake returns successfully. Values are synchronized, bounded
process totals across all instances. They retain only scalar counts, saturate
with an explicit overflow flag, and prove no current-owner or convergence claim.
No SDK call, task, observer, retry or persistence was added. Release and visible
UI behavior stay unchanged. The existing Debug accessibility value places these
counts before the policy snapshot and failure text.

Author `cleanroom/20261111T140533Z-53671` exports seven files. Independent review
`141043Z-62934` finds no P0–P2. Focused native tests
`swift-test/20261111T141115Z-64804` pass 45 cases in three suites, including
classification, overflow, concurrency and prefix checks; required lint passes.
Privacy `private/20261111T141120Z-65712` and similarity
`similarity/20261111T141124Z-66101` pass. Original assertions remain enabled.

The next native run must compare callback counts with successful Core intake.
These totals can include callbacks awaiting their serial turn. A zero Core
count read before callback completion cannot establish a dropped delivery.
Native runs, broader integration checks and the cross-device result remain open.
