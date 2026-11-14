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

## Owner policy direction

See `cloud-recovery-product-decision-20261114.md`: automatic recovery where
safe, otherwise an explicit local-or-iCloud collection choice. This settles
the user-facing policy. The detailed native recovery contract, implementation
and tests remain open; the draft and repair limit are unchanged.


## Approved extension: rounds three and four

Authority: `cloud-recovery-extension-20261114.md`. The counts are preserved.
Round three produced the reviewed K13.4/V13.6 contract, committed as
`fbc922a9`. Its publication check passes at
`private/20261114T131146Z-95832`; the actual receipt path is retained in
`build/cloud-resume-diagnosis/U113-r3-published-spec-private-20261114.log`.
This closes the behavior decision, not implementation acceptance.

Round four started at `270d9af3`. Component author `121408Z-47295`, fixture
fix `125800Z-80598`, independent review `130029Z-83353`, final partial fix
`131238Z-96242`, and read-only disposition `132203Z-4465` are cleanroom runs.
The initial fixture run stopped before the target condition. After correction,
`core-test/20261114T125948Z-82500` had three intended failures and a passing
foreign-write control. The first component passed ten focused tests at
`130043Z-83727` and 22 observer/WAL tests at `130437Z-86812`.
These passes do not cover the later partial fix.

Final disposition: HOLD. The last export does not compile:
`core-test/20261114T132216Z-4930`, missing EngineError qualification.
The final reviewer confirms the targeted writer-stamp and pause changes but
finds a new P1: a stamp mismatch skips other observers of committed changes.
The shared memory bound and first-freeze I/O ownership remain unresolved.
Typed refusal and the existing collection-choice integration are not built.
The idempotence assertion covers a repeated settled drain only.

The export stays in build; master has none of these source edits. Both approved
rounds are used. Antoine must decide whether to permit another source pass.
U113 remains unaccepted, with its P24 checks, gate and release evidence open.
