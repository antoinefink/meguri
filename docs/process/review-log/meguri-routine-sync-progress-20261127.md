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

The independent reviewer, session `s-20261127T080615Z-10a4c97b`, read the full
contents of both specs and classified these exact SHA-256 digests as PUBLIC.

| File | Reviewed SHA-256 |
| --- | --- |
| `specs/sync/meguri-provider.md` | `37cb64867dc11f051331a1befea521c0cf121e036969faeb93330e57297101c5` |
| `specs/sync/status-strings.md` | `d6f43622fbef904b5f7afd067485cf2d3852f503505f7a6b4db35b05d5f1e2eb` |

The same reviewer classified the implementation, first fixer, and review
session records as public operational metadata.

| Session record | Reviewed SHA-256 |
| --- | --- |
| `s-20261127T074919Z-68fbe91d` | `a312902b106253cbd9eed64038d65b794ac6292df303c4d31fed6410631cf70c` |
| `s-20261127T080516Z-0e7ac008` | `15dfedbc6230f12a88ea092597b217b5ae5c445518b2d0e1a6adcd1735b52445` |
| `s-20261127T080615Z-10a4c97b` | `7da0542057b93220f3b547cf89dbed7bc203763cd883a9380a358c23a813f16a` |

The independent standards reviewer confirmed the last fixer session record
`s-20261127T081450Z-648c50a9` is public operational metadata. The reviewed
SHA-256 is `7a1c5420fe24159739625dbd424f03f8b5fd2438194232664dbe199ecb7aaeb8`.

## Verification

- `core-test/20261127T081758Z-17866` passed 31 selected Host and FFI tests.
  The suite covers a valid finish reply, interrupted and invalid replies,
  a no-change round, a schema change at local apply, and media progress.
- `static-checks/20261127T081757Z-17557` passed all six checks on the candidate.
- The first Rust run `081106Z-5391` had one media-pause failure in the group.
  The case passed alone in `081340Z-9641` and passed in the revised group run.

The pull request records the Swift tests and native renders. The integration
register records inherited full-gate failures.
