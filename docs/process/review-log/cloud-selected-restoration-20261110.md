# Selected restoration source handoff — 2026-11-10

Role: fixer under the implementer rules; recorded session supplied by root.
Prepared seed: full OWN formatted export 13331. Starting baseline: `720b5c9d`.
Disjoint media repair `09823090` is outside this change.
Original coherent batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Root records this unit's clean starting commit. No Git/history, network, jobs,
builds, test execution, private inputs or raw logs were accessed in this runner.
Repair authority: supplied Core/Swift R1 findings, `cloudkit-agent.md` J2.3,
J3.2, J4 and J5, implementer rules and applicable source-directory AGENTS.
This is source awaiting integration, not a unit, gate or signed result.

## Changed paths

- `app/MeguriSnapshotTests/TestHooks/CloudKitSmokeRuntime.swift`: extract the
  existing launch composition; separate first-install and admitted reopen.
  Reopen configures only a freshly checked actual account, opens the existing
  collection normally, validates original progress before creating the native
  backend, runs ordinary required preparation/scan, and revalidates at factory
  admission. Actual restore/recover call the typed owned readers. The issuance
  observer appends synchronously from the exact original Sync Now command before
  execution; append failure propagates to existing cancellation/join handling.
- `app/MeguriSnapshotTests/TestHooks/CloudKitSelectedOriginalRole.swift`: wrap the
  historical reader behind the independently verified original-source capability.
- `app/MeguriSnapshotTests/TestHooks/CloudKitSelectedSyncProof.swift`: separate
  restored-edit proof; preserve the existing live opening/observed-seq checks.
- `app/MeguriSnapshotTests/TestHooks/CloudKitSmokeEvidence.swift`: admit only the
  new opening's real SDK to a new empty ledger. No original callback, tick, state
  commit, own attempt or held-review proof is imported.
- `app/MeguriSnapshotTests/TestHooks/CloudKitSelectedRestoration.swift`: fixed
  digest decoding only.
- `app/Packages/MeguriKit/Tests/AppModelTests/CloudKitAgent/Support/AgentSelectedRestoration.swift`:
  bounded prefix classification and original-instance/order/witness checks.
  `AgentEngine.swift` and `AgentProtocol.swift` remain unchanged.
- `app/Packages/MeguriCore/Sources/CoreBridge/OriginalCloudRoleInspection.swift`:
  typed historical-edit result and QueryLane wrapper; original edit facts expose
  the additional actual post-flush witness.
- `app/Packages/MeguriCore/Sources/CoreBridge/SelectedCloudRestorationInspection.swift`:
  typed original control/exchange witnesses, distinct restored control result,
  and exact original exchange QueryLane reader. Existing live control inspector
  semantics are unchanged.
- `app/Packages/MeguriCore/Sources/Instrumentation/IntervalNames.swift` and
  `app/Packages/MeguriCore/Tests/InstrumentationTests/IntervalNamesTests.swift`:
  five registered bridge calls and exact expectations.
- `core/crates/meguri-sync/src/cloud/runtime.rs` and `workflow_store.rs`: capture
  one bounded owned original-note tracking image before opening-counter advance
  and reconciliation can overwrite its tuple; test-hooks only, no persisted
  schema, no writeback of historical observer fields. Baseline role selection
  still comes from the immutable original source, never this capture.
- `core/crates/meguri-sync/src/cloud/selected_observation.rs`: distinct restored
  control and historical edit/exchange readers reuse current file, binding,
  publication and account validation and final rechecks. Original control
  correlation is emitted only after actual original Complete/Active or retained
  adoption validation. It binds the original canonical control digests and
  original generation to the actual owned binding (including epoch/key/account/
  publication identity) and native file. Reopen joins that durable original
  observation to the same actual binding/file/current accepted Active; it never
  recreates the original Complete reply or marks `current()` proven on Mac.
  Adoption additionally retains its actual activated-file/backup checks.
- `core/crates/meguri-sync/src/cloud/policy_store.rs`: bounded exact owned
  exchange-row selection, at most 64 candidates for one exchange; overflow and
  ambiguity refuse. Decode the original whole ExchangeRecord, preserving every
  outcome and optional media counter/Boolean. No summary substitution.
