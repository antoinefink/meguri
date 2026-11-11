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

## Scope observation and server-change state

Physical preparation `run/20261111T143554Z-17594` passes on `1b30605d`.
Drive `143758Z-19075` completes all 18 interaction steps with verified
continuity. Its overall result fails 257 hierarchy checks. Scope flags are
`inC=1 outC=1`; fetch starts advance from four to six, while collection-zone
starts and record counts stay zero. The phone has 43 actual reviews. The
preview is cancelled, and the phone returns to Decks. This drive retains the
prior completion timestamp; it does not prove a new exchange completed.

Starting commit: `1b30605d`. Filtered author `144457Z-28792` adds four Debug
flags: original database callback observed, collection zone among its changes,
any unfetched server zone, and collection zone among those unfetched zones.
Only Boolean facts survive full zone comparisons in memory. Read-only public
SDK state is sampled at existing admitted callback boundaries. No network call,
reset, retry, task or production behavior changes. Flags aggregate across
instances and times and cannot prove causal pairing, current ownership,
completion or convergence.

Independent filtered review `144753Z-30680` finds no P0–P2. Focused tests
`swift-test/20261111T144815Z-32207` pass 47 cases with required lint. Privacy
`144832Z-33252` and similarity `144833Z-33533` pass. Existing formatter bounds
and assertions remain enabled. Native observation of these new flags is pending.
The Mac has 47 actual reviews, the phone 43; sync acceptance remains open.

## Physical discovery observation

Physical preparation `run/20261111T145001Z-35003` passes on `433b095b`.
Drive `145121Z-37610` completes all 18 interaction steps with verified
continuity. Its overall result fails 257 hierarchy checks. The phone has
43 actual reviews; the read-only preview is cancelled and it returns to Decks.
The recorded exchange completion advances from `1794407966674` to
`1794408767300`, with succeeded/selected-zero outcomes.

Before and after the request, `inC=1 outC=1 db=0 dbC=0 un=1 unC=0`.
Fetch starts advance from four to six; collection-zone starts, original record
modifications and Core intake counts remain zero. Across this process, native
state therefore reports some unfetched zone, but never the full registered
collection-zone identity, and no database-change callback is observed. These
aggregate flags do not establish why that zone is missing or prove that every
fetch-options callback admitted it. No production fix or convergence is claimed.

## Scope-cut observation

Starting commit: `095efaf9`. Filtered author `145640Z-44488` adds one sticky
Boolean derived from the existing original callback's requested and returned
scope facts: `cut` records whether any callback requested the registered
collection zone but did not offer it back. Native SDK code is unchanged.
This observation does not identify an exchange or current owner.

Independent review `145919Z-48366` finds one P2: an exact formatter expectation
omits the new field. Filtered fixer `145919Z-48576` corrects only that literal;
the two exports differ only by this expected-string addition. No other finding
remains. Focused tests `swift-test/20261111T150110Z-50874` pass 47 cases with
required lint. Privacy `150222Z-52326` and similarity `150220Z-52024` pass.
The original formatter limits remain: maximum output is 254, below 256, and
the 512-character assertion also passes with the latest policy value.
Native observation is pending. No sync behavior changes or acceptance is claimed.

## Real foreground comparison

On `aedf2fda`, physical drive `152456Z-89270` completes one real Home/return
cycle in the same process, followed by normal Sync Now. All 12 interaction steps
pass with verified continuity; its overall result fails 94 hierarchy checks.
It leaves the request uninterrupted and opens no connection preview. The next
drive, `152750Z-92039`, captures status before any preview: `will=10 zoneC=1
modC=8 coreC=8 db=1 dbC=1 unC=1`, with `cut=0`, no refusal and no zone error.
The newer succeeded completion is `1794410833184`. A read-only local summary
then shows 47 reviews, matching the last verified Mac total. All 12 steps pass
with verified continuity; overall hierarchy verification fails 190 findings.
The preview is cancelled and the phone returns to Decks.

No transport or merge implementation changed to produce this arrival. The next
work is launch/foreground discovery and the reverse transfer. Cold-start behavior,
fresh media transfer and full acceptance remain unverified.
