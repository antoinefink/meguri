# Gate placement fixture follow-up

Start: `2deea6a46f36eed6e75731e2979d7499ff31da5e`.
Record: `build/unit-starts/gate-oracle-placement-consumer-20261026.json`.
Original integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Full gate `20261026T052152Z-6629` reached the tooling suite after passing static
checks, canaries, Rust lint/dependencies, Rust tests, native durability,
conformance, golden freshness and harvest reporting. The Rust receipt
`core-test/20261026T052325Z-15686` passed 3,096 tests with 12 skipped. Native
durability passed at `20261026T054708Z-23994`; conformance passed at
`20261026T054733Z-24561`.

The real-tier test failed at `tools/tests/gate.bats:1852`: its exact list omitted
`conformance-route-transport`, whose R1952 metadata had already been corrected.
The orchestrator verified the gate process and stopped it to repair this known
failure before further broad verification. The gate is cancelled/incomplete,
not passing. Its log retains the failed assertion. Remaining checks did not run.

Filtered fixer `s-20261025T162405Z-9dc656e4`, export
`cleanroom/20261026T055828Z-33101`, updates that ordered equality and adds an exact
placement/needs/oracle-files assertion. All existing entries remain required.
No production metadata, validator, trigger, budget or assertion was removed.

Focused `selftest/20261026T060131Z-35941` passes the previously failing test.
Full integration verification remains open at the original base. No unit or
milestone acceptance is claimed.

Independent R1 (`/tmp/meguri-gate-placement-consumer-review-r1.json`) is clear:
the exact list matches the manifest and the added check also rejects duplicate
route entries. The focused test and privacy scan pass; the full gate stays open.
