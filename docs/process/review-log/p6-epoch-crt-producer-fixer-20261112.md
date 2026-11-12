# P6 epoch-crt producer export

- Role: fixer. Integration starting commit: `1ff8bee6`. Session: `s-20261110T214025Z-20fa15fd`.
- Authority: approved `sync/policy` P3/P6/P14/P19, `sync/cloudkit-records` K10/K20/K21, `sync/cloudkit-recovery` V2.4/V14/V15. The under-review P24 amendment is untouched.
- Delivery: the explicitly permitted smaller producer/ledger step. **P6 gate repair is not complete.** Root owns builds, focused verification, independent review, provenance/privacy/similarity gates and acceptance. No builds, tests, commits or subagents were used here.

## Findings and implementation choices

`cloud_publication_bindings.crt` is not universal epoch authority: publication/adoption seed it from their epoch, but `accept_media_root` can insert the current local crt. Its historical value cannot distinguish those sources. The existing fetched intake skips the `epoch` control before entity classification. Policy projection still defaults `crt_mismatch` false.

The implementation choice is one separate `cloud_epoch_crt` table, keyed by the existing immutable binding identity, containing an integer `crt` and a constrained authority tag. The binding already owns the epoch/account/container/environment; those identities are not copied into a new JSON DTO. No local collection crt is persisted by this step. No flags are inferred from absence.

The ledger belongs to publication schema **3**. Under the Runtime's existing tracking ownership, migration from publication schema 1 or 2 creates an empty ledger and advances its marker in one transaction. Migration does not backfill prior data, even when values appear equal. The internal probe accounts for WAL state, requires the publication marker and ledger to agree, and rejects unsupported publication schemas before opening the source store. Outer tracking schema V3, runtime schema V7, and the workflow, policy and collection schema markers retain their existing versions. Adoption installs publication schema 3 without a downgrade to schema 2. Cleanup of binding-owned work also removes the ledger rows.

Qualified producers, each within its existing owned transaction:

| Stored authority | Actual evidence |
|---|---|
| `published` | The current publication request's accepted Saved/Conflict result for `SavePublishingEpoch` or `SaveCompleteEpoch`: supported epoch, matching epoch id/key/crt and expected state, nonempty system fields. Not confirmation, a local seed, or a media-root result. |
| `adopted` | The adoption op's actual decoded complete MGEpoch, admitted by the existing staging/adoption workflow, rechecked against the new binding and staged crt in `ready_adoption`. |
| `fetched` | Exactly one readable complete `epoch` record with nonempty system fields and matching bound epoch id/key, supplied to the actual SDK collection-zone intake or non-SDK collection-record intake. Its fact commits with inbox/refusals, before later cursor persistence; a failed transaction rolls it back. |

Media-root acceptance is unchanged and never writes the new ledger. Unsupported, unreadable, non-complete, duplicate, foreign-epoch or foreign-key controls do not establish new knowledge. This small step preserves their existing handling; it does not claim the full K20 control-refusal workflow. The read-only testing seams expose actual stored knowledge; there is no setter or known-flag fixture. No Swift/FFI ABI, CloudKit field or production-container change is needed for this step.

The approved-text gap is specifically **write admission with absent legacy epoch-crt knowledge**. P6 gives an equality condition and a known-mismatch failure row, but supplies no unknown/legacy row or rule establishing whether writes wait for qualified evidence. The implementation neither equates unknown with mismatch nor adds permission based on unknown. Existing admission behavior remains unchanged for this producer-only export. This is distinct from the representation/version choices above, which need no spec amendment.

## Focused selectors for Root

These are selectors to run through the authorized `tools/mg` jobs, not execution receipts:

```text
package(meguri-sync) & test(/epoch_crt_/)
package(meguri-host) & binary(cloud_publication_phase) & test(/epoch_crt_/)
package(meguri-host) & binary(cloud_adoption_guards) & test(/^epoch_crt_adoption_records_actual_complete_epoch_knowledge$/)
package(meguri-host) & binary(cloud_policy_driver) & test(/^epoch_crt_legacy_runtime_keeps_unknown_attention_conservative$/)
```

Added tests cover legacy/media-root absence after migration, version/scope guards, fetched control qualification, inbox/ledger rollback, real Host publication and reopen, real Host fetched changes and restoration through both intakes and reopen, real child Host adoption, and the existing prebound Host's unknown attention. Existing named tests remain; three storage-test calls only receive the new optional producer argument. Existing publication state budgets and adoption step budgets are retained, with no failure suppression or retries.

## Explicit remaining P6 integration

