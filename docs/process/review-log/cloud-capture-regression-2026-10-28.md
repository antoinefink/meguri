# Cloud capture regression repair

Status: focused repair verified; full integration acceptance remains open.
Starting commit: `efcf26cabdcb42653e6adff2811249aff1d97340`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

`gate/20261028T182013Z-62460` found one Rust test failure, also present in
native testkit verification. The test renamed a deck without changing its kind
or parent, which the expanded stable-layout capture correctly supports.
The failed receipts remain: `core-test/20261028T182220Z-94791` and
`conformance/20261028T184811Z-7483`.

Filtered repair `cleanroom/20261028T184708Z-6224` uses a valid notetype
sort-field transition with matching note sort values, and a valid deck-parent
transition. It preserves the invalidation, later supported capture, close and
reopen checks. It adds a production flat-rename case that checks the payload,
revisions, pending save, envelope and durable flight. Independent R1 is clear
of P0-P2. No production policy or assertion limit changes.

Verification on the formatted patch:

- `core-format/20261028T200124Z-18730`: pass.
- `core-test/20261028T200201Z-19280`: all 110 selected CloudKit tests pass;
  3,650 unrelated tests excluded by the explicit filter.
- `private/20261028T200322Z-20195`: pass.
- `similarity/20261028T200325Z-20439`: pass.
- `core-lint/20261028T200328Z-20708`: three of four checks pass. Clippy
  reports two redundant borrows at the new helper calls; a mechanical repair
  remains before this checkpoint is accepted.

The full gate was stopped after its known failures and loss of VM access;
it is cancelled, not passing or complete. The retained server-tooling failures,
R554 audio failure, snapshot differences, credential entitlement failure and
missing/queued VM results remain separate outstanding evidence. The dedicated
VM was restarted only after the verification job ended.
`doctor/20261028T200115Z-18333` confirms its SSH and Xcode are available again.
Catch-up requires a clean checkout. The first dirty-tree attempt was refused;
`catch-up/20261028T200558Z-22170` passed on the committed WIP, with nothing queued.
This does not complete the cancelled gate or its missing VM checks.

## Final mechanical correction

`cleanroom/20261028T200555Z-21866` removes only the two redundant borrows.
Independent R2 confirms equivalence and unchanged assertions.

- `core-format/20261028T200758Z-23268`: pass.
- `core-test/20261028T200813Z-23597`: all seven affected regression tests
  pass, zero skipped. Other inputs covered by the earlier 110-test selection
  are unchanged.
- `core-lint/20261028T200902Z-24085`: all four checks pass.
- `private/20261028T200904Z-24479`: pass.
- `similarity/20261028T200904Z-24336`: pass.

The fixture failure is repaired. The full integration gate and the separately
recorded release evidence remain outstanding; no Unit trailer is added.
