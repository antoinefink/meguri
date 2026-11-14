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

## Round 5: available-evidence fallback

Owner approval: `cloud-recovery-fallback-approval-20261114.md`; source start
`cbc09dc5562fea2303c9954527ef4dbd4490364f`. The narrowed K13.4/S2.5 contract
is committed at `b636a4cb`; the approval decision is resolved, not the unit.

Author: `cleanroom/20261114T144251Z-58500`. The patch is partial. Its retained
`codex.out` lists the completed paths and required remaining source work.

Verification of the implemented portion:

- Before integration, `core-test/20261114T150147Z-66249`: 16 existing cases pass.
- `core-test/20261114T150252Z-69766`: 17 focused cases pass.
- `core-test/20261114T150413Z-70999`: all 104 Host workflow cases pass.
- `swift-test/20261114T150507Z-72210`: five connection model tests pass after
  native binding regeneration.
- `similarity/20261114T150505Z-71921`: 18 files, no flagged matches.
- `private/20261114T150241Z-69382` flagged two new comments. Both complete
  comments were rewritten without changing code or guard behavior.
  `private/20261114T150403Z-70717` passes for the resulting source.

Independent review `cleanroom/20261114T150127Z-65745` is still running.
This is WIP, with no Unit/P24/gate or release acceptance. Round 6 remains for
required gaps and review findings. The live Mac and phone still show unknown
sync status; a phone request reports incomplete. No new peer-delivery claim.
