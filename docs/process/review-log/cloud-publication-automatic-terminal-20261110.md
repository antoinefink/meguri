# Publication automatic scheduling — WIP

Start: `8e8e1ec2`. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer: `s-20261103T133750Z-f276f58e`. Contract: sync/policy P4/P6.

Driver now processes actual binding changes before another projected event
consumes the edge. This supplies the required automatic-off command after
publication while retaining the original trigger and guard checks.
`core-test/20261110T063433Z-80134`: all 69 policy/publication tests passed,
none skipped. Driver tests use actual Runtime publication and issued commands.

Independent R1 review found an explicit tracking-failure ordering gap. That
failure must be installed before the new Binding step, exactly once. The repair
and its regression remain open. Native factory verification is also pending.
No unit, gate or signed acceptance is claimed.

`core-lint/20261110T063555Z-82265` passed.
