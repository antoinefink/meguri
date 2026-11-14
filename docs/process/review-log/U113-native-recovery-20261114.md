# Native recovery contract review — 2026-11-14

Approval: reviews-cloud-stabilization-approval-20261114.md. Original start:
`6fbc9a24`; integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Both additional rounds are used. No source or approved spec changed.

Round one: `cleanroom/20261114T081442Z-64290`, read-only audit and a draft.
The reviewer rejected its general historical-proof requirement and found
ordinary recovery incomplete. The revised proposal is Under review.

The second round started at `7a80be65`. Independent spec-writer session
`s-20261114T083432Z-b1d738f4` reviewed the revision. Verdict: HOLD, no P0.
The report is retained locally at
`build/cloud-resume-diagnosis/U113-native-recovery-independent-20261114.md`.

P1: a type with three fields and no notes admits two histories: Meguri renamed
fields, or another writer reordered them. Their final rows match; their maps
differ. After restart, the rows and file metadata do not resolve this case.
The approved contract supplies no authority to choose the prepared map.

A behavior decision remains: whether retained preparation may impose its
ordinal interpretation in this exact case. No such authority is assumed.
The draft also needs precise acceptance, Abort, successor-admission and
final-stamp handoff rules. Further repair stays held at the recorded limit.

No test, Unit trailer, P24 closure, gate pass or release acceptance is claimed.
