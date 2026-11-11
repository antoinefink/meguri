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
