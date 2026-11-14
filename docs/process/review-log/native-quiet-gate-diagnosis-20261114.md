# Native quiet-point diagnosis

Source: `794ee3ba`. Role: fixer, session `s-20261110T214025Z-20fa15fd`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This is diagnosis, not a repair round or acceptance.

## Evidence

The unchanged native lifecycle test runs against a synthetic service with the
real bridge and core. Baseline `swift-test/20261114T024532Z-73394` retains the
quiet-point failure in its close/reopen case. Its replacement case also fails
the owned-cancellation checks with `cloudPolicy.generationChanged`. Neither
failure is waived or classified as flaky.

Temporary trace `cleanroom/20261114T024506Z-72888` exports four paths. Root
rejects its optional Swift printouts, leaving the original test body unchanged.
The remaining three-file Rust trace passes privacy in
`private/20261114T025249Z-75759`. It records scalar facts at the actual failed
gate while holding the same Runtime lock. Publication query results come from
their original evaluations, including whether short-circuiting skipped one.
It changes no guard, return, effect, assertion, limit or production API.

Diagnostic `swift-test/20261114T025312Z-76011` retains both failures. Its one
failed-gate record occurs in the serialized close/reopen case:

| Fact | Observed |
| --- | --- |
| Binding present, usable and verified; crt equal | true |
| Publication hold | evaluated, false |
| Whole scan complete | true |
| Scan required/running, tracking stopped, adapter paused, incoming running | false |
| Known loss, durable loss, epoch verification | absent |
| Root confirmed | false |
| Publication permits sending | evaluated, false |

These facts select the original `gate_locked` branch
`!root_confirmed && !publication_allows_send`, which returns `Incomplete`.
The generic FFI error hides that distinction. `RuntimeStatus.incomplete` is a
constant and was not used to identify this failed branch.

The trace's ordinal and fixture time can repeat across Runtime instances. This
is direct evidence of the rejected call's conditions, not a global command or
signed SDK ownership proof. It does not establish why confirmation was late.

## Remaining work

The policy quiet predicate does not require root confirmation. The driver feeds
drains into policy before its `RootConfirmed` input. Independent read-only
interpretation `cleanroom/20261114T025754Z-77977` supports a separate gate for
quiet metadata under the approved contract:

- P6 lists the general gate conditions without root confirmation. P14.3 requires
  the quiet write once for a qualifying drain; P19 stores its instant.
- K12.1 rule 3 and V10.2 impose root confirmation on collection sends. V26.1
  permits a root record read after a drain that did not confirm the epoch.
- The stamp records a quiet drain. It grants no send authority, proves no
  delivery to another device and cannot replace P12 success accounting.

The repair must keep current ownership, identity, reconciliation, loss,
verification and freshness checks, while keeping the strict physical-send
gate. Adding a new root prerequisite to P14.3 or silently consuming its valid
candidate is not supported. Tests must prove a durable stamp in the observed
state, no collection batch granted, stale candidates rejected and real write
failures retained. No behavior or specification has changed in this diagnosis.

Root reverses the exact temporary Rust patch after the diagnostic run; no trace
code remains on master. The normal signed preview remains the earlier verified
`edd07eed` product. The separate U113 round-limit decision and its two P1s
remain open. No Unit, full gate, milestone or release claim is made.
