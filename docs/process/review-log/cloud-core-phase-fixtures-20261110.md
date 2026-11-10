# Cloud fixtures at the Core phase boundary

Starting commit: `9038bda6`. Pending integration batch base:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The combined review check found fixtures waiting for an ordinary input while
holding the Core result that must finish before that input can run. The queue's
Core phase gate is required. Its eligibility and ACK ordering remain unchanged.

The existing DEBUG drain observers now report an input waiting at that gate.
This adds no Release code. Fixtures can hold the actual native result before
its ACK, admit the real account resolver before holding later status work, and
observe cancellation before releasing an original task for its later quiesce.
Native execution, issued handles and results remain real. Existing outcome
assertions and timing limits remain intact.

Antoine requested direct work without other agents. Local review checked queue
eligibility, callback isolation, original-task release, account-read admission
and cleanup paths. This is not a new independent review or unit acceptance.

## Retained runs

- `swift-test/20261110T210829Z-25762` and
  `swift-test/20261110T211413Z-27337`: cancelled after the identified fixture
  deadlocks. The original MG process handled cancellation and joined its work.
- `swift-test/20261110T211922Z-28940`: both Resume cases pass.
- `swift-test/20261110T212037Z-30386`: account-proof cases pass; two other
  fixtures fail at the same ordering boundary.
- `swift-test/20261110T212303Z-32353`: compilation fails on a captured mutable
  fixture value. An explicit MainActor fixture object corrects that error.
- `swift-test/20261110T212444Z-33744`: both corrected close and retained
  publication cases pass.
- `swift-test/20261110T212548Z-35190`: all 29 selected tests pass together,
  including the real Core review and media Service round trip, fresh adoption,
  reopen, Resume, account proof, publication and close.
- `private/20261110T212554Z-36727` and
  `similarity/20261110T212553Z-36385`: the three changed Swift files pass.

The Service case uses the real Core and production review/coordinator code,
with a local service replacing the cloud network. It checks durable card and
review-log rows and media bytes after reopen and fresh adoption. It does not
prove Apple SDK transport, signed devices or visual rendering.

Earlier failures remain failures. The full batch gate, independent review and
real signed iCloud round trip remain pending. No Unit trailer is added.

## Broader clean-tree check

`swift-test/20261110T213105Z-38198` ran all MeguriKit tests on clean `fc15bcc2`:
790 pass and 6 fail out of 796, with no skips. The 29-test checkpoint passes in
that run too. The broader run stays failed.

One failing edit fixture generated its baseline with the system clock, then
opened it with the Service harness's fixed 2025 clock. Its real updater therefore
moved the note's timestamp backward. Starting at `fc15bcc2`, that fixture now
uses `.system`, as the existing test instructions require. No timestamp or
revision comparison changes. `swift-test/20261110T213734Z-40331` passes the
corrected edit test, including close/reopen and exact committed readback.

Four failures share an actual `requestSend` refusal during automatic-scheduling
reconfiguration. Retained traces show an older send and later
`ConfigureAutomatic` overlapping, with `connection.authorityChanged` at native
invocation admission or reporting. Publication then lacks its required reply.
These are unresolved; they are not dismissed as fixture errors or passing sync.
The affected cases are publication with peer adoption, the later-rewrite refusal,
published-edit restoration, and adopted-phone restoration. Diagnose that overlap
with a controlled original-command hold before changing its production behavior.

The sixth failure is the replay-control test's unavailable native AX child tree.
Its cause is not established. The renderer also reported its already-declared
known WebKit memory issue. Neither check was suppressed or changed. The full
batch gate and real signed round trip remain pending.
