# Bounded journal and observer payload access

`storage/journal` J1-J4 and `engine/change-stream` S2.1-S2.3 describe Meguri-owned
transaction storage and in-core delivery. No Anki source, oracle behavior or
private collection was used. No scenario, golden, protocol or pin changes.

Own-source intake: `/tmp/meguri-u107-bounded-payload-audit.md`, current O3/O4,
change-stream S2, undo M1/M8, and the frozen Meguri implementation at
`build/mg/cleanroom/20261025T162508Z-58172/source/core`.
The audit traces before-image spilling, resident after-images, observer row
copies and queued review copies to one journal ownership/access seam.

R1851-R1853 retain their current limits. The 48 MiB assertion applies only
to the existing note journal test, pending U245. The 150k sync tests check
time and durability, without measuring memory. PeakAlloc and the process
memory sampler have separate meanings. The new focused proof must measure
card after-images and actual consumers, not reuse those passes as evidence.

Session: s-20261025T132312Z-c047b13c
Unit context: U107; batch base 1416a520542a8c5daa38a23bfe398da704c3d6c9.
This note carries no unit acceptance or completed measurement claim.
