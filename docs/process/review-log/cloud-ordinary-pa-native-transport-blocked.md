# Native payload-file transport preparation

Source start: `6c3105f80abefc0d6fcec9bb305ec4a6be1ccdb2`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Sessions: `s-20261025T134357Z-235af9a2` (implementer),
`s-20261025T162405Z-9dc656e4` (fixer).

Authority: cloudkit-records K3, K5, K7, K13.1, K20; policy P11.2.
Exports: `20261031T141400Z-98262`, `20261031T142227Z-3873`,
`20261031T142914Z-9624`; prerequisite tests `20261030T145855Z-60887`.

Native MGEntity pa conversion retains copied files through callback delivery.
MGActive/MGEpoch remain e-only; MGBlob a is separate. Aggregate staging stays
bounded: earlier copies survive, later records retain metadata without a file.
Source open/read errors are typed. Destination errors, cancellation and free-space
failures still stop delivery; failed copies release their files and reservations.

Independent review: first two P2 findings repaired. R3 has no P0/P1. Remaining
source-shape P2 repaired in R5 source: empty, nonregular, oversized and growing
SDK sources use typed source failures; projection retains e/system fields and
forwards nil pa. Destination, capacity, cancellation and identity failures stay
strict. Limits are unchanged. The existing real SDK-source fixture now also
covers zero bytes, a directory and an oversized sparse file, with all earlier
assertions retained. Rephrased the indicated comments.

Verification:

- `core-test/20261031T141915Z-99685`: all four FFI prerequisites fail against
  the unimplemented core builder/intake/cursor path. They remain enabled.
- `swift-test/20261031T141948Z-534`: 30 tests, one new upload case failed
  against the earlier native code; control-record isolation passed.
- `swift-test/20261031T142731Z-6347`: 33 passed with native conversion and the
  real three-file aggregate-budget case.
- `swift-test/20261031T143344Z-11481`: 36 passed, including deterministic source
  read, destination, cancellation, reserve and cleanup cases.
- R5: one source consistency pass; builds, tests, formatter, privacy, provenance,
  similarity and full gate unrun in the filtered runner. No network or commits.
  The four FFI tests and their assertions are unchanged; core pa remains
  unimplemented.
- Actual R5 `swift-test/20261031T144838Z-18599`: 36 passed. Independent native
  review has no P0–P2 for staging repair.
- `private/20261031T144834Z-18306`: 29 files scanned; only the two identified
  public test texts flagged. `similarity/20261031T144947Z-20775` passes R5 source.

R6 source-only from `6c3105f8`: rephrased the flagged test diagnostic as a brief
destination-error message. No native logic or guards changed. One final source
consistency check; no network, build, test, formatter or commit. R6 focused checks,
privacy/provenance/similarity and the original-base gate remain unrun.

Core pa construction, exact-flight ACK, durable intake/WAL apply and cursor
failure handling remain open; four FFI tests remain RED. The separate core pa
implementer's changes are absent from this seed. First adoption, conflict reads,
per-entry retries, structural ma, crash-orphan cleanup and suspended SDK-event
proof also remain open.
Privacy, similarity and the original-base gate remain required.
No Unit/U114, P24, Q6, F5 or full-workflow acceptance.

Public-copy compatibility repair (source only): clean source base
`d9df5e6ce59a6c60de93eda65f1dc208d745acf7`; original integration base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`. Filtered snapshot start:
`d5a5b2a44e627fa932bcb411d31ed593414ee514`. No seed patch.
Broad `swift-test/20261031T162505Z-70725` exposed a legacy staging error-type
regression outside the earlier 36 native cases. `copy` now maps typed source
shape/short-read failures to `CloudMediaFailure.invalidAsset`, size failures to
`assetTooLarge`, and unavailable source operations to their original error.
Fetched MGEntity pa alone calls `copyPayload`, the same bounded implementation
with its typed source failures. Destination open/write/sync/close, cancellation,
capacity and reservation failures remain outside source-error wrapping.

All existing native cases and per-entry zero/nonregular/oversize cases remain
enabled. The blob read-failure assertion now requires the original Foundation
error rather than the leaked internal wrapper; its failed-read and cleanup
assertions remain. Added bounded wrapper regressions for empty/directory/symlink,
oversize/growth/short-read, raw source open/read error identity and cleanup with
reservation reuse. One source consistency check; no builds, tests, formatter,
network or commits in this runner. Integrator compilation, focused tests,
independent review, provenance/privacy/similarity and original-base gates remain
unrun. Other Swift policy/capture failures and Rust migration/lazy-flight work
are outside this repair. No Unit or fullsync acceptance.

Integrator checks for native preparation: `private/20261031T150508Z-31715`
passes all 29 selected source/note files; final native source is covered by
`similarity/20261031T150653Z-32828`. The diagnostic-only edit changes no native
logic. Core bridge work remains a separate unverified export.
