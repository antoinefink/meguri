# Native predecessor fixture

Start: `506147c63678ef3556a461f28d4625ca4c1fc094`.
Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/policy#P4`, P14.2; `engine/jobs#J2.1.1`.

The existing predecessor regression now enables real automatic sync before selecting its SDK. With automatic sync off, the Paused state masks the exchange-completion status transition that issues command B. Changing that mode replaces the SDK, so the fixture must settle configuration before capturing its waits and holds.

All original assertions, ten-second signal limits, provider/native holds, actual A/B results, ACK order, account projection, SDK validation and successor checks remain unchanged. Production and harness error handling are unchanged. The test documentation records the engine-selection trap.

Author `cleanroom/20261113T200706Z-62679` first enabled automatic mode after SDK selection. Review `200936Z-63605` found that retired-instance mistake. Native `swift-test/20261113T200947Z-63976` failed at the original fetch hold and retained two issues, including failure cleanup. Repair `cleanroom/20261113T201506Z-66264` moves configuration before selection. Review `201637Z-66788` finds no P0-P2 and confirms every original assertion remains.

`swift-test/20261113T201647Z-67166` passes the original test, one of one, in 3.3 seconds. It reaches actual Core predecessor waiting, both original result ACKs, account projection and a usable reopened owner. This is a setup correction, not a production ordering fix or a weakened expectation. Earlier failed receipts remain saved.

The parked-flush deadline and cancellation-classification failures remain open. Their saved audit identifies missing original exchange proof provenance and a distinct flush witness gap. The batch gate and broader native suites remain due. No Unit, milestone or release acceptance is claimed.

Static checks `20261113T202025Z-69640` pass all three checks. Privacy `20261113T202025Z-69661` passes the three owned files before this receipt note; the enabled hook checks final staged content. Similarity `20261113T202024Z-69375` passes the Swift file without flags or exceptions.
