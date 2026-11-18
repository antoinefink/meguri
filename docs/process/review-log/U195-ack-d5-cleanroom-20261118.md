# U195 durable acknowledgment and D5 fallback — source-only repair

Role: implementer. Specs: `editor/add-flow` A3.1/A5, `notes/add-defaults` D5/D6.
Requested cumulative seed: b147-based t26n,
`bc615fa5281b24b2b89fba27b69aff9997c32b8353ef911115b644fb78ac0a4f`.
The 33-file prepared seed was already present in this filtered checkout.
Actual starting filtered Git commit:
`842402cfb9ca12ecaf6b841527d5d0b59aa1f4cb`.
The upstream patch serialization is not present here for independent digest
verification. The prepared source was retained; no reset, branch, worktree,
commit, patch application, staging or push was performed. Normal MG export
owns the cumulative patch from the actual allowed file edits.

## Repairs

- `AddNoteFlow.performSave` validates the real guarded Core mutation's durable
  acknowledgment before presentation ownership. It returns that exact reply,
  including after owner revocation, with no retry or shadow success. The
  submitted draft identity records its acknowledged revision. A still-retained
  exact closed draft is removed; open/save refuse an acknowledged revision.
  Newer revisions and different retained identities survive, including a
  successor draft with the same field bytes. Current-owner history is separate
  from acknowledgment; advance requires the exact submitted revision. Late
  success does not mount, release or refresh a revoked owner. Post-add mount
  error reporting and refresh also require the retained current next draft.
- The editor-only `add_notetype_selection` query acquires one Query snapshot,
  validates its generation against the caller, and returns the present
  selected aggregate or the aggregate selected by the existing storage
  `ReadRows::first_notetype_id_by_name` query. SQLite's registered `unicase`
  and stored name order remain authoritative. The returned generation and
  full aggregate digest travel through CoreBridge on QueryLane. Swift checks
  owner/revision and returned generation after the query, retains its existing
  lifecycle check, and passes the unchanged aggregate base to the guarded
  writer. D5 repair maps fields and halts for another explicit Add activation.
- `NotetypeRecord` conversion is shared internally so the D5 snapshot returns
  the same complete record as `read_notetype`, including base, layout and
  cloze metadata. Normal NoteType API signatures and name/ID ordering are
  unchanged. No storage query, schema, source-control policy or job changed.
- Registered `add_notetype_selection` in the existing BridgeCall definition,
  exhaustive list and exact registry-test expectation. Generated bindings are
  left to normal MG regeneration; no generated artifact was hand-edited.

## Scoped regression source (unrun)

The existing serialized `ReviewCollectionStartupTests` extension and method
`preparedAddDraftFlushFeedsOneDurableU27WriteAndExistingStudy` remain in
`FieldEditorTests.swift`. Its original field/runtime, precommit refusal,
held-check revision race, durable write, history, next draft, deck refresh,
review and cold-reopen proof remain. The same fixture now holds actual Core
commit replies through a bounded callback using the existing `HeldResult`:
revocation, newer revision after revocation, another retained draft identity,
and newer input in the current owner. Assertions require the exact actual
reply, durable row, one write's card-count increase, no second activation,
closed-host state, history ownership and preservation of newer targets/tags/
pending fields. The exact acknowledged stale identity cannot reopen/save.

D5 creates a lower-ID Zulu type and a higher-ID mixed-case `aARdvark` type.
An actual Add Note Type undo removes a third stored selected type. The fixture
proves the public list remains ID-ordered, D1 differs from the D5 winner,
a present non-first type is retained, and the missing-type repair receives the
expected stored aggregate/base. Reordered/new field names exercise exact-name
and positional mapping. Counts, undo status and remembered defaults remain
unchanged on repair; only the next explicit Add writes. Cold reopen checks the
original durable row and stale-generation refusal by the new query.

## Files changed relative to the prepared seed

- `app/Packages/MeguriCore/Sources/CoreBridge/EditorServices.swift`
- `app/Packages/MeguriCore/Sources/Instrumentation/IntervalNames.swift`
- `app/Packages/MeguriCore/Tests/InstrumentationTests/IntervalNamesTests.swift`
- `core/crates/meguri-ffi/src/editor.rs`
- `core/crates/meguri-ffi/src/notetypes.rs`
- `app/Packages/MeguriKit/Sources/AppModel/Editor/AddNoteFlow.swift`
- `app/Packages/MeguriKit/Sources/AppModel/Editor/NoteEditorDraft.swift`
- `app/Packages/MeguriKit/Tests/AppModelTests/FieldEditorTests.swift`
- `docs/process/review-log/U195-ack-d5-cleanroom-20261118.md` (this report)

All other prepared seed files are byte-for-byte retained, including the
already-prepared ReviewCollection integration. ReviewCollection, Sync, entry,
manual consumers and native shell received no edits in this repair. The editor
stays hidden; the separately owned replacement-consumer work remains required.
This repair does not complete U195 or establish app/milestone acceptance.

## Limits and required integration work

No network, build, test, MG job, tooling change, Main edit, forbidden source,
third-party editor source or private input was used. Only source inspection
and the source consistency check below are available. No independent review,
privacy admission, provenance/similarity gate, generated-binding build, Swift
compilation, Rust compilation, runtime execution or full gate is claimed.
The integrator must review the complete changed contents (including the FFI
surface and registry), regenerate bindings, run focused existing suites and
required gates, and perform exact-hash privacy/provenance/similarity checks.
`verification-status.md` was read and its failed/incomplete evidence retained;
no existing failure is cleared by this source-only repair.
