# Native asset-read bridge

This slice starts at `89c358d8`. The integration batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The Swift bridge wraps actual opaque Rust passes and work. It retains the
original owners and the concrete copied-file lease across awaited completion,
including failure and cancellation. Queue callbacks synchronously forward actual
admission; the caller retains accepted operations even if accounting then fails.
The bridge introduces no SDK scheduling, envelope parsing or acknowledgement
policy.

Implementation export: `cleanroom/20261031T220313Z-78406`.
Independent review found one missing instrumentation registry expectation.
The filtered fix `cleanroom/20261031T221432Z-82240` adds the seven exact names
without weakening equality. Round 2 independent review found no required fixes.

`swift-test/20261031T221531Z-83719` compiled actual generated bindings and passed
all 13 selected tests, but failed the zero-warning rule for a weak variable that
was never mutated. Filtered fix `cleanroom/20261031T221720Z-86381` changes it to
a weak constant and preserves the ownership assertions.

- `swift-test/20261031T221810Z-86993`: pass, 13 tests, zero warnings. Includes
  actual Host empty-pass begin/finish, owned-file lifetime for three outcomes,
  queue admission forwarding, result projection and the exact interval registry.
- `private/20261031T221628Z-85790`: six-file bridge slice passes.
- `private/20261031T221830Z-88348`: final source and test correction pass.
- `similarity/20261031T221827Z-88068`: six-file Swift bridge slice passes.
- `similarity/20261031T221502Z-83252`: preceding Rust ports and final tests pass.

Real SDK work submission/completion, cancellation and draining, generic conflict
receipt routing, cross-device collection/review/media proof and the batch gate
remain outstanding. This bridge proof does not establish end-to-end iCloud
sync. U113 and U114 remain unaccepted. Existing UI/accessibility gate failures
remain open; this slice changes no rendered interface.
