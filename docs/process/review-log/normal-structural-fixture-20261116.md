# Structural fixture repair

Start: `9202677285a765aa2d8b90b7fe5744d99dd7bc74`.
The pending integration gate retains base
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.
This is shared-engine maintenance for the own-provider contract, not
acceptance of a frozen legacy unit.

The original full-gate failure remains retained. The same test fails in
`core-test/20261116T222418Z-97683` on clean `92026772`: its fixture writes
a changed field layout without the required checked structure report.
The failure occurs before the delayed metadata assertions.

Filtered author `cleanroom/20261116T223134Z-99114` changes only
`core/crates/meguri-sync/tests/normal_sync.rs`. Its structural branch declares
backup admission and reports the actual before/after layout around its
writes. Existing field origins remain; the appended field has no origin.
No production code, assertions, time limits or nonstructural behavior changes.

Independent filtered review `cleanroom/20261116T223537Z-3249` found no
P0–P2 findings after tracing report validation, backup ordering and undo.
The reviewed patch exactly matches the author export.

Applied-source checks, based on `78c5a4a9` with only this source patch:

- `core-test/20261116T223758Z-5972`: both selected delayed-metadata tests
  pass, zero failures or skips. The nonstructural test retains its four
  direction combinations; both tests retain their actual undo checks.
- `private/20261116T223802Z-6375`: the source passes the unchanged guard.
- `static-checks/20261116T223803Z-6711`: three checks pass.
- `similarity/20261116T223804Z-6374`: one eligible file passes, zero flags
  or exceptions. The filtered implementer report was read.

These focused results do not clear the failed integration gate. No unit
completion, full client convergence or server acceptance is claimed.
