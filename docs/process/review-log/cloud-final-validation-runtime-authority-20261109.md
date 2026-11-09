# Narrow Runtime authority mirror

- Role: fixer. Requested integration start `003bcaf8`; filtered runner snapshot
  `08e2d6cd70ef4391cd780efe777f7f0825709a6a`.
- Open batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
  U113/U114/U151 remain unaccepted. Authority: engine/jobs J2.1.1,
  sync/policy P4, sync/cloudkit-records K10.2 and the independently reviewed
  narrow internal design supplied with this fix.
- Final native validation reads one exact Runtime mirror between compact Driver
  checks. Ordinary mutation validation retains its original State predicates.
  State guard Drop publishes actual facts before unlocking; Condvar waits keep
  the same atomic unlock/wait/reacquire protocol. Shared writer/Running aliases
  use the same publishing guard; running-only changes grant no authority.
- Explicit publications cover actual invalidate/incarnation/unbind, account
  isolation and verification before later persistence/pending work, SDK stopping,
  postcommit pending-loss unusability before readbacks, and adoption-ready
  unusability before next work. Static helpers use the mirror retained by State.
- Post-isolation readback failure keeps the original Tracking error and leaves
  SDK stopping at its original later position, while committed isolation remains
  unusable and unverified. A Runtime-local test-only readback failure avoids a
  new global faultpoint or fault-catalog contract.
- Added stable-State caller/SDK error-equivalence checks across real lifecycle,
  SDK replacement/release, no-ops, same-account, pause, identity, invalidation,
  stop and unbound cases. Positive held-edge tests cover account isolation,
  postcommit SDK isolation/readback (success and error), and ready adoption.
  Scoped release precedes joins; completion bounds stay five seconds. New
  bounded tests reserve all nextest threads.
- Existing enabled Runtime-held regression49664, Driver-held regression and
  their limits/assertions/teardown/reservations are untouched.
- One source consistency pass completed; whitespace diff check passed. No
  compilation, test execution or gate run in this filtered runner. Integrator
  owns formatting, compilation, focused tests, independent review and
  provenance/privacy/similarity verification.
- This is bounded final validation only. It does not establish native Started,
  full SDK closure, instruction atomicity or V3.4 stop-before-isolation.

## First integration

Independent review of export `57877` finds no P0–P2 source issue. Privacy
`67356` passes. Core `67076` stops at compilation: the new test-hook path
in runtime.rs has one extra parent module. No focused test ran. Preserve
this failed receipt; repair compilation before claiming any behavioral pass.
