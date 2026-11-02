# Real cloud inputs for VM Unit checks

Status: independently reviewed WIP; focused tests and VM execution are unrun.
Prepared source base: 60b8cc90. Integration start: 87775197.
Batch base: 1416a520542a8c5daa38a23bfe398da704c3d6c9.
Fixer session: s-20261025T162405Z-9dc656e4.

The failed full gate gate/20261102T075235Z-35258 includes 33 Mac Unit
failures. Thirty-two failed methods lacked the real Rust fixture or Service
executable that the package runner already creates. Those failures occurred
before their cloud behavior; they do not reproduce the original U114 race.
The separate NoCollection unavailable failure remains unexplained.

This component reuses the existing real producers, retains source and file
authority, copies the exact inputs into the VM, and checks the readback. Only
the Mac Unit invocation receives its checked TEST_RUNNER inputs. Other test
lanes clear them. This supplies no signed-agent or account evidence.

Prepared exports: cleanroom/20261102T100954Z-11229 and the final
cleanroom/20261102T103526Z-43044.
Independent reviewer: native_bounds_review. Round 1 found a P2: a producer
failure stopped unrelated device checks. The repair records that failure,
clears partial inputs, refuses the affected Mac Unit launch, and still runs
independent Snapshot, UI, simulator and DriveScript checks. Round 2 found no
remaining required finding. Eight other seed files remain unchanged.

Generated tooling tests cover the producer receipts, strict current-run scope,
retained file authority, exact guest copies, input propagation and early failure.
Focused tooling execution and actual VM execution remain pending disk recovery.
No test selector, assertion, limit, privacy exception or production Swift/Rust
behavior changes. No Unit trailer or passing integration gate is claimed.

One internal action/function name was replaced after a privacy check. This is
a consistent rename across its call sites. No scanner rule changes.
