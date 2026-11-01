# Guarded zone inventory checkpoint

U113/U114 remain unaccepted.
Starting commit:8d78ad36234c998fafdfadcb406f99cf8f4a4113.
Original integration base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Contracts: sync/policy P10.3 and P24; sync/cloudkit-recovery V26.1;
conformance/cloudkit-acceptance Q11.

The request retains its original Runtime and Collection authority through a
listing. A successful zone removal invalidates earlier requests. Completion
consumes the request. An old result cannot restore the removed tracking row;
a later real listing can record a recreated zone. The testkit captures authority
before its Service call. The old names-only callback is gone.

Source cleanroom/20261101T060312Z-13071 used implementer session
s-20261025T134357Z-235af9a2. Eight files changed. No native transport or UI change.

| Initial run | Result |
| --- | --- |
| core-test/20261101T060107Z-11865 | Fail, Q11 line 303 |
| core-test/20261101T061012Z-15154 | Five pass, one fails |

Reviewer native_conformance_lifecycle_audit initially found no source issue,
then examined that runtime failure. Completion leased the old Collection before
checking Runtime ownership. A closed reader could fail before stale rejection.
The fix rejects foreign ownership before the lease and provider reads,
while retaining the later locked checks and exact error assertion.

core-format/20261101T060955Z-14733 passed.
similarity/20261101T061025Z-15742 passed the first patch.
private/20261101T061023Z-15460 found one comment region; its complete removal
changes no test behavior. No privacy exception is added.

The remaining integration work is recorded in
[the prior component log](cloud-delete-zones-native-execution.md#remaining-acceptance).
P24 stays open. No Unit trailer is added.

Fixer cleanroom/20261101T061136Z-16600, session
s-20261025T162405Z-9dc656e4, made those two repairs. Reviewer
native_conformance_lifecycle_audit cleared round 2 with no P0-P2 findings.

| Run | Result |
| --- | --- |
| core-test/20261101T061244Z-17676 | Pass, all six cases |
| core-lint/20261101T061330Z-18564 | Pass |
| private/20261101T061318Z-18249 | Pass, eight source files |
| similarity/20261101T061342Z-18890 | Pass, final source |

core-test/20261101T061407Z-19334 passed all 42 related Rust tests.
The cases include publication, adoption, intent replay, saved reviews, media
round trips and the FFI deletion boundary.
