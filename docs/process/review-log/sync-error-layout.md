# Sync error layout

Start: `6ca4fcbb278ec9d0be66fef8ab07c6d3241b2b42`.
Fixer session: `s-20261024T174115Z-511e3ad6`.
Contract: `sync/errors#E1`.

Gate lint `core-lint/20261024T183834Z-89242` reported 128 instances of
`result_large_err`: the optional redirect diagnostic made `JobError` 152 bytes.
The same lint findings caused one tooling self-test to fail its expected
layering-only diagnostic assertion (`selftest/20261024T185441Z-2911`).

The first export `cleanroom/20261024T184135Z-90958` boxed the optional redirect
diagnostic. Focused lint `20261024T191337Z-54302` still failed at 128 bytes;
that attempt was not accepted.

The final source patch from `cleanroom/20261024T191613Z-56149` also stores the
immutable detail as `Box<str>`. Its constructor still accepts `Into<String>`.
Only the source hunk was applied; the lint already provides the size check.
The Rust field types change; existing workspace consumers remain compatible.
The external FFI DTO, wire data, error kinds, methods and sanitized diagnostic
values are unchanged. Independent round 2 found no P0–P2 issues.

Focused lint `core-lint/20261024T191758Z-57047` passed with unchanged limits.
`core-test/20261024T191841Z-57866` passed all 16 sync error tests.
FFI mapping `core-test/20261024T192257Z-60140` passed both selected regressions.
The affected tooling assertion passed unchanged in
`selftest/20261024T192006Z-59202`. Privacy
`20261024T192304Z-60410` and similarity `20261024T191844Z-58336` passed.

Gate `20261024T183736Z-64020` retained the discovered failures and was stopped
before VM checks so repairs can share a fresh full gate. Its passing checks
include native and Docker conformance, native durability, goldens, harvest,
host tooling, private self-test, design and milestone manifests, UI lint,
CoreBridge normal and optimized tests, MeguriKit tests and the iOS build.
The separate backup and Mac signing failures remain open. The integration base
remains `81c207573c20c337c8834e4ae8b4ee8f29603dbf`.
