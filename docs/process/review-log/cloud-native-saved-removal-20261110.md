# Native Saved pending-removal plumbing — 2026-11-10

Role: recorded implementer. Clean starting commit: `5149ad9e` (provided by root).
Original coherent batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contract: `sync/cloudkit-records#K6`, `#K10`, `#K12.3`, `#K12.4`,
`sync/policy#P4`, `conformance/cloudkit-agent#J3.2`.

Implementation only; no Unit, gate, signed-run or acceptance claim. Root owns
execution, independent review, commits and provenance/privacy/similarity gates.
No Git, network, build or tests were run; no forbidden/private sources or raw
runtime logs were read. One final source consistency check is recorded below.

## Change and review/fix notes

- Expose existing `Runtime::sdk_saved_removed` through `CloudWorkflow`, a
  `MaintenanceLane` CoreBridge method, and the exact BridgeCall registry and test.
  The FFI also validates the SDK; Core's exact saved set and returned-handler
  requirements remain unchanged.
- Add typed original-instance/request/name receipts, an empty-facts engine
  default, and a delegate hook for later real native bookkeeping boundaries.
  Production does not clone or interpret Core's observation history.
- Service receipts use the actual entries removed by `removePending`, matched
  against issued per-name revisions and original network generation, after its
  held handler returns. A failure, missing issue revision, cancellation, release,
  partial removal or newer applied revision supplies no receipt.
- Native snapshots require originally handed pending membership, the original
  SDK object, unchanged application-update version and original network
  generation. Query before pending updates, observe before applying any Core
  mutation, and allow a later SDK callback to supply an original receipt. Any
  unresolved proof is invalidated by application mutation; an absence created
  by that mutation cannot become a Saved-removal observation. Confirmed facts
  observed before a re-add remain associated with the old request.
- Adapter forwards only the owned engine object's receipt, with exact original
  saved names, unchanged generation/binding/token guard and current SDK checks.
  Core ACK -> handler return -> SDK removal observation -> pending application
  remains ordered. Existing state persistence, returned-work ownership and all
  existing native ACK ledger assertions remain intact. Pending applications
  retire overlapping unresolved metadata rather than retaining it forever.

## Exact tests for root to execute

Rust `cloud_conflict_receipts_tests.rs` (module is nested under existing media
fixtures; test name filter is sufficient):

- `saved_removal_export_requires_original_complete_receipt_and_handler_return`
  Uses an actual Service save and original FFI batch; checks early handler,
  foreign SDK, unknown request, wrong/empty names, exact receipt, duplicate, and
  changed-current-guard SDK refusal.

`NativeCloudSavedRemovalTests`:

- `nativeSavedRemovalRequiresActualAbsenceAfterOriginalIssue`
- `nativeNewPendingUpdateCannotMasqueradeAsOriginalRemoval` (false/true)
- `nativeRemovalObservedBeforeReaddRetainsOnlyOriginalReceipt`
- `nativeFailedOrUnknownSavedCannotConfirm` (false/true)
- `nativeChangedNetworkGenerationRefusesUnconfirmedAndReadyProof` (false/true)

Existing serialized `ReviewCollectionStartupTests`:

- New `serviceSDKSavedRemovalRequiresOriginalHandlerReturnAndActualBookkeeping`
  holds the actual Service Saved handler after durable ACK, checks exact
  rev/digest/system fields, no early removal, then returned-before-removed
  observation order and actual SDK absence for the original request.
- Extended `serviceSDKAutomaticSendUsesAdditionalOriginalBatchesForHandedBackRevision`
  (false/true) asserts exact first-request removal observations while retaining
  all existing real revision, pending retention, automatic-run and empty-pass
  bounds.
- Retain/run existing
  `serviceSDKCancellationAndReleaseJoinOriginalAutomaticCallbackAndFenceAdmission`
  (false/true) and `InstrumentationTests`' exact BridgeCall registry test.

## Limits and unimplemented proofs

No actual Native32328 replay or Apple SDK execution happened in this filtered
runner. A native SDK state that still contains Saved names at all available
readbacks provides no removal proof. If a Core pending mutation arrives first,
that original proof stays Missing/unconfirmed; a later unqualified absence is
intentionally refused. The native ownership check conservatively invalidates
on any application pending update, including unrelated names; no per-entry
revision is available from CKSyncEngine. Root must establish whether normal
native event scheduling exposes the required absence before mutation, and
review whether further SDK bookkeeping boundary integration is necessary.

The added native matrix tests the production ownership/absence collector; it
does not execute CKSyncEngine's own pending bookkeeping. No new adapter-level
foreign-object injection test was added; the adapter checks object identity and
Core tests foreign SDKs and changed current authority. Existing fake engines return no removal facts
unless explicitly implemented; their empty default is not confirmation.

## Ownership / overlaps

Edits are confined to `sync.rs` export, tracking-store bridge, SyncModels,
CloudKitSyncAdapter callback/pending application bookkeeping,
NativeCloudKitEngine bookkeeping, its small NativeCloudSavedRemoval collector,
ServiceSDK, associated tests and BridgeCall registry. No edits to
AdapterSend/CorePolicyExecution diagnostics, AX replay, app/UI or tooling.
Other fixers may share CloudKitSyncAdapter/SyncModels or the interval registry;
root should merge those by function/registry entry.

Final consistency check: passed one source-only Python pass over these 13
paths: whitespace/conflict markers, FFI/Swift spelling, lane/registry alignment,
handler-return/query/update order, original-object/name guards, pre-mutation
native readback and Service issued-revision/generation checks. No compilation
or behavioral tests were executed.

## Root integration

Independent R1 review found no P0–P2 defects. The first Swift build failed.
Both real collectors now use the exact async protocol signature. The native
caller uses await. Neither body changed or gained a suspension point.
The reviewer confirmed this correction.

`swift-test/20261110T105643Z-91094` passed all five collector functions and
eight cases. The filter did not select the instrumentation registry test;
that check remains pending. The initial compiler failure is retained in
`swift-test/20261110T105532Z-87532`. Rust and Service checks are pending.
There is no native SDK or signed round-trip acceptance from these results.


## R2 — newer pending revision

Source and spec reviewers confirmed a defect in the new test assertion.
K12.3/K12.4 and P4 require a newer pending revision to survive the old reply.
The original request therefore has no complete-set removal receipt. The
separate held-handler positive remains intact.

The test now joins the real pending update within its existing bound. It
checks the edited name's SDK revision, Core localRev, mark and pending save.
It rejects false first-request removal proof. The automatic second batch uses
actual issued revisions and the pending map at removal. An unchanged complete
set needs real removal observations; a superseded set must have none. These
read-only test snapshots change no SDK behavior.

All earlier assertions, counts, time limits and cleanup remain intact.
Production code is unchanged. R2 source review is clear. No Unit is accepted.
Run the three Service functions already listed above. The first R2 selection
could not compile because of a separate replay-test API error; it executed
no cases. Root must run them after that correction.
