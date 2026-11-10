# Cloud command ordering — WIP

Starting commit: `cc9a43da`.
Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer session: `s-20261103T133750Z-f276f58e`.
Specs: sync/policy P4, P21; engine/jobs J2.1.1.

The filtered patch adds Core phases to SyncCoordinator. Command results still
cross the queued ACK path. Local publication and adoption work uses that phase
on its original task. RunCleanup releases its phase after local preparation and
keeps the native inventory task until its final result. Shutdown cancels command
jobs before waiting for the input barrier. No guard or admission check changed.

Four new tests cover Core result ordering, long native work with cancellation,
cleanup drain, and publication rebind. The original four native integration tests
are unchanged. Integration added the new diagnostic enum case to the existing
InitialCloudAdoptionTests switch without changing its trace or assertions.

Verification:

- Filtered export `cleanroom/20261109T235216Z-5265` passed its source checks.
- `swift-test/20261110T000652Z-9158` failed compilation: the existing test switch
  needed the new enum case. The integration change above corrects that error.
- `swift-test/20261110T000729Z-10555` compiled, then ran eight tests: all failed,
  with 22 issues. The previous original-owner rejection did not appear.
- Publication still lacks accepted control evidence. One observation reports
  `currentCollectionChanged`; the matching-conflict case reports no operation
  failure but also no Complete or done proof. These checks remain required.
- The three controlled tests record real setup errors before their fake command
  assertions. Their results do not prove the ordering assertions pass.

Independent R1 review found two P2 issues. The first is the failing handle
fixture above. The second is another P4 path: account completion
can make a second policy input before returning the first input's Core actions.
That path requires repair. It is not yet established as the cause of the missing
control evidence. Keep both failures open and diagnose them separately.

No unit, batch, milestone, signed round trip, or release is accepted. Selected
exchange ACK, peer readback, close and recovery proof remain incomplete. The held
AgentGeneratedBase test patch remains preserved outside this commit.

Privacy: all nine source paths passed `private/20261110T000904Z-12159`;
the rewritten log passed `private/20261110T000927Z-12456`.
Similarity passed `similarity/20261110T000951Z-12726`.
The full gate has not run on this WIP.


Fixer continuation: supplied clean master `bcf4e165`, same recorded session
and original batch base. Specs remain P4, P21 and J2.1.1.

Implemented source: account proof returns its actual outputs before a later
reducer turn prepares runtime and refreshes policy. Its Core commands and
follow-ups cross their original queued ACKs first. A controlled regression
holds both stages before the subsequent preparation/input marker.

Native lifecycle, exchange, flush and media chains deliver output prefixes
through the same coordinator seam. Subsequent policy inputs take reducer turns
which consume and ACK their actual Core outputs before returning the original
value. Full output lists remain available; receipts prevent duplicate execution.
Failed Core/cleanup work returns its original error before handing its phase to
the refresh continuation. Transparent backend observers retain their hooks.
Cleanup's qualified local-pass/inventory split and native concurrency remain.

The three original fake bodies are unchanged. Their fixture uses supported
Service adoption and baseline exchange with media initially disabled. Additional
handles come from actual Core settings outputs; replacement-SDK and media setup
actions are not executed to acquire them. Setup errors still fail acquisition.

The separate publication guard failure is unresolved. Test-hook diagnostics add
fixed labels only at existing CurrentCollectionChanged exits, at most two per
observation. Predicates, short-circuit order, errors and absence semantics stay
intact. No Rust schema or FFI change is made.

Verification: one source consistency inspection, with source corrections;
no git, network, builds or tests. Root owns compilation, focused verification,
independent review and provenance/privacy/similarity/batch gates. The recorded
eight-test run (all failed, 22 issues) remains the latest executable evidence.
Four original Native cases and the held-Status case remain enabled and unchanged;
the held AgentGeneratedBase patch is untouched. Selected exchange ACK,
peer-observe, close and recovery remain incomplete. No unit, signed run or
full-gate acceptance is claimed.

R2 integration verification:

- `swift-test/20261110T002519Z-53047` failed type inference in the optional
  follow-up closure mapping. Integration made the closure type explicit.
- `swift-test/20261110T002644Z-54675` compiled and ran nine tests: four passed,
  five failed with 12 issues. Account proof ordering, Core follow-up ACK ordering,
  long fetch/cancel ordering and cleanup drain all passed. The five Native cases
  remain failed and enabled. Every one reached the initial path refusal in the
  control observer. No receipt or snapshot predicate was relaxed.
- Independent R2 source review found no P0–P2 findings in the supplied repair.
  The original three fake bodies are unchanged; handle acquisition is corrected.
- The path failure is now a separate, precise next repair. It still blocks
  Native proof, unit acceptance and the signed round trip.

R2 privacy passed `private/20261110T002912Z-56562`; Rust format and
`core-lint/20261110T002916Z-56824` passed. Similarity passed
`similarity/20261110T002929Z-57105`. The path-stage diagnosis is concrete:
Host opens the engine through CollectionPaths' canonical directory, but supplies
the original directory spelling to Runtime. Aligning those admitted roots is
next; no observer predicate has been changed.