- `core/crates/meguri-sync/src/cloud/policy/driver.rs`: original issuance reads
  require the pending exchange to match the issued action and remain uncaptured.
  Both admission checks surround the Runtime read; completed-result lookup keeps
  its earlier immutable witness and the ordinary policy lifecycle is unchanged.
- `core/crates/meguri-ffi/src/cloud_policy.rs`, `test_hooks.rs`, and
  `test_hooks_cloud_original.rs`: callback entry points expose bounded owned
  observations. The fixed-edit result is observed after its own WAL flush.
  Historical inspection separately checks present durability, canonical data,
  structural relations and ownership, while retaining the original edit stamps.

Tests/support added or extended:
`core/crates/meguri-ffi/tests/cloud_original_role.rs`,
`core/crates/meguri-ffi/tests/cloud_original_observation.rs`,
`core/crates/meguri-ffi/src/cloud_policy_tests.rs`,
`core/crates/meguri-sync/src/cloud/policy_store.rs`,
`app/Packages/MeguriKit/Tests/AppModelTests/CloudSelectedRestorationTests.swift`,
`app/Packages/MeguriKit/Tests/AppModelTests/Support/CloudServiceAppHarness.swift`,
`app/Packages/MeguriKit/Tests/AppModelTests/CloudKitAgent/AgentSelectedRestorationTests.swift`,
`app/MeguriSnapshotTests/TestHooks/CloudKitSelectedRestorationTests.swift`.
The harness seam runs test-owned admission before native backend creation.

## Supported bounded source paths

New original journals record `original-control-witness` after the two original
canonical control observations at publish/adopt. The original fixed-edit result
adds `original-edit-witness` after unchanged `localCommitted(n1)` and
`localDurable(n1)` observations. Original issuance records
`original-exchange-issued` from the retained actual RunExchange handle. These
are fixed content-free AgentFacts, not a command or receipt table.

- Mac completed launch/connect/publish: admit the original control witness and
  unchanged original note, then permit the first edit once.
- Mac completed edit, or edit started with all three sufficient original
  synchronous observations: join original seq/revision/generation/digest/witness
  to the retained pre-scan tracking tuple and actual durable canonical row; never
  replay fixedEdit. Permit the first exchange after a proven original edit.
- Phone completed launch/connect/adopt: validate actual retained activation and
  backup, original control witness and original note, then permit first exchange.
- For an interrupted iPhone exchange, recovery requires the persisted original
  issuance correlation together with that operation's exact successful record.
  It returns the recorded accounting and performs no replacement exchange.
  A subsequent independent peer read is still necessary. Storage inspection can
  return failed outcomes, which the runtime rejects as completion evidence.
- Mac exchange accounting remains readable, but recovery fails explicitly with
  `implementation_missing`: no historical revision-specific n1 ACK/event reader
  exists. No recovered facts, checks, command advance or close proof follow.
- Fully completed delivery remains handled by the unchanged Engine fast path,
  returning original journal bytes without runtime restoration.

Reject bootstrap interruptions, absent/lost preview, incomplete or unwitnessed
edit history, changed payload/revision/relations, wrong file/binding/epoch/
original observer tuple, correlation overwritten before capture, missing exchange
row, wrong issuance, candidate overflow, arbitrary later completed-exchange or
observe prefixes, and smoke recovery. Old journals lacking these added original
correlations remain unproven. No account settings or production scheduling was
changed; no media paths, CloudKitSyncAdapter, nextest or selected close logic was
repaired or replaced. Earlier tests, including smoke held-review checks, remain.

## Unrun verification and remaining gaps

All tests below are written but UNRUN. Root owns native bindings regeneration,
compilation, focused MG verification, independent review and per-unit provenance,
privacy and similarity checks at the original batch base and coherent boundary.
Generated bindings were not edited. Source inspection is not execution evidence.

