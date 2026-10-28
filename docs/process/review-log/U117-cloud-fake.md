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
