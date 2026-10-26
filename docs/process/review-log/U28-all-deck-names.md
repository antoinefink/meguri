# U28 — all-name query for Go to Deck

Supplemental starting commit: `1df3bcea29004e49565d0bd002c076aff180ddeb`.
Intake: `build/unit-starts/U28-all-deck-names-20261026.json`.
The U156 start is recorded separately. The gate base is still
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

The scoped API/prepared-work audit finds no complete exported name list.
Display trees hide empty Default and omit missing-parent records. D17 adds
a separate generation-tagged, read-only projection of all persisted records;
the display-tree contract is unchanged.

Spec-writer session `s-20261026T041939Z-a8f043ce` uses only public Meguri
contracts. No reference, oracle, private or Anki source is read. Independent R1
clears the substance and requests R26/R27 in the Requirements header. That
metadata fix is the sole second edit; R2 closes it with no further finding.

Implementation, focused query/generation/purity checks, Swift bridge checks
and the native switcher remain pending. No unit or integration acceptance is
claimed by this spec addition.

## Implementation and review

Filtered implementer `s-20261025T134357Z-235af9a2` produced
`cleanroom/20261026T050419Z-85824` from `9394489c`. It uses the existing persisted
row reader, unicase ordering and human-name conversion. One Query snapshot owns
both the rows and their collection generation. Swift maps immutable DTOs through
QueryLane and registers the exact bridge interval. Display-tree behavior is intact.

Independent R1 (`/tmp/meguri-u28-all-names-review-r1.json`) found one P2 proof gap:
a required storage/decode failure was not exercised. Filtered fixer
`s-20261025T162405Z-9dc656e4`, export `cleanroom/20261026T051642Z-99177`, extends
the existing serialized fixture with a malformed persisted row after valid rows.
The public query must return Internal, leave database/undo/event state intact,
and return the original exact snapshot after restoration. It also removes the
comment anonymously flagged by privacy. R2 closes the P2 and finds no further
required repair. No new production injection hook or behavior was added.

## Focused evidence

- `core-test/20261026T051320Z-93082`: two passes with default features; this does
  not cover the feature-gated D17 integration helper.
- `core-test/20261026T051401Z-94664`: one pass with FFI test-hooks, including
  complete projection, purity, coherent rename, absent Default and reopen.
- `core-test/20261026T051905Z-2494`: two passes with FFI test-hooks after R2,
  including the actual malformed-row error and exact restoration.
- `swift-test/20261026T051351Z-93790` and
  `swift-test/20261026T051923Z-3180`: final bridge, open/read/close/reopen and
  interval registry pass. Each log records seven registry tests and five bridge
  tests in separate executables; the MG summary reports the last executable's five.
- `private/20261026T051908Z-2767`: pass for the final seven code files.

The supplemental query is implemented; full U28, U156 and integration acceptance
remain open. Required full-gate base remains unchanged. No Unit trailer is added.

Final `core-lint/20261026T052037Z-4912` passed all four checks.
`similarity/20261026T052040Z-5168` passed for the repaired query delta.

## Typed-name lookup for D18

Supplemental base: `6433086c`. The filtered implementation
`cleanroom/20261026T073757Z-31411` adds one Query snapshot result containing its
actual generation and an optional actual deck entry. It uses existing D1 input
conversion and D2 comparison across persisted records. It makes no writes.
The Swift QueryLane wrapper and exact interval registry test are updated.
Independent D18 implementation R1 and R2 find no issue in this query path.

`core-test/20261026T075226Z-40311` passes both focused FFI tests with test hooks,
including actual canonical matches, hidden and filtered records, read errors,
no state or event changes, and closed/reopened generations. Rust formatting
initially fails in `core-lint/20261026T075424Z-43461`; formatter
`core-format/20261026T075531Z-44104` changes only test layout.
`core-lint/20261026T081551Z-57917` passes all four checks.

`swift-test/20261026T081810Z-59601` passes the selected registry and actual bridge
checks across their test executables. Its final executable reports five tests;
the registry executable reports seven. Privacy for the six lookup files passes
separately; similarity `20261026T081244Z-52307` passes for the whole candidate.
The picker continuation remains a separate UI change. No full U28 or batch
acceptance is claimed here; the original integration base still applies.