Rust focused additions:
- `exact_original_exchange_joins_binding_owner_and_all_optional_accounting`
- `original_exchange_witness_uses_issued_handle_and_refuses_completed_or_stale_command`
- `original_exchange_witness_rechecks_pending_issuance_after_provider_read`
- `bound_original_edit_witness_survives_process_kill_without_a_second_mutation`
- `original_edit_committed_without_bound_original_witness_fails_after_process_exit_without_replay`
- `original_edit_reopen_process_entry` (subprocess entry; normal unselected no-op)
- `historical_inspection_does_not_promote_current_rows_to_missing_original_control_or_edit_proof`

Swift focused additions:
- `selectedPublishedEditAndExactIssuedExchangeSurviveFreshCoreOwner`
- `selectedAdoptedPhoneReopensOriginalActivationWithoutAnotherAdoption`
- `onlyBoundedPrefixesSelectOriginalOwnedReads`
- `partialWrongInstanceReorderedOrMissingOriginalWitnessFails`
- `originalEditGenerationBelongsToItsOwnOpening`
- `macExchangeRecoveryRefusalNeverCompletesOrStartsClose`
- `originalMacExchangeRecoveryCannotSupplyCompletionOrCloseFromAccounting`
- `restoredAdmissionBeginsAnEmptyLedgerAndDoesNotSupplyOriginalCallbacksOrHeldReview`
- `restoredSDKAdmissionRejectsWrongOpeningAndMissingRequiredScan`
- exact `IntervalNamesTests` registry expectation; preserve existing selected
  exchange/bootstrap/close, protocol failure/recovery-linkage, and smoke suites.

Remaining proof work is explicit. The new subprocess source test publishes once
through the real fixture, then a different OS process makes its first fixed edit,
syncs its actual result and witness, and waits for the parent's kill. A further
process reads the same owned collection and witness and compares stored rows and
sequence around historical reads. It has not run. The prior unwitnessed-process
refusal test remains. Successful interrupted-exchange recovery across OS processes
still lacks a positive test and Mac completion remains unsupported without the
separate original n1 revision/envelope/ACK and callback-operation proof.

The new Service cases are intended to carry actual publication through a fresh
owner's first edit and exchange, then inspect history after another owner change
inside the same process. Their succeeded-record branch invokes Mac's existing
missing-proof refusal. The protocol fixture checks a failure terminal and denial
of a close successor with synthetic receipts. App-hosted cases address fresh
ledger admission and that refusal; their scope supplies no acceptance evidence.
Actual CloudKitSmokeRuntime restoration and recovery still need coverage using
retained product, account, run and journal admission. Also outstanding are the
runtime negative for absent original ACK proof, native issuance-journal append
failure with cancellation and joining, and the full authority/corruption matrix.
Remaining checks cover file, binding, epoch, open, generation, revision, payload,
WAL and integrity; original observations and exchange records; and each optional
accounting field. General bootstrap/smoke recovery, later callback reconstruction,
final close and signed acceptance remain pending.


R1 source repair: keep command-3 control generation independent from the positive
edit-witness generation; retain historical comparisons to the latter. Rebuild the
exact-one-original-observation selector without changing its missing/duplicate,
instance, ordering, terminal or hash refusals. Fix the reported Rust outcome path
and Swift unused optional binding. Existing assertion budgets, live inspection,
control proof, selected close and smoke held-work requirements remain in place.
The single source-only consistency inspection checks both issuance admissions,
the independent edit-generation parse, Mac refusal before recovered observations,
unchanged live historical comparison, bounded subprocess coordination and retained
negative tests. It does not compile or execute any product/test code. Root owns
generated bindings, formatting, compilation, focused checks, independent reviews
and final privacy/similarity scans. No test, Unit, gate or signed result is claimed.


## Narrow test repair and diagnostics, source only

