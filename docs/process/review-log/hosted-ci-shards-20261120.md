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

Hosted run `44732230097` failed every shard's receipt check: `jq --args`
read the `--quick` that followed as its own option. Its shard 3 had passed
1,369 tests. Run `44736475417` on `88383ef14` passed all four shards and
"Linux · core" in 29.6 minutes from the run start. Each shard spent 7.6 to
8.0 minutes on the image, 13.3 to 14.7 compiling and 5.8 to 7.5 running
tests, against 8.3, 15.0 and 23.0 in master's single job (`44711659224`).
Its advisory job ran all five listed tests alone, and each passed. The job
still failed, because the JSON names of library unit tests
(`meguri-ffi::meguri_ffi$...`) did not match the listed binary id
(`meguri-ffi`); `d71b6124f` compares by binary id, and
`selftest/20261120T125458Z-84906` (2 of 2) covers a library unit test. Dispatch run
`44736534476` from a throwaway docs-only branch, with `base=unit/ci-faster`,
reported every check as passing and "not selected" within 25 seconds.

## Review

The first independent review found one blocking defect, the `jq --args`
check, which failed closed. It also raised three should-fix items, all
fixed in `e95cc5013`:

- Renames hid the old path from the selection; it now uses `--no-renames`.
- Adding a list line silently drops a test from blocking Core; such a change
  now gets a warning, and "Linux · core" lists the uncovered tests.
- Branch protection must not require the shard or advisory checks; the doc
  now says so.

A nit asked to check that the shard list is 1..N; that is done too.
CodeRabbit asked for NUL-separated paths (`1ac511086`).

A second independent review of `e95cc5013`, `88383ef14` and `d71b6124f`
found nothing blocking and raised two should-fix items:

- A manual run given `base` could report "not selected" on a pull request's
  head commit and replace its red checks. The input served only the
  docs-only proof, so it is removed.
- Library names that are not the package name with `_` could not be
  listed. Listed results now take the binary id from nextest's own event
  fields when present (`selftest/20261120T130559Z-12142`: 17 of 17).

The added-line warning now also catches indented lines.
