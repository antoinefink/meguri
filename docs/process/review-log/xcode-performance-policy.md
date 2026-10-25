# Generated Performance policy repair

Start: `60ce96dba9244b3f75f7d362033ed43163d96763`.
Fixer: `s-20261024T174115Z-511e3ad6`.

The Xcode project check validated the source Performance plan but copied its
generated products without the required generated-policy check. Since every
test copy includes the whole Products directory, a later Debug copy could also
transport retained unchecked Profile products.

## Review and repair

- R1 found that an in-memory blocker did not survive a new invocation.
- R2 confirmed that persistent pending state and checked-cache evidence closed
  that gap. It found a false refusal for successful unchanged builds.
- R3 found no P0–P2 after the unchanged-build correction. Current policy is
  still checked. Failed or partial builds retain their blocker.

The final filtered patch is `cleanroom/20261025T024121Z-38117`.
Only the project-check runner and its regressions changed. The runner records
pending state before a Profile build and checks retained evidence before any
test build or whole-Products copy. Unmarked caches require separate preservation
and refresh. It does not alter generated policy or remove rejected products.

## Focused evidence

The regression-only patch failed against the old runner in
`selftest/20261025T023010Z-31140`: an enabled checker did not stop transport.
The final patch passed all 37 selected checks, without skips, in
`selftest/20261025T024333Z-40635`. They cover enabled or missing policy,
injections, partial builds, retained products, changed policy, and a successful
build that leaves all products unchanged.

Privacy `20261025T024340Z-40904` and similarity
`20261025T024406Z-41481` passed. The earlier privacy failure named only three
test-title or comment lines; those lines were rewritten without changing the
guard. The full integration gate remains pending against batch base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`. This repair adds no unit acceptance.

## Mac Snapshot signing

Start: `f1a6e81f`. Filtered repair: `cleanroom/20261025T025430Z-50155`.
The real credential test failed with status -34018 because this test-build path
ignored the explicit team-signing mode. The signed native workflow had passed.

The runner now honors that existing opt-in only for the Mac Debug Snapshot
host. Configuration checks, hidden launches, Mac Unit tests, simulators and the
Mac UI runner retain their existing signing. A bounded widget recopy prevents
an old embedded signature from surviving the mode change. The direct credential
assertions are unchanged. There is no fallback after a failed build.

Independent review found no P0–P2. All eight focused regressions passed without
skips in `selftest/20261025T030022Z-60289`. Privacy
`20261025T030023Z-60563` and similarity `20261025T030023Z-60504` passed.
Native result `focused-mac-snapshot/20261025T031019Z-78148` passed the canonical
Snapshot plan: 76 passes, zero failures, two existing skips. The unchanged
`deviceSlotIsNonsynchronizingAndNeverContainsPassword` case passed. The source
script was pinned to SHA-256
`75961551c83fbe0fd437071af9edc0e9533f968448a1aaa3ea5551a79acb9487`.
The full gate remains pending.

## Mac Debug signing changes, 2026-10-25

Gate `20261025T031426Z-94062`, at `d6075d6c` from base
`81c207573c20c337c8834e4ae8b4ee8f29603dbf`, failed Mac Unit validation.
The earlier Team Snapshot left its signed widget inside the ad-hoc app.
The filtered fix `cleanroom/20261025T035851Z-98124` removes that exact embedded
copy before every Mac Debug test build. The build recreates it. Existing path
checks, Profile checks and signing argument scopes remain enforced.
Independent source review found no P0–P2. All ten focused regressions passed in
`selftest/20261025T045859Z-57591`; privacy `20261025T050034Z-60566` passed.
Similarity `20261025T045908Z-57978` passed on the repair diff.

Native cycle `focused-mac-signing-cycle/20261025T045929Z-58991` exercised Team
Snapshot, ad-hoc Unit and Team Snapshot again without clearing DerivedData.
The actual results were 1/0/0, 18/0/0 and 1/0/0 (passed/failed/skipped).
Its temporary coverage guard incorrectly expected 19 Unit tests, so the job
receipt remains failed. Independent review counted all 18 selected EventTests
in source and the native log. The separate top-level clock-value test explains
the broader count of 19. The temporary helper expectation was corrected; the
native cycle was not repeated for this counting error. This focused evidence
does not replace the required full gate from the original base.
