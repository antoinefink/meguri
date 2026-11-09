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

Export `68039` corrects that module path. Core `68899` compiles and runs
108 checks: 107 pass, including both unchanged held-lock regressions. One
new lifecycle fixture reaches an SDK account event after resume while its
binding is unverified and fails with IdentityUnverified. Mirror/state
comparison passes before that event. Fixture setup review remains open;
no behavioral or unit acceptance is claimed.

## Verified repair

Independent review confirms the fixture mismatch. Export `70422` adds two
pre-proof IdentityUnverified assertions, uses the existing scoped provider
through refresh_identity, and preserves the original SDK event and every
success assertion. It changes no production code or timing limit.

Core `71254` passes all 108 selected checks, including both unchanged
held-lock regressions and the durable isolation/readback cases. Lint `71531`
and similarity pass. Earlier compilation and fixture failures remain retained.
This completes the reviewed validation component only. Started reporting,
full native invocation, V3.4 workflow evidence and batch acceptance remain open.

## Native follow-up

XCFramework `73119` passes. Swift `86199` passes 26 checks in the execution
lane, retained network-operation and native-engine suites. It uses the same
Rust products with the test-only replay import pending; it is focused
component evidence, not a clean full gate. Signed phone preparation `88332`
passes. Phone status drive `90160` completes all five steps but fails its
hierarchy check. The status still reports missing media. No full sync or
unit acceptance is claimed.
