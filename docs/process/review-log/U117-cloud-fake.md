# U117 — shared CloudKit fake in progress

Status: incomplete; no Unit trailer or full fake-lane acceptance.
Starting commit: `f7a83f2b`; recorded before work in
`build/unit-intake/U117-cloud-fake/start.json`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This additive service work supports U113; existing transfer work stays frozen.

Filtered author: `cleanroom/20261028T034439Z-39349`.
The first slice supplies scoped service records, conditional writes, zones,
limits, quotas and stable fetch pages. The engine, scheduler, device harness,
coordinator, fault and signed lanes remain unimplemented.

`core-test/20261028T035759Z-49411`: twelve public service tests pass, alongside
seventeen separate codec tests; no skipped tests. Small fixtures do not prove
media or large-collection behavior. Independent review 1 found P1
`FAKE-WHOLE-STATE-PAYLOAD-GROWTH`: one JSON value duplicates asset bytes in
records, changes, outcomes and fetch snapshots. A valid large asset can exceed
the SQLite value limit; repeated fetches retain more copies. Repair is pending.

`similarity/20261028T035800Z-49642` clears the supported source.
`core-lint/20261028T035900Z-50556` has two documentation indentation findings
in the service; its other checks pass. `private/20261028T035800Z-49640` flags
four fake-service files. The filtered repair must close those findings without
changing guards, thresholds or behavioral assertions.

The complete fake, production capture/scan/send path, P24/Q11, U117 acceptance
and all required integration evidence remain open.

## Normalized service repair and focused closure

Filtered repair `20261028T040218Z-52139` replaces the single state value with
scoped SQL rows and immutable version references. Encrypted bytes and assets
are stored once; current records, changes, outcomes and snapshot members use
references. Named-field reads load only requested bodies. Paged inspection is
available; the compatibility full view checks its row/byte limits before
materializing records. Unknown database formats are refused.

Review 2 closes the payload-growth finding. Compilation exposed unsigned SQL
reads (`core-test/20261028T041215Z-57474`). Filtered repair
`20261028T041553Z-59655` adds checked signed reads; review 3 is clear.
The next compile exposed unsigned bindings (`20261028T041838Z-62493`).
Repair `20261028T042058Z-63892` checks writes and arithmetic; review 4 is clear.
The subsequent build exposed two inferred usize reads
(`20261028T042437Z-68150`). Combined repair `20261028T042650Z-69659` checks
those counts; review 5 is clear. None of those builds ran runtime tests.
The signed-boundary rule is recorded in `core/AGENTS.md` (`4a16ce6b`).

- `core-test/20261028T042812Z-71131`: all seventeen service tests and ten
  separate observer tests pass, zero skipped. Includes actual 100 MiB assets,
  repeated 256 KiB fetches with physical payload-count checks, 1,200 records,
  reopen/page continuation, malformed integer rejection and original behavior.
- `private/20261028T042813Z-71353`: all nine combined inputs clear.
- `similarity/20261028T042813Z-71357`: supported combined source clear.
- `static-checks/20261028T042915Z-72485`: pass.

`core-lint/20261028T042914Z-72246` retained three final findings: an unused
private method, tuple complexity and a nested conditional. Filtered repair
`20261028T043240Z-73904` removes that method, adds a private tuple alias and
preserves the asset-read short circuit with a let-chain. The seventeen tests
are unchanged. Independent review 6 confirms equivalence and no unresolved
P0–P1 finding in this service slice.

- `core-lint/20261028T043423Z-75307`: all four checks pass, also covering the
  committed accessor without changing its source.
- `private/20261028T043426Z-75799`: both final files clear.
- `similarity/20261028T043425Z-75536`: both final files clear.

Metadata expiry/GC, seeded delivery order, engine instances, scheduling,
coordinator/device harness, injections, staging lifetimes, full 40 MB/150k
workloads and Q5/fault/signed lanes remain open. These focused tests do not
accept P24, U117 or a full integration gate. The original batch base is kept.