Supplied current baseline: `81f635f4`, fast-forwarded only for approved
spec/oracle work and the public scan pin; no app/core contents changed.
Resume seed: complete OWN formatted R2 recovery patch `89253`. Original batch
base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`; media repair
`09823090` remains outside this delta. This fixer used filtered source only,
without Git/history, network, builds, tests, private data or raw logs.

Actual root evidence supplied for the seed: core-test
`20261110T133057Z-98735` compiled 18 cases, with 16 PASS and 2 FAIL. The exact
exchange store fixture failed at its second Active binding (Constraint); the
bound-process positive failed readiness after 20.6 seconds at witness helper
line 795. The process's inner cause is UNKNOWN. Swift `20701` compiled six
cases, with the three parser cases PASS and the terminal fixture plus both new
Service cases FAIL (3/6 passed). Both Service failures occur during INITIAL
selectedBootstrapPublish, before the first close/reopen, as established by the
unique selected-terminal diagnostic producer. Source shows owner.close already
quiesces and joins its backend; exact command issuance order remains unproved.
Private `20690` identified the terminal-validation block at parser lines 62–64
and the two whole documentation paragraphs at 103/164. The correct observation
selector is unchanged.

The second binding fixture now uses the existing Stopped isolation state while
the first stays Active. All exact-record, optional-accounting, newer-summary,
wrong-file/witness/binding and lost-row assertions remain. The protocol refusal
fixture selects the last done/failed terminal and separately requires the later
ownedWorkJoined observation. No command-5 success, command-6/close, fresh success
checks or extra execution are permitted; the single fresh join stays required.

The bound child now emits only a closed checkpoint at existing boundaries and a
test-child panic location consisting of its current checkpoint, a whitelisted
file tag and numeric line/column. No panic payload, path, input or identifier is
printed; stderr remains null. The parent retains only the last recognized tuple,
labels boundCommit/boundRead on readiness failure, and reports after kill, reap
and reader join. Exact original readiness, 20-second bounds, 64-KiB stdout cap,
actual SIGKILL, synced witnesses, full row/sequence comparisons, no second
mutation and durability assertions are preserved. Diagnostics do not fix or
explain the process positive yet.

Only the two new Service catches print filtered retained command.begin/end and
closed configureAutomatic-requested traces from the original publication
owner's first backend, before cleanup. The requested Boolean comes directly
from the typed action. No commands, guards, settings or payloads are dumped;
no new exchanges, waits, retries or lifecycle changes were introduced. The
complete terminal-validation block was reconstructed from its journal contract,
retaining action matching, one done per original start, strict terminal order,
and original-instance/nil-link or exact recovered-from-start linkage. The two
flagged prose paragraphs were rewritten as whole paragraphs.

One source consistency check precedes export. These test fixes and diagnostics
are UNRUN here, and every reported failure remains unresolved pending root's
compilation/execution. Production readers, issuance guards, SDKs, close, smoke,
assertions and limits are retained. No production SDK/stopping/authority repair
or historical Mac ACK support is claimed. Runtime close/restore integration,
complete historical Mac n1 ACK and callback evidence, positive interrupted
exchange recovery across OS processes, native append-failure/cancel/join, and
the authority/corruption matrix remain open as described above.

Root's next checks: compile/run the process positive, legal-binding fixture and
old unwitnessed refusal first; then the new Swift six and old OriginalExchange
two, comparing original publication ordering with passing `31543` on this
source. Privacy, lint, independent review and all provenance/similarity/batch
gates follow. No Unit, gate, signed execution or acceptance claim is made.

## Root checkpoint

This is WIP. Core run 98735 passes 16 of 18 checks. Corrected binding case
88968 passes. The four Swift journal/protocol cases pass on R3.
The combined Swift run 96256 stalled in the older comparison and was cancelled
after a one-second stack sample; it supplies no passing integration receipt.
The two new traced Service cases did not execute in that cancelled run.

Process run 92426 passes the unwitnessed refusal and fails the bound positive
at `boundCommit / waitEdit`. Independent review confirms a Query-reader cycle:
the FFI caller holds the one Query slot and the new witness requests it again.
The next repair will reuse the held snapshot's identity and sequence.
No reader-capacity, authority, durability or timeout change is planned.

The initial publication SDK reconfiguration failure remains open.
Full admitted-runtime recovery, historical Mac ACK, process exchange recovery,
append-failure handling, corruption checks and signed execution remain pending.
No Unit or gate acceptance is claimed.
