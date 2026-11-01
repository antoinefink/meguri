# App CloudKit configuration

U113/U114 continuation. Component start:
`cb3434d0be1806a2c24a60a648dcc77d06b69ad7`.
Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
The separate review/media test landed as `587d2c6a` during this component.
Neither unit is accepted; no Unit trailer is added.

## Change and scope

The app now packages explicit container and environment values, typed schema
readiness, and signed-platform CloudKit/push entitlements. Development and test
builds, including the Mac personal preview, use the isolated development
container. Normal Release/AppStore builds use production. Widgets and ad-hoc
Mac products retain no protected cloud grants. Readiness defaults remain NO.

The common verifier checks resolved settings against actual Info and signed or
embedded entitlements. Standalone preview receipts record cloud configuration
verification only after that check passes. The existing Xcode checker also
checks ordinary products and offers an explicit signed configuration matrix.
Existing signature, sandbox, identity, fixture-scope and core checks remain.

Container registration, profile authorization, encrypted schema review/promotion,
signed device execution and live cross-device delivery are still unverified.
The setup document describes those user steps. No account or portal action was
performed. An ad-hoc build with readiness false does not activate native sync.

## Independent review and repairs

Implementation `cleanroom/20261101T025249Z-6592` used implementer session
`s-20261025T134357Z-235af9a2`. The initial export omitted the typed source Info
entry. Integration also identified production routing in Release.dev previews.

Fixers used session `s-20261025T162405Z-9dc656e4`:

- `cleanroom/20261101T025945Z-11231`: typed Boolean inputs and isolated preview
  selection, product/profile checks and negative tests. Round 2 found no new
  required source fixes; actual artifact evidence was still needed.
- `cleanroom/20261101T030549Z-16757`: standalone actual-artifact verification,
  negative fixtures and complete rewrites of four privacy-flagged prose regions.
  No privacy exceptions changed. Round 3 found no P0/P1 or substantive P2.
- `cleanroom/20261101T030937Z-19877`: fixed the two missing built Info keys in
  both platform inputs. The test now checks those entries before and after CPP.
  Round 4 found no P0/P1. Specs and UI are unchanged.

## Verification

- `selftest/20261101T030336Z-13544`: 12 focused checks pass.
- `selftest/20261101T030825Z-17563`: 18 checks pass, including negative artifact
  and signing fixtures.
- `build-app/20261101T030339Z-13816`: initial Mac Debug build passes before the
  stronger artifact guard; this alone did not prove the destination keys.
- `build-app/20261101T030833Z-17873`: preserved failure, missing built Info keys.
- `selftest/20261101T031018Z-20399`: 18/18 pass after the Info repair.
- `selftest/20261101T031128Z-22654`: 27/27 existing Xcode build-control checks pass.
- `build-app/20261101T031021Z-20656`: Mac Debug standalone passes, including
  `cloud_configuration_verified: true` on the actual unlaunched app.
- `private/20261101T030458Z-15978`: four prose regions failed; final full scope
  `20261101T030855Z-19157` passes, then final three-file repair
  `20261101T031038Z-21944` passes.
- `similarity/20261101T030857Z-19420`: full component passes; final repair
  `20261101T031040Z-22200` passes.

Full integration and milestone gates remain open at their original base. This
component does not close their recorded technical failures or the remaining
runtime conflict, generation, media and app-composition work.
