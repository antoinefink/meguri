# Publication completion — WIP

Start: `f1a0d33a`. Batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer session: `s-20261103T133750Z-f276f58e`.
Contracts: sync/cloudkit-recovery V15/V26.1, sync/policy P4/P6,
engine/jobs J2.1.1, conformance/cloudkit-agent J3.2.

Startup and original completion:

- Trace `swift-test/20261110T004352Z-5296` proves startup changes failure false
  to true before publication, with backendReplacement/bridge.unavailable.
- Initial sync starts from completeOpenedHost. Lifecycle events may update
  generation before that call, without constructing a backend. Closing ends
  this admission; another open must establish it again. Existing replacement
  and error handling is unchanged.
- The five Native cases now wait on buffered original Core-result observations:
  doneActive for completion, or the processed Complete conflict in its negative
  case. Final control/ownership assertions and the ten-second bound remain.
- `swift-test/20261110T005331Z-8387`: ten cases, five pass and five fail.
  Ready-before-open-return and all four ordering cases pass. Actual startup is
  healthy; Native cases still time out waiting for completion.

SDK model and execution:

- `swift-test/20261110T005822Z-10036`: timeout at zones, 40 rows with four
  marked/behind, zero in flight, zero inbox, no media backlog and valid scan.
  Review finds ServiceSDK ignores automatic scheduling, unlike the real SDK.
- The SDK model now uses captured mode, actual subsequent Core batches and
  Service replies, an owned detached lifetime, one network slot and pending
  revision checks. Production scheduling policy is unchanged.
- `swift-test/20261110T011704Z-18659`: ten cases, five pass and five fail with
  18 issues. Publication reaches original Complete/done and proven controls.
  Queued actions reject a changed policy token; peer adoption also fails later.
- `swift-test/20261110T012057Z-20450`: exact new SDK test names, two functions
  with four parameter cases; failed with 13 issues. Unexpected send/media
  cancellation results and the no-further-batch assertion need repair.
  The earlier filter ran only the original ten cases.

Required next repairs:

- SDK R1: an addition during the terminal empty pass can lose its wake. Preserve
  newer actual updates, stop on empty work/errors and keep owned drains.
- Policy: publication_done deletes the cloud_publication_ops entry. Runtime
  emits publish_step=None. binding_progress must cover Complete to None for
  this unchanged binding. Ownership changes must still invalidate commands.
- Peer page: the helper defaults to 200 while adoption and Apple's transport
  allow 32. The current page-level diagnostic has not proved the exact count.
  Trace `swift-test/20261110T012408Z-21888` fails at the source token check
  before reaching the new aggregate page-counter observation.

The startup repair has a clear independent review. The SDK model still has its
R1 finding and failed tests. Preserve every assertion, deadline and receipt.
No unit, batch gate, signed Mac/iPhone round trip or release is accepted.
Selected close and interrupted-operation recovery remain incomplete.
