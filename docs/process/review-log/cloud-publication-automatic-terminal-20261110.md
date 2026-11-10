# Publication automatic scheduling — 2026-11-10

Start: `8e8e1ec2`; R2 start: `57c9dfd2`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261103T133750Z-f276f58e`. Contract: sync/policy P4/P6.

Driver processes actual binding changes before another projected event consumes
the edge. This supplies the required automatic-off command after publication.
Explicit tracking failure is installed once before Binding, with its actual
no_space value. Original triggers, outputs and guard rules remain intact.

R1 found the explicit failure ordering gap. R2 source review is clear.
`core-test/20261110T083716Z-87097`: all 71 policy/publication tests passed,
none skipped. Earlier 69-case run `80134` and lint `82265` are retained.
Both new Host cases use real publication progress before the failure input.

`swift-test/20261110T063812Z-83211` still failed the native edit/peer matrix.
The former timestamp and automatic-on checks were passed before later send
errors. The first error is currently flattened to bridge; its exact type remains
unproved. No signed exchange, unit, full gate or release acceptance is claimed.

Final Rust lint `88946`, source privacy `87366` and similarity `87894` passed
(2026-11-10).
