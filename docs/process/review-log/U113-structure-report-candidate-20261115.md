# Structure-report prerequisite: held candidate

Start: `20cddcab4b269f06b3cae68219755406f8398bb3`, additive U23/U24/U25/U28/U29 work. The pending full-gate base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. No Unit or acceptance claim.

The candidate adds authored maps and reasons, a local commit guard, retained observer payloads and undo/redo reports for S2.5/M9.11. It does not enable generation sending or close P24 items. It was removed exactly from the main checkout after testing; the exported source and findings remain in the local build artifacts. Master does not contain this candidate.

Author `cleanroom/20261115T140318Z-46083`; compile correction `cleanroom/20261115T143412Z-84078` adds only the required observer name. The integrator read the full 18-file export and verified the correction's exact bytes. Independent source review `cleanroom/20261115T142922Z-78257` found:

1. P1: `journal_images.rs` still calls the removed ordinal-report API. Migrate it without losing rollback, release or raw-write coverage.
2. P1: the new guard rejects the existing incoming field-rename transfer. S2.5 exempts only the Cloud sources, while normal-sync N5.2 admits equal-count aggregates regardless of other differences. Counts cannot prove ordinal origins. Settle the contract before changing that caller; do not guess identity maps or silently exempt it. Frozen transfer code still needs compatibility preservation.
3. P1: after an operation reports a swap, a raw swap back can leave the old map in its report. Equal row names do not prove map authority. Require another authored contribution.
4. P1: large sparse ordinals can allocate dense maps outside the journal spill budget. Bound aggregate allocation and handle failures, with an explicit policy that preserves supported legacy ordinals.
5. P2: deliberate fixture writes in `changeset`, `notetype_records` and `stock_kinds` need authored contributions. Retain their assertions and separate negative tests.
6. P2: direct cloze-map validation rejects a valid composed map on Redo. Distinguish admission of primitive contributions from retained composed reports; exercise repeated undo/redo.
7. P2: a standard-to-cloze-to-standard chain can synthesize the wrong destination arity. Validate composed domains without inferring origins.
8. P2: adding a type and saving or renaming it in the same operation is rejected, despite needing no transaction-start layout.

The review also requires explicit token nesting semantics and coverage. Missing-origin repair coverage must follow the actual maintenance operation's undo class; no inverse map to an absent layout may be invented.

Verification retained:

- `core-test/20261115T143102Z-80191`: missing-API compile baseline, also affected by the fixture's missing observer name. Not behavioral RED.
- `core-test/20261115T143252Z-82328`: candidate compile stopped at that observer name; corrected by the isolated author.
- `core-test/20261115T143443Z-84726`: PASS, 106 storage, editing, schema, preset and undo tests, including the spill-memory checks.
- `core-test/20261115T143553Z-85694`: nine new contract tests pass, three fail. Two fixtures have invalid cloze backs; the earlier-unreported-write case does not reach its expected typed error. Preserve the checks while making the fixtures valid.
- `core-test/20261115T143718Z-86113`: FAIL, existing local-stub rename exchange. No external service was contacted.
- `private/20261115T143949Z-90056`: two comments flagged; not cleared. The privacy guard is unchanged.
- `similarity/20261115T143953Z-90379`: PASS, 17 files scanned, zero flags and one document out of scope; filtered implementer view read.

These findings block integration of this candidate. Resume its prepared work after the immediate native-review checkpoint; do not rebuild it from scratch. Full structural feature coverage, normal generation integration and required gates remain open.
