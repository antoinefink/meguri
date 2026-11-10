# Pending update delivery — WIP

Start: `7b3455c8`. Batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Fixer session: `s-20261103T133750Z-f276f58e`.

The normal UpdatePending executor now retains its actual Core output, then
uses the existing adapter to deliver the queued lists to the SDK. The adapter
keeps its owner checks, callback ordering and acknowledgement rules. No network
request or timer change was added.

`swift-test/20261110T015125Z-39025` compiled five new test functions. Failure,
cancellation and stale-owner cases passed. Four positive parameter cases and
the callback-order case failed: they observed two lists per edit where the new
tests expected one. Those assertions remain enabled pending source review.
No unit, gate or signed exchange acceptance is claimed.

Independent source review found no P0–P2 product finding.
`swift-test/20261110T015213Z-40647`: all six parameter cases passed across
three SDK workflow tests, including automatic delivery, a later addition during
an empty callback, cancellation and release. The assertions and bounds from
the earlier failed run were retained.

Source review explains the new Bridge test failures: normal tracking already
emits a Full list before explicit UpdatePending emits its list. The tests must
measure the command at its actual boundary and retain proof for every original
list and its actual producer. That test correction remains open.
`similarity/20261110T015132Z-40201` passed. Source privacy passed; the shorter
log is checked separately before commit.
