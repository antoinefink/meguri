# U151 retained publication at startup

Status: component WIP; no unit acceptance.
Component start:b1a62f3e724d201bb386b25ed6a3b613ed234ac6.
Integration base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Implementer:s-20261103T064254Z-24574bf2.
Fixer:s-20261103T084239Z-e23dce75.

V15.3 requires an active publication to continue after restart with matching
account proof. Startup did not read recoveryStatus, so the existing Resume UI
was absent and persisted callbacks could not drive its own work. Export97674
adds attachment discovery and a separate intent for one start after successful
ordered proof. It reuses the original operation and executor.

Tests-only RED swift-test6166 compiled: two test functions failed with12 issues;
the no-recovery case passed. With production applied, swift-test9403 compiled:
existing presentation cases and failed/stale/replaced/paused/disabled proof
cases passed. The positive reopen case timed out at10 seconds. Its failing
wait was not identified. No passing relaunch claim is made.

R1 requires two repairs. A paused startup must continue after fresh proof when
the user clears pause. Every attached owner must observe platform signals.
The live owner reported observation enabled with no monitor, task, cached path
or policy error. Source inspection found a stale initial factory followed by
a successor could produce that state; the live event order was not traced.

R1 repair preserves fresh account proof, current host ownership, original
operation identity and all10-second test limits. It must cover unpause and
obsolete-first-factory ordering. Test diagnostics must identify the positive
wait before another production change. No native adapter, Resume binding or
SDK ownership repair is authorized by this component; U114's cap remains.

Discovery-await close and suspension races remain a coverage limit. Full
integration, signed publication, media convergence and remote adoption remain
unproved.

## Repair evidence

Export12504 addresses both P2s. R2 source review is clear. The observer test
checks stale factory exclusion, one start across successors and joined drain.
The paused test now holds a new proof after unpause before allowing work.

Swift16295 stopped at compilation: exact command identity needs testable
CoreBridge access. Privacy17938 flagged one new comment. Export18451 changes
only that import and diagnostic/comment prose. R3 review found no P0–P2s.
Privacy20540 passes all4 component files.

Swift19482 compiled. Observer and presentation cases pass, as do the negative
proof cases and empty recovery. Reopen and unpause both expire at their zone
creation wait. The10-second limit stays. Actual completion diagnostics are
needed to select the next repair. This component has no acceptance.

Lint21650 passes145 files. Similarity21791 passes3 files with no flags or
exemptions.
