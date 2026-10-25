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
