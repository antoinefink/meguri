# Native core refusal diagnostics — 2026-11-10

Recorded fixer, filtered runner. Original batch reference supplied by root:
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Current starting commit was not queried: Git is prohibited in this runner.
Root owns execution, independent review and integration.

Failure diagnostics only; neither focused native failure has been rerun here.
No cause, behavior fix, unit completion or acceptance is claimed.

Touched functions:
- `RuntimeError::refusal_tags_for_testing` (new, feature-gated exhaustive classifier).
- `Runtime::original_note_observation_with_recheck_for_testing`, including its
  local `mismatch` helper. Both original-observer entry points were already
  `test-hooks` gated; the forwarding entry point is unchanged.
- `CloudMedia::start_next_work`. `preparation_error` and its cancellation mapping
  are unchanged; diagnostics inspect failures before that mapping.
- `CloudOriginalRole::observe_inner`: its existing Runtime failure still maps to
  `CoreError::Internal`; only its diagnostic message changes from validation
  line to closed `observation.runtime`/`errorKind`/`variant` tags.
- Both tests in `CloudSelectedOriginalExchangeTests.swift`, plus the new private
  `selectedOriginalObservation` wrapper and `SelectedOriginalObservationStage`.
  Each original observation stays at its original position; the wrapper awaits
  once, prints only on refusal and rethrows the same error. Assertions unchanged.

No existing recorder at these boundaries retains the typed preparation error or
observer refusal phase. Runtime/FFI stderr diagnostics are `test-hooks` only;
Swift output stays in the existing macOS Debug test file. No dependency, logger,
FFI export/record, production predicate, retry or extra authority read is added.
No associated error payload is inspected/formatted. No success output is added.

New output prefixes: `cloudMedia.refused`, `cloudOriginal.refused`,
`selectedOriginal.observation.refused`. Media sites: `admission`, `cancelCheck`,
`ownerCollection`, `runtimePreparation`, `authorityCheck`. Only the last two
carry Runtime category/variant tags. Observer fields: `phase`, `errorKind`,
`variant`, `mismatchMask` (decimal bitset).

Closed Runtime categories: `Tracking`, `Engine`, `StaleIncarnation`,
`IdentityUnverified`, `Incomplete`, `FileIdentityUnavailable`, `Publication`.
Tracking variants: `Io`, `Database`, `Constraint`, `InvalidIdentifier`,
`InvalidData`, `BindingNotFound`, `IsolatedBinding`, `RecoveryRequired`,
`RecoveryRefused`, `RuntimeSchemaUnsupported`.
Engine variants: `ExportSourceChanged`, `ExportSourceInvalid`,
`ExportFormatLimit`, `SyncNote`, `DeckOptionsInvalid`, `SchedOp`,
`InvalidTypeAnswerReplacement`, `SessionActive`, `CollectionCorrupt`,
`InvalidIgnoreBeforeDate`, `SuspendDeadline`, `NotFound`, `DeckRefused`,
`NoteNotFound`, `Stale`, `StaleGeneration`, `CollectionUnavailable`, `Busy`,
`UndoUnavailable`, `Cancelled`, `SchemaNotMarked`, `NotetypeRefused`, `Open`,
`Storage`, `SafetyBackupFailed`, `Media`, `CommitCompleted`, `MediaCommitPending`,
`Panicked`, `InvalidArgument`, `Unsupported`, `Internal`.
Other outer categories use variant `none`; their payloads are discarded.

Observer phases (exact tags): `initial.account`, `initial.decision`,
`initial.openEvidence`, `initial.binding`, `initial.accountTracking`,
`initial.withScanAuthority`, `initial.capturedWAL`, `initial.store`,
`initial.bound`, `initial.runtimeBindingRead`, `initial.bindingOpen`,
`entity.name`, `entity.read`, `entity.missing`, `entity.exact`,
`tracking.applyIntents`, `tracking.inbox`, `final.account`, `final.decision`,
`final.providers`, `final.withScanAuthority`, `final.store`, `final.bound`,
`final.runtimeBindingRead`, `final.bindingOpen`.

Mask bits, starting at bit 0, in original short-circuit order:

| Phase | Mismatch predicates |
| --- | --- |
| initial.accountTracking | account; tracking stopped |
| initial.capturedWAL | WAL generation; native file; current seq; tracking stopped |
| initial.bindingOpen | publication identity; decision; open evidence; stored binding; runtime file; runtime open |
| entity.exact | kind; key; tombstone; observer open; generation; observed seq; digest |
| final.providers | account; decision; current collection; open evidence; current seq |
| final.bindingOpen | tracking stopped; retained binding; stored binding; runtime file; runtime open; current seq |

Masks retain the original short circuit. Only one reached mismatch can set a
bit. Zero marks an operation error. The shared `with_scan` remains unchanged.
Absent apply proof still returns `applied_seq=None`. Read failures retain
their existing errors.

Swift stages: `sourcePreACK`, `earlierAutomaticACK`, `sourcePostExchange`,
`sourceReboundRefusal`, `peerBaseline`, `peerPostApply`, `sourceAfterPeerRefusal`,
`refusalBeforeSend`, `refusalHeld`, `refusalAfterFailedACK`, `refusalLaterRewrite`.
Expected refusal assertions can also produce these failure-only diagnostics.

Verification: one source-only consistency check passed. It compared original
observer/media tokens after removing diagnostics, confirmed all 11 Swift call
wrappers preserve the original file/assertions, checked unchanged FFI call inputs
and surrounding code, exhaustive enum tag coverage, feature gates and the output
interpolation allowlist. This is not compilation or runtime verification.
No build, tests, Git, network, forbidden/private/Anki sources or raw logs used.
Compilation, focused native execution, diagnostic capture and independent review
remain root-owned. No change to SDK/automatic-send tests, general capture, AX/UI
or tooling. No new FFI ABI; the classifier is a test-only Rust helper.

Root R1 review found no P0–P2 defects. Focused Docker result
`core-test/20261110T111615Z-63718` passed all eight original-role tests,
with zero skips. Native execution remains pending. No cause is yet proved.
