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
