# U195 collection-owned Add coordinator — source-only partial handoff

Role: implementer. Requested upstream base:
`b1475209593e22d5cf2ea46df92d7d9b2058b714`.
The filtered copy's actual starting Git commit is
`4da18b4f852100aaf50849ab9341700e0ae60f90`; the independently reviewed
30-file combined seed was already present as working changes. No branch,
worktree, commit, upstream application or push was made. MG owns export of the
cumulative `changes.patch` from these actual source files.

U195 is **incomplete**. This delivers the explicitly authorized safe fallback,
not a usable normal-app Add entry. No Home/Shell/editor route was exposed.

## Completed implementation part

- `ReviewCollection` retains `AddNoteFlow`, its preparation/draft, and the
  actual `FieldEditorHost` through a small AppModel runtime boundary. Navigation
  still supplies real selected-deck and review-card identities; layout remounts
  obtain the same runtime instead of recreating it. No alternate store,
  persistence path, prototype route or setup-only production behavior exists.
- One save task performs the actual held field flush, checks stored note types,
  a generation-bearing normal-deck snapshot and fresh lifecycle, captures the
  exact persisted type/base, field values, tags and deck/generation, runs a
  fresh field check, and calls `CoreNoteService.addNoteForScreen` once. There
  is no fallback to the unguarded writer and no retry.
- Typed empty/non-cloze refusals release input and focus the first field.
  Missing-cloze consent has an attempt-specific token and keeps the field hold.
  The draft reference, runtime epoch, attempt and revision are rechecked after
  suspension and consent. Error retains the exact draft and releases only the
  same current host. Delayed callbacks cannot write to a later runtime.
- Durable success records the actual mutation/card IDs/count and one history
  entry, advances once to sticky values/tags from the submitted draft, mounts
  the real next editor, and refreshes real deck counts and the selected deck
  projection. The next draft is prepared before submission so a later runtime
  mount error cannot make the committed input eligible for resubmission.
- Save-time D5 repairs a missing type by A3.1 mapping and a missing/filtered
  deck to stored normal deck 1, displays the repaired draft and stops for a
  new Add activation. A changed aggregate of the same type is a typed stale
  refusal, preserving the original base rather than silently rebasing it.
- Explicit deck entry validates the actual normal deck both on first open and
  when the editor is already open. Stale/filtered IDs never silently choose
  defaults. A remount/reentry retains the actual field runtime.
- A single asynchronous close guard joins the accepted save, makes a held
  flush and asks through a tokenized discard decision. Keep Editing releases
  input and returns false; failed flush retains presentation/draft/error.
  Successful close revokes and disposes the old runtime. An unchanged draft
  remains retained; explicit discard removes it. ReviewCollection close/quit
  uses this guard before collection flush/close.
- Background joins accepted save work and performs held field flush before
  Core background flush/suspend. Failure stops that Core lifecycle work and
  retains the editor/error; resume releases the same current host. Pending
  missing-cloze consent is cancelled rather than submitted by background or
  close. New Add actions are excluded during close/background admission.
- `prepareForReplacement` reserves Add admission before the close guard and
  returns an identity token; only matching terminal release lifts it. This is
  a composition API, **not** evidence that replacement consumers use it.
  Generation notifications defensively revoke callbacks, clear history and
  discard only unchanged retained drafts. They are not pre-operation guards.

## Regression changes, not executed

The original focused method identity remains
`ReviewCollectionStartupTests.preparedAddDraftFlushFeedsOneDurableU27WriteAndExistingStudy`
in `FieldEditorTests.swift`. It now uses
`ReviewCollection.prepareAddNoteEditor` and `AddNoteFlow.save`, rather than a
manual field check and direct `addNote`. The existing isolated real collection,
empty normal deck, preparation-race cases, ordinary Study and cold reopen
remain. Added assertions cover invalid explicit entry, exact runtime retention,
empty refusal, rejection of a second activation, and a deck identity changed
while an actual fresh Core field-check result is suspended with the existing
bounded `HeldResult`. That failed attempt must retain exact fields/tags/draft,
release input and write no card to either real deck. The following save reads
persisted fields/tags, real generated IDs/count, defaults and deck projection,
then studies the generated card and checks persisted fields after cold reopen.
No old timeout or other runtime test was relaxed, and no runner, lane, harness,
custom testing tool or job was added.

## Still-uncovered pre-operation consumers — entry must remain hidden

The decisive uncovered consumer is
`SyncCoordinator.confirmConnection(_:)`, `.adoption` branch, which invokes
`connection.confirmAdoption(handle)` directly. It does not call an async
editor close admission first. Publication/resume and retained automatic own
work also require a complete replacement-capability audit before enabling a
route; notification-time revocation cannot provide A7.1 admission.

The following existing consumers also still need to acquire the same async
reservation and cancel/dispose their unconfirmed authority on Keep Editing:

- `CollectionEntryCoordinator.confirm()` before `beginEntryActivation()` and
  `service.activate` (Files collection package import).
- `ManualTransferCoordinator.prepare()`, `confirm()` and recovery admission
  before `beginManualActivation()` / `service.confirm`.
- `SyncCoordinator.confirmMeguriPreview()` and `finishMeguriSetup()`, which
  invoke the synchronous `beginMeguriActivation` callback installed by
  `ReviewCollection.configureSyncOwnership`.

