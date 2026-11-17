# Checked-template focused verification

Frozen source: `f4770d47eab63e8965e5514a01f052ef653a9fde` on local master.
Actual receipt: `core-test/20261117T064105Z-42939` on that clean commit.
Result: 22 passed, 0 failed, 0 skipped.
The run includes 8 new API tests and 14 earlier regression tests.

The command enables `meguri-engine/test-hooks`, selects
`--test notetype_field_rename`, and filters the exact engine package and
binary. All eight `template_formats_` names are individually PASS, including
precommit rollback, stale plan/writer, cancellation, metadata and no-op,
front-only generation, live rederivation, plain-save guards and undo/redo.

Source reviews, provenance and scoped checks are recorded in the unchanged
[implementation record](template-format-domain-20261117.md). The verified
public signature and receipt were handed to the server worker. Its required
normal round-trip cases remain that worker's actual verification task.
No Unit completion or passing app gate is claimed; the app Resume proof
method remains failed in the failure register. No remote push occurred.

## Complete public-content classification

Snapshot decision: PUBLIC for all 4 files. Independent non-author
`/root/provider_public_snapshot_review` inspected each complete file with all
comments, literals, embedded data and paths. Root verified the bytes and hashes.
The review identified no actual or uncertain private values or unresolved
source/license concerns given the recorded clean-room provenance.
No token values, forbidden sources or raw similarity inputs were read.
This classification grants no technical acceptance.

| Frozen path | Lines | Bytes | SHA-256 |
|---|---:|---:|---|
| `core/crates/meguri-engine/src/notetypes/mod.rs` | 490 | 17945 | `6a7e726e643509d820365b2cad75301103e510f14519dd6d6036321488a040ba` |
| `core/crates/meguri-engine/src/notetypes/template_formats.rs` | 227 | 8146 | `2a4d8da5991de243f26387ac79758b86e7e1ec6ba72dbc410ae5bf333823a32a` |
| `core/crates/meguri-engine/tests/notetype_field_rename.rs` | 1487 | 66095 | `14626df1fe23f825bff05af4e848782c9932381d09324ea7247ef52324d33acf` |
| `docs/process/review-log/template-format-domain-20261117.md` | 34 | 1906 | `98a99e451ac9ed412811bd7e2486614172a3f27c2aec9d6479d3f383ef2268bd` |

Full local review: `build/cloud-resume-diagnosis/template-api-public-frozen-f477-review-20261117.json`.
The frozen implementation record stays unchanged; this separate file records
its later actual test evidence. Root changed no privacy matcher or admission
map for these files. Exact admissions, if needed elsewhere, retain the normal
independent-review and full-file-hash rules.
