# Isolated reopen reconciliation — 2026-11-11

Role: fixer. Repair starts at `d40e3b49`; scalar diagnosis starts at
`df52885b`. Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/cloudkit-records#K11.4`, `sync/cloudkit-recovery#V2.2`, `#V9.2`.

## Cause and repair

The original Q6 crash test fails after Resume: the collection retains the
edited note and review, but tracking retains the earlier note without a mark.
Scalar diagnosis `core-test/20261111T111522Z-37298` shows tracking sequence 10
and collection sequence 3 sharing open counter 2 and generation 1. Isolated
reopen reused the prior counter. The scan therefore treated the old tracking
observation as a newer write and skipped the edit.

Every isolated collection open now atomically advances its observer counter
and clears the drained flag. It preserves file evidence and all other stored
state. Active opens, explicit Resume admission, account fences and protection
for newer writes are unchanged. The wrong-account test checks complete stored
state, pending entity and collection preservation across isolated reopen.

## Review and verification

Filtered repair: `cleanroom/20261111T111604Z-38239`. Independent review:
`cleanroom/20261111T111910Z-43819`, no P0–P2 findings; static review only.

- Before repair: `core-test/20261111T111237Z-28926` and
  `20261111T111522Z-37298` fail the unchanged content/dirty predicate.
- After repair: `core-test/20261111T111922Z-44844` passes both Resume cases,
  including the original actual-kill/300-pair recovery scenario.
- Broader regression `core-test/20261111T112155Z-46807`: 771/771 pass,
  including sync-core, native absence and FFI cancellation checks. Two existing
  150,000-row tests account for most of the run's 709 seconds.
- Lint `core-lint/20261111T113409Z-67589`, privacy
  `private/20261111T113411Z-68191`, and similarity
  `similarity/20261111T113410Z-67789` pass.

The sandbox-only attempt `20261111T111217Z-26837` could not reach Docker;
the subsequent host-authorized run reached and exposed the actual failure.
Failed receipts remain retained. No assertion, timeout or kill requirement was
weakened. This repair is not a two-device or milestone acceptance claim.

The integration batch still awaits the retained testkit stale-callback
regression, dependency advisories, native accessibility work, and a required
full gate. No unit or milestone is accepted by this commit.

## Full-suite expectation follow-up — 2026-11-12

Start: `d02886ffe330d49806b322b3617970230b585191`. Full Rust run
`core-test/20261112T015202Z-84026` passes 4,912 cases, fails two and skips
ten. The historical-birthtime case still expects the old open counter.
Every preceding file-mismatch, isolation and scan-refusal check passes.

Filtered repair `cleanroom/20261112T020838Z-85550` derives the expected
post-open state from the original historical state, advances its counter
once with checked arithmetic and clears the drained flag. Both full-state
comparisons and all file, pending and ACK comparisons remain. No production
code changes. Independent review `021036Z-86099` finds no P0–P2 issue or
weakened preservation check. Focused `core-test/20261112T022841Z-92381`
passes the corrected case.

Gate `20261112T014846Z-84391` is cancelled after those two failures and the
255 passing native durability checks, to repair before further integration
work. Its incomplete result and all failure evidence remain. It accepts no
unit or milestone; a complete gate at the original integration base is still
required. The separate memory-limit failure is not resolved by this change.

Refreshed lint `core-lint/20261112T022949Z-92697`, static
`static-checks/20261112T022952Z-92969` and scoped similarity
`similarity/20261112T022956Z-93284` pass. No guard exception is added.
