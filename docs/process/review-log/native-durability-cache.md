# Native durability build preparation

Start: `dba0c601`.
Integration base: `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.

Gate `075407Z-60333` passed static checks, canaries, Rust lint, dependency
checks and the core suite: 2,932 cases passed, 12 skipped. Native durability
`081051Z-97423` failed during compilation because a locked test dependency
was missing from the host cache and the build required offline operation.
No native tests ran. The gate was stopped through its documented cancellation
path before the Apple suites. Its incomplete error result and failed native
result are retained; neither is a pass.

The repair allows both locked, target-pinned native compilation steps to
fetch missing dependencies before taking the measurement lease. Executable
selection, the dedicated target cache, test commands, timing limits and
single-run handling are unchanged. No retry or new command interface is added.

The existing success test now models the missing-cache condition. Focused
selftest `081443Z-1138` failed before the repair as expected. All seven
native-durability selftests passed in `081512Z-1627` after it. Real native
durability `081514Z-1878` passed all 25 cases without skips. Independent
review found no P0–P2; its complete receipt is retained in `build/`.
The full gate from the original base remains pending. No unit acceptance
is claimed.
