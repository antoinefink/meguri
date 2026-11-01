# Local conflict-copy expiry checkpoint

U113/U114 remain unaccepted. No Unit trailer is added.
Starting commit:2f0e9751004834a9581d98455859cb5425f166e6.
Applied after the disjoint native transport commit987d7270.
Original integration base:1416a520542a8c5daa38a23bfe398da704c3d6c9.
Contracts: sync/policy P10.3 item4; sync/cloudkit-recovery V2.4 and V17.3/V17.6;
conformance/cloudkit-acceptance Q11.

One transaction expires resolved copies at the 30-day cutoff across all owners.
It retains younger, unresolved, future and overflow cases. An isolated or salvage
owner loses its row only with its last copy and no remaining work. Active owners
stay. The code never calls remove_work to force an empty owner.

Owned staged-file unlink evidence commits with copy removal. File identity and
directory identity are checked before unlink. Directory sync precedes journal
removal. Replay retains evidence after failure. Ledger, inbox, apply-intent,
other copies and recovery-operation references protect staged bytes. The open
path performs local recovery and expiry before requiring verified identity.

Source cleanroom/20261101T061905Z-22547 used implementer session
s-20261025T134357Z-235af9a2; tests were authored first. Fixes used
s-20261025T162405Z-9dc656e4 in cleanroom/20261101T063613Z-35128 and
cleanroom/20261101T064210Z-43290. Source copies had no reference/private trees.

Independent source review: three rounds complete, no repair findings remain.
The first failed run reached an invalid test setup. The second failed run expected
zero startup identity reads. The final test compares cleanup opens with a real
open before cleanup: each reads identity once, stays unverified and cannot send.
Production identity checks are unchanged.

Three added storage tests protect staged bytes through expiry and replay when
a ledger, inbox or media apply intent holds them. These are typed local storage
fixtures, without a server ACK or UI resolution claim. SIGABRT and directory-sync
failure tests provide process-crash evidence; hardware power loss is unproved.

| Run | Result |
| --- | --- |
| core-test/20261101T063243Z-32790 | 56 pass, fixture setup fails |
| core-test/20261101T063953Z-41598 | 59 pass, identity baseline assertion fails |
| core-lint/20261101T063420Z-34131 | Pass before fixture repairs |
| core-format/20261101T064504Z-46243 | Pass |
| private/20261101T064557Z-47653 | Pass, 13 files before these notes |
| core-test/20261101T064536Z-46934 | Pass, all 60, none skipped |
| similarity/20261101T064551Z-47222 | Pass, 12 source files |
| core-lint/20261101T064842Z-49233 | Pass |
| private/20261101T064903Z-49959 | Pass, review notes |

RunCleanup remains unavailable until its other work and retained native inventory
execution are implemented. Blob cleanup, full app sync, signed platform evidence
and the original batch/milestone gates remain required. P24 stays open pending
all named cases and the spec-writer closure.
