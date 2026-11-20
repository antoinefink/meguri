# Hosted Core shards, advisory timing job and suite reporting

Starting commit `289dec032d75a8c530affaac6b51fe80370b42d6` (session
`s-20261120T114213Z-1c2ee6be`, role infra). Infrastructure change; no test,
bound, assertion, retry setting or production code changed. Pull request #11.

## Why

Master run `44711659224` (commit `1db262c71`, docs and specs only) spent 46
minutes in one "Linux · core" job: 8 minutes building the dev image, 15
compiling and 23 running 5,619 tests. Two `meguri-ffi` media-port tests failed
on timing and made it red. Pull request #5 (`44703674187`, docs and specs only)
failed the same way on two other media-port tests.

## What changed

- `tools/mg core-test --quick` takes hosted options: `--partition hash:M/N`
  for the ordinary tests (canaries still run first in every shard), and one of
  `--exclude-list` or `--only-list` with a file of exact `<binary id> <test
  name>` lines. A malformed line is an error, so no line can widen or narrow
  the selection. `--only-list` builds, flushes the disk with `sync`, runs the
  tests one at a time and fails when a listed test did not run exactly once.
  The receipt records the selection (`details.hosted_selection`) and, for
  `--only-list`, each listed result (`details.listed_results`).
- `.github/core-timing-sensitive.txt` lists five tests with a reason each.
- The workflow runs four shards, the advisory timing job and the aggregate
  "Linux · core", which needs every shard to have passed with its expected
  arguments on this commit. It runs on every change; suites that the change
  does not affect pass at once with "not selected".
- `docs/process/tiers.md` ("Hosted checks") and the register row
  "Hosted Core timing-sensitive tests".

## Verification

`selftest/20261120T115922Z-81382`: 17 of 17 `core-test` bats cases passed,
including the two new ones. `static-checks --base origin/master` passed.
Hosted evidence is on pull request #11.
