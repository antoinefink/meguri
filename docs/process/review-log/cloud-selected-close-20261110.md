# Selected close — WIP

Start: `13526851`. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Implementer: `s-20261109T210639Z-47d06d5b`.
Contract: conformance/cloudkit-agent J3.1/J3.2/J4.

Selected close uses the existing coordinator drain, coherent integrity check
and owner close. It requires the prior source ACK or peer apply proof and its
original generation. Common join checks retain original operation/callback
history. Smoke still requires its held review images. Restore/recover remain
explicitly missing.

Six ledger tests are authored. The Service close scenario in the original
handoff was prose only and is not implemented. Independent review found no
code finding but requires that real lifetime/close integration test. Ordinary
review close and SDK cancellation tests are separate evidence.

Compilation and apphook execution remain pending. No unit, gate, signed sync
or release acceptance is claimed.

`unit-sim/20261110T062053Z-60571`: iPhone build passed, execution failed before
any test because this target is absent from the Unit plan. It proves no ledger
case. The dedicated Snapshot-plan Mac job is now used for execution.
