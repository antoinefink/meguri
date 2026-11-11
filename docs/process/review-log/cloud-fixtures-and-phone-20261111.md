# Sync fixtures and physical phone checks — 2026-11-11

Role: fixer. Starting commit: `69b139b3`.
Integration batch base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/cloudkit-recovery#V14`, `conformance/cloudkit-acceptance#Q6`.
Status: WIP; required checks remain red.

## Fixture changes

Adoption's generated transport now uses 32-record pages within its helper and
restores the caller's complete configuration before returning. Ordinary fetch
fixtures keep their existing page size and assertions. Each adoption page also
checks its count before Core validates it.

The FFI cancellation case requires the exact `ReadActive` action after a
temporary active-read failure, including the original encrypted bytes and
complete system fields. Its successful branch still requires `Read` for epoch.

Q6 carries the original note's actual name into the child and reads that exact
row. The original ID, fields and dirty-marker predicate remains intact.

Filtered authors: `103218Z-31418`, `103723Z-38686`, `104153Z-51554`.
Independent review `cleanroom/20261111T104339Z-53929` found no P0–P2 issues in
the three changed test files. Formatting changed one call's wrapping only.

## Verification and unresolved Q6 result

- `core-test/20261111T103530Z-36777`: 23/37 pass. Setting the service's global
  page size to 32 broke ordinary-fetch assumptions. That approach was replaced
  by the helper's temporary configuration and exact restoration.
- `core-test/20261111T103930Z-43580`: 36/37 pass. Q6 reaches its Resume check
  but fails the original note-content/dirty predicate before its kill point.
- `core-test/20261111T104400Z-57989`: 36/37 pass with the exact note lookup.
  This disproves the first-page explanation of that failure: a row exists, but
  the original predicate still fails. Individual content and marker facts need
  diagnosis. The assertion and actual SIGKILL requirement remain enabled.
- Final lint `20261111T110709Z-19782` and similarity `20261111T110707Z-19496`
  pass after formatter `20261111T110642Z-19074`.

The earlier lint formatting failure and every failed test receipt are retained.
No broad gate was repeated. The other retained Rust failure, dependency
advisories and native accessibility failure remain open.

## Phone and Mac observations

The latest normal signed phone app installs and launches through
`run/20261111T104534Z-63744`. After the owner unlocked the phone, status and
settings drives `104708Z-68434` and `104941Z-71658` complete all three interaction
steps each with verified process continuity. Their identifier audits fail.

The Mac commits one Good answer and ends the session: counts change from
20 new / 0 learning / 19 due to 19 / 1 / 19. Its three pending changes clear,
and the UI reports a sync completion today at 11:54.

Phone receive drive `105416Z-82115` completes four interaction steps with
verified continuity, but its counts stay 20 / 0 / 19. Its overall identifier
audit fails. Thus this attempt does not prove delivery of the Mac review.

Read-only count inspection `105707Z-91224` hits the tab bar while the preview
button is partly covered. No replacement is confirmed. The adjusted inspection
`110042Z-98751` captures an empty Account & Sync destination and fails to find
its content. The duration and cause of that blank state are unproven.

Final drive `110344Z-14095` returns to Decks: three steps pass, zero hierarchy
issues, continuity verified. Counts still show 20 / 0 / 19. The phone is left
on Decks; no phone answer or collection replacement was performed. Account
equality across the two devices remains an owner question, with no account
values requested. No two-device round trip or sync acceptance is claimed.

## Prepared work

Swift coverage from `cleanroom/20261111T104242Z-52535` remains under `build/`,
unapplied and unexecuted. Its retained confirmation and cleanup order correct
two source-review findings in the first draft. It covers staging with media
active and a later preview, but does not force the isolation race. Independent
review and native compilation remain required before using that coverage.

No Unit trailer or milestone pass is claimed.
