# Meguri Sync round progress

This unit starts at `2b80682e85743b68f0be98230d9005519873137f`.
The owner approved separate collection, local apply, and media status, with a
record of each round in Sync Log and Copy Debug Info.

## Review

The filtered implementation exported from `cleanroom/20261127T075103Z-62439`.
The independent source review in `cleanroom/20261127T080633Z-94023` found five
issues. The two fixer runs `20261127T080545Z-92017` and `20261127T081503Z-12413`
addressed them. Progress delivery rejects an older callback before buffering.
The count names downloads only. A lifecycle interruption records cancellation.
The new test routes appear in the exact inventories, and Swift tests use the
regenerated progress record fields. A schema change after server acknowledgment
does not record a completed local apply.

A subsequent review found that core can retain an acceptance-unknown recovery
state after the app observes a valid Finish reply. The log now uses the observed
reply and local apply evidence. It preserves a native media refusal when the
media result also has an error. The three provider tests with new wall-clock
bounds run alone in Nextest. See `cleanroom/20261127T082710Z-32904`.

Session `s-20261127T080615Z-10a4c97b` reviewed the complete provider spec.
The independent standards reviewer, `sync_standards`, reviewed the complete
revised status strings spec. Both exact SHA-256 digests are PUBLIC.

| File | Reviewed SHA-256 |
| --- | --- |
| `specs/sync/meguri-provider.md` | `37cb64867dc11f051331a1befea521c0cf121e036969faeb93330e57297101c5` |
| `specs/sync/status-strings.md` | `a4c126c83ae9095f20d5a2453c9a2c2488c1e3f8286107348dedd7b7e956c282` |

The same reviewer classified the implementation, first fixer, and review
session records as public operational metadata.

| Session record | Reviewed SHA-256 |
| --- | --- |
| `s-20261127T074919Z-68fbe91d` | `a312902b106253cbd9eed64038d65b794ac6292df303c4d31fed6410631cf70c` |
| `s-20261127T080516Z-0e7ac008` | `15dfedbc6230f12a88ea092597b217b5ae5c445518b2d0e1a6adcd1735b52445` |
| `s-20261127T080615Z-10a4c97b` | `7da0542057b93220f3b547cf89dbed7bc203763cd883a9380a358c23a813f16a` |

The independent standards reviewer confirmed both later fixer session records
contain public operational metadata.

| Session record | Reviewed SHA-256 |
| --- | --- |
| `s-20261127T081450Z-648c50a9` | `7a1c5420fe24159739625dbd424f03f8b5fd2438194232664dbe199ecb7aaeb8` |
| `s-20261127T082339Z-96caebdd` | `c64b51b8a68175c5e239973433d0fd5f42713485e4e964e47255c4f8b86873d9` |

## Verification

- `core-test/20261127T081758Z-17866` passed 31 selected Host and FFI tests.
  The suite covers a valid finish reply, interrupted and invalid replies,
  a no-change round, a schema change at local apply, and media progress.
- `static-checks/20261127T081757Z-17557` passed all six checks on the candidate.
- The first Rust run `081106Z-5391` had one media-pause failure in the group.
  The case passed alone in `081340Z-9641` and passed in the revised group run.

The pull request records the Swift tests and native renders. The integration
register records inherited full-gate failures.

## Integrating the iCloud retirement

The integration merges PR #246 at base `042bedc1a`. The filtered fixer run
`cleanroom/20261127T100110Z-85759` reconciled the eight conflicts. Routine
progress remains available through Meguri Sync. The native iCloud removals
stay in place. The Swift diagnostic case uses the existing `"cloud"` wire tag
so installed timeline entries still decode.

The independent standards reviewer, `sync_standards`, approved the complete
merged specs as public. These are the final reviewed SHA-256 digests.

| File | Reviewed SHA-256 |
| --- | --- |
| `specs/sync/meguri-provider.md` | `2d5502cf18f4d115f4621ba3e67142d9995b26bafa15a0fa24feafd2760bf9c9` |
| `specs/sync/status-strings.md` | `661d66ea156479fb0dd3feda4f8cce1e36bdb4798af8ecbbd03d65b897dd928f` |
