# U110 — offline conformance registration repair

> **2026-10-26: Anki server work is frozen indefinitely.** Earlier next steps
> and acceptance requirements below are retained history. Do not resume them
> without Antoine's explicit request. See [the delivery scope](../review-first-delivery.md#anki-server-freeze--2026-10-26).

Supplemental starting commit: `da8ebea1e1d65cfdf6b47d29736dd6470997a27d`.
Recorded before work in `build/unit-starts/U110-offline-registration-20261026.json`.
Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The earlier U110/U107/U111 starts, failures and open live checks remain retained.
This is a bounded offline repair, not full U110 or live media acceptance.

The seven baseline Rust failures recorded in `integration-lint-20261026.md`
came from six valid published media scenarios that the model could not parse,
and a stale expectation about an already registered review-session adapter.
Existing contracts `conformance/media-sync-ops` V1–V4 and `sync/media-tracking`
T4/T11 define the required behavior. No new spec or protocol research was needed.

## Implementation and independent review

Filtered implementer `s-20261025T134357Z-235af9a2`, export
`cleanroom/20261026T033253Z-32345`, adds strict vocabulary and actor-local login
validation. The real local tracker supplies quick-check and reset. Quick-check
keeps the folder-marker fast path; the separate full pending comparison still
forces scanning. Reset preserves files, facts, trash and baseline ownership.
Hosted operations pin the collection through a Maintenance reader and the
existing generation/media-job admission; no nested Query lease is retained.

`media_sync` is parsed but remains absent from the executor registry. The two
live reset scenarios are valid and pending, rather than invalid or falsely
passing. No live request, implicit collection transfer, fake counter, new
transport or activation policy is added. Published scenarios, goldens, schemas
and deviations are unchanged.

Independent R1 finds one P2: the required CLI capability consumer still expects
the old exact registry and language. Its focused reproduction is
`core-test/20261026T033956Z-36261`: one failed test after successful compilation.
The offline adapters, ownership, validation and new regression assertions
otherwise review clear.

Filtered fixer `s-20261025T162405Z-9dc656e4`, export
`cleanroom/20261026T034140Z-37654`, updates only that test. Exact arrays now
include the actual registered operations and observation; language count is 76.
An explicit assertion keeps media sync language-only and unsupported. Existing
exact equality, unsupported backup and filtered-answer checks are preserved.
Independent R2 closes the finding with no further P0–P2 issue.

## Focused evidence

- `core-format/20261026T033926Z-35773` and `034305Z-38574` apply only
  standard formatting to the owned source.
- `private/20261026T034010Z-36571`: PASS for the six testkit files.
- `similarity/20261026T034011Z-36813`: PASS for those files.
- `core-test/20261026T034334Z-39042`: PASS; 1,091 passed, zero failed, one
  skipped. All seven original failures are resolved. The new raw/hosted tests
  prove exact hashes/deletion values, marker skips versus full scans, reset
  preservation and busy-operation refusal. CLI consumers pass as well.

The scenario-format tests passing for live reset assets prove valid loading and
honest missing-capability reporting, not live execution. Nextest also records
one leaky-case diagnostic for the unchanged five-megabyte-template generator;
the MG result passes. No assertion, limit, retry policy or skip was changed.
Retain that diagnostic with the run.

- `core-lint/20261026T034524Z-39767`: PASS; formatting, Clippy, layering and
  policy checks all pass.

- `private/20261026T034630Z-40458`: PASS for the CLI fixture and log.
- `similarity/20261026T034631Z-40689`: PASS for the CLI fixture.

The original-base
integration gate and U110/U107/U111 live checks remain required. No Unit trailer
or milestone acceptance follows from this repair.
