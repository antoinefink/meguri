# Acknowledged media and case spelling

Status: clarification reviewed; implementation and verification pending.
Component start: `a05b642648bf10190931b1ad6ea47391c4d57e16`.
Spec writer: `s-20261102T011456Z-f4a4e630`.
Contract: `sync/cloudkit-records#K15.9` and the same `sync/policy#P24` item.

The naming test found an extra local revision on an acknowledged destination.
Diagnostic run `core-test/20261102T014915Z-48019` located the failure after the
move's tracking commit, while its gate was held. K15.9 already requires
that live destination to remain clean when its acknowledged content matches
and it is not withheld.

Review of the repair found two incompatible shortcuts: comparing only SHA-256
could leave different current and ACK canonical images marked clean; requiring
exact filename equality added a spelling exception absent from K15.9.
The clarification preserves the exact ACK image when the case key, both content
hashes and size match. Actual stored spelling and observed file facts remain
separate. Unchanged facts do not create another Cloud edit; later real changes
still follow normal tracking.

Independent reviewer: `native_bounds_review`, no P0–P2 findings in this
clarification. It adds no pending exception, synthetic ACK or acceptance waiver.
The P24 item stays open, including its Q6 and fault-lane requirements.
No passing implementation result is claimed by this spec commit.
