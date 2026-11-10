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
