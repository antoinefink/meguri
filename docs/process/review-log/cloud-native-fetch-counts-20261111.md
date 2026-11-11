# Original fetch counts — 2026-11-11

Role: fixer. Starting commit: `f5ff71b9`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/policy#P12`, `sync/cloudkit-recovery#V3`,
`sync/cloudkit-records#K4`. Diagnostic work; no unit or sync acceptance.

The controlled device test remains red: three Mac changes were selected in a
successful exchange, but the phone's next exchange left its actual review count
at 43. Policy `incomingWaiting` counts blocked inbox entries; it excludes ready
entries. Zero waiting alone does not prove that the inbox is empty.

This Debug-only patch counts original native fetch starts, registered collection
zone starts, collection/root modifications before conversion, native fetched
refusals and failed zone results. It also counts collection records after the
original Core intake returns successfully. Values are synchronized, bounded
process totals across all instances. They retain only scalar counts, saturate
with an explicit overflow flag, and prove no current-owner or convergence claim.
No SDK call, task, observer, retry or persistence was added. Release and visible
UI behavior stay unchanged. The existing Debug accessibility value places these
counts before the policy snapshot and failure text.

Author `cleanroom/20261111T140533Z-53671` exports seven files. Independent review
`141043Z-62934` finds no P0–P2. Focused native tests
`swift-test/20261111T141115Z-64804` pass 45 cases in three suites, including
classification, overflow, concurrency and prefix checks; required lint passes.
Privacy `private/20261111T141120Z-65712` and similarity
`similarity/20261111T141124Z-66101` pass. Original assertions remain enabled.

The next native run must compare callback counts with successful Core intake.
These totals can include callbacks awaiting their serial turn. A zero Core
count read before callback completion cannot establish a dropped delivery.
Native runs, broader integration checks and the cross-device result remain open.

## Native delivery observations

Physical preparation `run/20261111T141339Z-69897` passes on clean `766c53d4`.
Both built apps' signed iCloud grants match, including the Development
environment and signing team. No grant or account values are included here.

Drives `141538Z-76449` and `141824Z-81303` complete all 7 and 12 interaction
steps with verified continuity. Their overall results fail hierarchy checks
(35 and 190 findings). The process counts advance from `will=3` to `will=5`;
`zoneC`, `modC`, `modR`, `coreC`, `refused` and `zoneFail` stay zero. A new
reported completion at `1794406625759` still leaves 43 actual reviews.

A fresh Mac Good is then recorded after this instrumented phone process has
started. Automatic sync is held off for that edit, three pending changes are
observed after ending the session, and a manual exchange reports success with
three selected changes at `1794406981025`. Pending returns to zero. Automatic
sync is restored on. A read-only preview confirms 47 actual Mac reviews and
is cancelled without replacement.

Drives `142421Z-95776` and `142615Z-98122` request and inspect the phone's next
exchange. All 7 and 12 interaction steps complete with verified continuity;
overall hierarchy checks fail with 35 and 194 findings. `will` advances to
seven while every other fetch counter stays zero. The new completion is
`1794407147240`; the phone still has 43 actual reviews. Preview is cancelled,
and the phone returns to Decks. No record intake or collection-zone fetch is
observed, including for this post-install edit. This does not yet identify
why the SDK does not fetch that zone.

## Fetch-options follow-up

Starting commit: `766c53d4`. Author `cleanroom/20261111T142835Z-1027` adds only
two sticky scalar observations: whether an original SDK options callback has
requested the registered collection zone and whether a returned scope has
offered it. They may come from different callbacks; no pairing, current-owner
or convergence inference is allowed. Entry admission is captured before any
suspension. Missing zone information records nothing. All production return
paths, errors and filtering remain unchanged.

Independent review `143142Z-8200` finds no P0–P2. Focused tests
`swift-test/20261111T143213Z-10566` pass 46 cases with required lint. Similarity
passes. Privacy flags one new comment line. Filtered fixer
`143254Z-12639` changes only that two-line comment; the exported diff confirms
all code and tests are unchanged. Final focused tests
`swift-test/20261111T143447Z-14191` pass 46 cases with required lint, and the
final source/evidence privacy scan passes. Native observation of these two
flags remains open.
