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
list and its actual producer. That test correction is prepared below; verification remains pending.
`similarity/20261110T015132Z-40201` passed. Source privacy passed; the shorter
log is checked separately before commit.

Fixture-only repair: clean invocation start `0b6801ee` (supplied by parent;
no Git inspection). Recorded fixer in the filtered clean room. The two positive
Bridge functions now capture the exact boundary after tracked addition and
command issue, before execution, and require one command emission. Expected
lists retain tracking Full followed by command Full/Changes for each edit
(two lists in the parameter cases; four in the held callback), with exact
ordered SDK application and per-producer addition acknowledgements. Changes
expects empty adds because the tracking Full already updated desired state.
The failed `39025` receipt above is preserved. Product code, SDK tests, native
policy guards, failure/cancellation checks and existing bounds are unchanged.
One source consistency check only; no build, tests, Git or network run here.
Parent owns focused verification of five Bridge functions/eight cases and
inclusive regressions, independent review and provenance/privacy/similarity
checks. No extra SDK six-case rerun without a new concern; no Unit/full gate or
signed-exchange claim.

## Corrected producer-boundary verification

Independent test review found no P0–P2 finding. The corrected tests retain every
tracking and command list, isolate the command's single emission, and check
acknowledgements against each actual producer. Root moved one async read out of
a Boolean autoclosure to compile the same assertion.
`swift-test/20261110T060227Z-50327`: all five functions passed (eight parameter
cases). Earlier failures 39025 and the compile failure 46475 remain recorded.
`private/20261110T015714Z-45852` and `similarity/20261110T015720Z-46131` passed.
The real review/media/reopen test also passed in
`swift-test/20261110T015609Z-43861`. No signed exchange is claimed.
