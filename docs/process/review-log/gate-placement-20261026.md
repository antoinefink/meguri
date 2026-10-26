# Gate placement repair

Starting commit: `e43b1c17`. Recorded before work in
`build/unit-starts/gate-placement-20261026.json`.
The pending integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Gate plan `20261026T034736Z-41386` errors before scheduling: the route run
declares an oracle dependency but only host placement. The actual host wrapper
also builds/runs the oracle image and Docker namespaces, and calls the native
CLI. Its placement is corrected to `[host, docker:oracle]`, matching existing
mixed-placement rules. No job, dependency, trigger, guard, route implementation
or test is removed or weakened.

Independent review is clear. Gate plan `20261026T034835Z-42409` now passes
manifest validation with every required run retained. This proves the plan can
be scheduled, not that its checks or the route lifecycle pass. The full gate
and all existing technical gaps remain open. No unit is accepted by this fix.