`SyncCoordinator.beginConnectionPreview()` also has a direct initial-adoption
call. Its documented no-file condition must be preserved and proved mutually
exclusive with Add, rather than treating discovery as an editor guard.
`ReviewCollection.startEmpty()` is an existing no-file creation path, not a
replacement admission that can coexist with a ready Add editor. No production
backup-restore consumer was identified in the inspected app sources; future
restore wiring must use the same pre-operation guard.

The existing `beginEntryActivation` and `beginManualActivation` now fail closed
if this editor is open. They do not run a discard prompt and do not claim the
async contract. Their consumer control flow and sync/network implementation
were left unchanged. Ordinary incremental Meguri sync is not disabled for an
open draft. Completing the async admissions, exact confirmation cancellation,
and terminal reservation release across these consumers is required before
exposing **any** new Add entry.

## Deferred auxiliary U195/UI work and limitations

Home and normal/empty-deck entry actions, Mac resizable trailing panel, mobile
native sheet composition, exact keyboard/first-responder command ownership,
review input suspension, sheet interactive-dismiss interception, native discard
and cloze dialogs, refusal announcements, accessible native labels/identities,
toast/Undo, searchable choosers/D2, stored pin toggles, tag suggestions/native
tag input, History/Browse integration, live change-stream D5 reconciliation,
and broader same-type aggregate refresh/recovery are not delivered here. The
AppModel tokens/state are ready for native consumers; they are not native UI.
Image occlusion remains explicitly unsupported by the existing field owner.
No interface rendering, accessibility evidence or user design acceptance is
claimed. No milestone or unit acceptance is claimed.

No network, build, test, oracle, refs, local-data, Anki/editor third-party
source, policy changes, sync/network implementation changes, instrumentation
changes or existing-job changes were performed in this session. Seed Core,
CoreBridge, instrumentation, manifest, runtime JS/HTML/resources, existing
seed review log and their tests were not rewritten. No clear seed Core compile
mistake was identified. Verification is limited to source inspection and a
whitespace/diff consistency check; compilation, focused execution, independent
review, privacy/provenance/similarity and all required gates belong to the
integrator. Existing failure evidence and the verification register are
unchanged.

## Actual files changed by this session relative to the supplied seed

1. `app/Packages/MeguriKit/Sources/AppModel/Collection/ReviewCollection.swift`
2. `app/Packages/MeguriKit/Sources/AppModel/Editor/AddNoteFlow.swift` (new)
3. `app/Packages/MeguriKit/Sources/AppModel/Editor/NoteEditorDraft.swift`
4. `app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift`
5. `app/Packages/MeguriKit/Tests/AppModelTests/FieldEditorTests.swift`
6. `docs/process/review-log/U195-add-coordinator-cleanroom-20261118.md` (new)

The cumulative export retains the 30-file seed and adds three paths (the
collection source, coordinator source and this report): 33 paths total. Its
exact current file list is:

```text
app/Packages/MeguriCore/Sources/CoreBridge/EditorServices.swift
app/Packages/MeguriCore/Sources/CoreBridge/NoteService.swift
app/Packages/MeguriCore/Sources/CoreBridge/NotetypeService.swift
app/Packages/MeguriCore/Sources/Instrumentation/IntervalNames.swift
app/Packages/MeguriCore/Tests/CoreBridgeTests/MutationServiceTests.swift
app/Packages/MeguriCore/Tests/InstrumentationTests/IntervalNamesTests.swift
app/Packages/MeguriKit/Package.swift
app/Packages/MeguriKit/Sources/AppModel/Collection/ReviewCollection.swift
core/Cargo.lock
core/crates/meguri-engine/Cargo.toml
core/crates/meguri-engine/src/editor/mod.rs
core/crates/meguri-engine/src/notes/add.rs
core/crates/meguri-engine/src/notes/mod.rs
core/crates/meguri-engine/tests/editor_services.rs
core/crates/meguri-ffi/src/editor.rs
core/crates/meguri-ffi/src/notes.rs
core/crates/meguri-ffi/src/notetypes.rs
core/crates/meguri-storage/src/paging.rs
core/crates/meguri-text/src/iri.rs
app/Packages/MeguriCore/Tests/CoreBridgeTests/EditorServicesTests.swift
app/Packages/MeguriKit/Sources/AppModel/Editor/AddNoteFlow.swift
app/Packages/MeguriKit/Sources/AppModel/Editor/NoteEditorDraft.swift
app/Packages/MeguriKit/Sources/Features/Editor/EditorBridgeMessage.swift
app/Packages/MeguriKit/Sources/Features/Editor/EditorDocument.swift
app/Packages/MeguriKit/Sources/Features/Editor/FieldEditorHost.swift
app/Packages/MeguriKit/Sources/Features/Editor/Resources/FieldEditor/page.html
app/Packages/MeguriKit/Sources/Features/Editor/Resources/FieldEditor/runtime.js
app/Packages/MeguriKit/Sources/Features/Editor/Resources/Localizable.xcstrings
app/Packages/MeguriKit/Tests/AppModelTests/FieldEditorTests.swift
core/crates/meguri-engine/src/editor/tag_complete.rs
core/crates/meguri-engine/tests/add_screen_writer.rs
docs/process/review-log/U190-U195-field-runtime-cleanroom-20261118.md
docs/process/review-log/U195-add-coordinator-cleanroom-20261118.md
```
