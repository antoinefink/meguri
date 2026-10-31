# Native conditional-save conflicts

This slice starts at `2df9441127eb989a81b3b6085abd3cd283cebd80`.
The original integration batch base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Filtered tests-only export: `cleanroom/20261031T221912Z-88842`.
Three regressions use actual SDKRig conditional Service replies: disjoint inline
edits, identical content from another writer, and a lost older save followed by
a newer local edit. They preserve collection intent, WAL, ACK and resend checks.
Test helpers expose actual transport and latch operations; routing is unchanged.

`core-test/20261031T222422Z-90457` compiles all three and fails all three at
`deliver_sent_event`. Independent review confirms the exact cause: the SDK error
becomes `Other`, with no server record passed to the core. `Runtime::sent_results`
then returns `Incomplete`. The fixtures already confirmed the root, built the original request and
proved the actual conditional Service conflict. This is the intended baseline
failure, rather than a missing fixture prerequisite.

Independent test review finds no required fixture repair. Keep the failing
boundary and subsequent assertions. Generic routing must preserve original
batch/handler authority. An older own-landed save needs retained-flight proof;
it cannot be passed off as a Saved reply for the current envelope. Reuse existing
incoming merge and ACK bookkeeping instead of replacing either algorithm.

Production routing, focused green verification and further independent review
remain pending. This is a WIP test checkpoint, not a passing unit or gate.