- Consume current local crt at bind/open, after committed col changes (including real undo) and from reconcile/barrier snapshots. The actual existing producers are `snapshot.col()?.created.0` and `ObservedCommit.collection`'s `CollectionMark.crt` (decoded from the same committed col image). No new ABI is needed to consume those facts; never substitute mod/scm.
- Feed the durable optional epoch fact and real current local crt into `Summary.crt_mismatch`, including immediately after a qualified fetched update. Keep all other attention facts conservative and `attention_known` false.
- Compare at every barrier and on the writer immediately before every CloudIncoming/CloudRekey effect. Recheck already-issued work, preserving Root/epoch/account/generation/file/incarnation fences. A mismatch leaves the engine running and the binding active.
- Connect Sync Now's `recoveryRequired` result; clear only the crt mismatch when real equality returns, including undo. Preserve V14/V15 recovery offers and existing attention conditions.
- Consider qualified complete epoch record-ID reads (`adapter_complete_fetch_read`) and `ReadOwnEpoch` as further real producers where required. Their existing existence/root-confirmation behavior is untouched; mechanical adapter options explicitly carry no new evidence from those paths in this step.
- Add the requested complete Runtime/Host P6 gate matrix: matched and absent legacy knowledge, mismatch durable at open, local crt mutation, fetched epoch mutation, already-issued barrier/apply denial, and equality/actual undo. The tests exported here prove producer/ledger behavior, not that gate matrix.

## Changed paths

- `core/crates/meguri-sync/src/cloud/AGENTS.md`
- `core/crates/meguri-sync/src/cloud/mod.rs`
- `core/crates/meguri-sync/src/cloud/epoch_crt_store.rs` (new)
- `core/crates/meguri-sync/src/cloud/publication_store.rs`
- `core/crates/meguri-sync/src/cloud/publication_runtime.rs`
- `core/crates/meguri-sync/src/cloud/adoption_store.rs`
- `core/crates/meguri-sync/src/cloud/adapter_store.rs`
- `core/crates/meguri-sync/src/cloud/adapter_runtime.rs`
- `core/crates/meguri-sync/src/cloud/fetch_read_runtime.rs` (mechanical empty evidence options)
- `core/crates/meguri-sync/src/cloud/incoming_store.rs`
- `core/crates/meguri-sync/src/cloud/incoming_runtime.rs`
- `core/crates/meguri-sync/src/cloud/runtime.rs` (read-only test hook)
- `core/crates/meguri-sync/src/cloud/workflow_store_tests.rs`
- `core/crates/meguri-sync/src/cloud/conflict_expiry_tests.rs`
- `core/crates/meguri-host/tests/cloud_publication_phase.rs`
- `core/crates/meguri-host/tests/cloud_publication_phase/epoch_crt.rs` (new)
- `core/crates/meguri-host/tests/cloud_adoption_guards.rs`
- `core/crates/meguri-host/tests/cloud_policy_driver.rs`
- this review/fix log


## Integration evidence

Filtered producer: `cleanroom/20261112T192832Z-91516`.
Independent review `cleanroom/20261112T194709Z-98067` found one P1:
the retained adoption reader still required publication V2. Repair
`cleanroom/20261112T195034Z-99561` updates it to V3 and extends the real
suspend/close tests. Independent review `cleanroom/20261112T195438Z-4939`
reports no P0–P2 for that repair.

Compilation then found two missing optional fields in loss paths and a test
helper called on the outer rig rather than its child. Filtered repairs
`cleanroom/20261112T195810Z-8508` and `cleanroom/20261112T200726Z-14213`
add explicit None values and forward the existing read-only child command.
An initial over-anchored filter selected no test and failed; it is not evidence
of a pass. The corrected selection below exercises the actual suspension.
Final independent review of these small compile repairs follows the WIP commit.

- RED `core-test/20261112T200939Z-15382`: the retained-adoption regression
  fails at suspension with the old V2 reader, while the installed schema is V3.
- GREEN `core-test/20261112T201002Z-15706`: all ten focused tests pass with
  the V3 reader; the same regression now suspends and closes successfully.
- `core-test/20261112T201054Z-16566`: all 319 affected sync, publication,
  adoption, policy and FFI connection tests pass.
- `core-lint/20261112T201150Z-17135`: correctness, layering and host scan pass.
- `private/20261112T200914Z-15040`: all 23 prepared files pass.
- `static-checks/20261112T201149Z-16914`: three checks pass.
- `similarity/20261112T201149Z-16921`: 22 source files pass, no exceptions.

No unit trailer or full gate is claimed. The comparison/admission and complete
status work remain open. Pending batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
