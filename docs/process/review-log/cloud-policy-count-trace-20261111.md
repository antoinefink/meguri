# Policy count diagnosis — 2026-11-11

Role: fixer. Starting commit: `b34fb5fc`.
Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/policy#P12`, `sync/cloudkit-recovery#V13`.
Status: diagnostic work; no sync or unit acceptance.

## Scope and review

The actual Mac and phone totals remain 44 and 43 after reported manual syncs.
A passive Debug-only trace now projects existing policy counts, binding flags,
and each exchange's outcome and end time. It keeps bounded initial, first
completed and latest observations. It adds no SDK/FFI calls, tasks, polling,
notifications, persistence, record contents or identifiers. These are policy
projections, not original SDK accounting or convergence proof.

Author `cleanroom/20261111T131955Z-66345`; review `132618Z-72382` finds one P2:
old-owner evidence can remain visible before a replacement receives a snapshot.
Fix `132950Z-83436` qualifies reads by generation and host revision, and hides
them while current status is absent. Review `133342Z-86422` finds no P0–P2.
Visible UI and Release behavior remain unchanged.

## Verification

- Initial tests: `swift-test/20261111T132630Z-75964` passes 24 cases;
  `132759Z-79346` passes 22 coordinator cases. These did not cover the review
  finding's interval before another snapshot.
- Final tests: `swift-test/20261111T133356Z-86984` passes 25 cases, including
  wrong-owner reads before any subsequent retain; `133423Z-90015` passes
  22 coordinator cases.
- Final lint `lint-ui/20261111T133412Z-88178`, privacy
  `private/133414Z-89107`, and similarity `similarity/133413Z-88948` pass.

All source is verified through the filtered implementation workflow. The
native observations follow; actual account equality and review/media
convergence remain unverified. No existing assertion or limit was weakened.

## Native observations and display correction

The owner confirmed that the Mac and phone use the same Apple Account. No
account value was requested or recorded. This is owner confirmation, not an
SDK identity proof.

The signed normal Mac build `build-app/20261111T133731Z-94439` passed on clean
`dc8398c2`. The executable hash matches the receipt. After reopening that exact
product, the policy projection showed zero pending, held, in-flight and waiting
incoming changes, an active usable binding, and an account matching that binding.
The first retained completion selected zero changes. A new manual request
advanced the displayed completion time to 14:41.

A fresh authorized Good grade advanced to the next card with Undo available.
After ending the session, deck counts were 19 new, zero learning and 19 to
review. A manual exchange advanced the displayed completion time to 14:45.
These observations do not prove receipt on the phone. The previous actual
collection totals were 44 on the Mac and 43 on the phone; the new Mac total
has not yet been read directly.

Native inspection cut off the policy value before its latest outcome. The
three-file follow-up moves the latest frame before retained history and the
policy value before the failure text. It retains every fact and owner check.
A new case uses typical counts and requires its newest snapshot to fit in
the first 512 characters. Extreme numeric values may exceed that inspection
limit; the full retained value remains bounded and is not truncated by the app.

Focused tests `swift-test/20261111T134741Z-13013` pass 26 cases. Its required
lint preflight passes. Similarity `20261111T134653Z-10781` and privacy
`20261111T134745Z-14267` pass.

Phone preparation `run/20261111T134138Z-97742` was queued because Xcode could
not see the selected device. It installed nothing and is not a pass. The owner
has since reconnected and unlocked the phone. Independent filtered review
`cleanroom/20261111T134720Z-11370` finds no P0–P2. The native check of the
reordered value is still pending. No sync acceptance is claimed.

## Controlled send and phone receive

The follow-up is committed as `4f033c54` and pushed. Signed normal Mac build
`build-app/20261111T135447Z-42599` passes on that clean source; its executable
hash matches the receipt. Physical preparation `run/20261111T134921Z-20657`
passes and launches the same source normally. The apps' recorded product
metadata has matching cloud container, environment and app identity; no values
are included here.

The reordered policy value is readable on both platforms. Phone drives
`135104Z-25598` and `135245Z-39226` complete all 7 and 12 interaction steps,
with verified process continuity. Their hierarchy checks fail with 35 and 190
findings. After the requested exchange, the phone still has 43 actual reviews.
The Mac's read-only preview confirms 45 after the first fresh grade. The
preview was cancelled; neither collection was replaced.

For the next grade, automatic sync was turned off while the binding stayed
active. Good advanced to the next card; ending the session changed the deck
counts to 19 new, zero learning and 18 to review. The policy snapshot then
showed three pending changes, zero held or in-flight changes, and zero waiting
incoming changes. A manual request subsequently reported success with three
selected changes, zero newer pending, zero held and zero incoming waiting;
its completion time is `1794405478713`. Pending returned to zero. Automatic
sync was restored to its original on state. The expected local total is 46,
but this last total has not yet been read directly. A generic failure message
remains visible despite that reported completion; its cause is unproved.

Phone drives `135843Z-45091` and `140030Z-46866` then request and inspect another
exchange. All 7 and 12 interaction steps complete with verified continuity;
the hierarchy checks still fail with 35 and 194 findings. The phone reports
a successful completion at `1794405609440`, zero selected, zero pending, zero
held, zero in-flight and zero incoming waiting. Its read-only preview still
shows 43 reviews and 565 media files. The preview is cancelled and the phone
returns to Decks.

These are Core policy projections and actual local row counts. They show the
remaining discrepancy; they do not establish the original server record set,
SDK delivery, or convergence. Both apps also report 2389 missing media files
on the device and in iCloud. The next diagnosis must distinguish original
fetch delivery from incoming staging and application. Repeating empty syncs
or replacing the phone collection would not establish that boundary.
