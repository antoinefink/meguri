# Signed normal Mac drive profile — 2026-11-16

Starting commit: `c246c6cea`. Source commit: `9b8148fc`.

The normal team-signed VM drive could build but not launch in
`drive/20261116T154119Z-35895`. A read-only native comparison confirms the
built automatic profile excludes the VM; the already-approved pin covers it.
Only those Boolean facts were retained. No account, capability or profile changed.

The existing explicit app-only input now also admits exactly
`drive/team/build-for-testing/mac/Debug/UI/Meguri`. It uses app-scoped Xcode
variables and checks its binding after building, before VM delivery. Other
jobs/configurations/plans/platforms/actions/schemes remain excluded. The validator
queries four named targets and requires the widget and both test targets to keep
their original signing style/specifier. It never re-signs products or changes defaults.

Author `cleanroom/20261116T161430Z-56392` and independent review
`20261116T161828Z-69612` are clear. Review preserves the seed. Fixer
`20261116T162045Z-73285` only renames newly flagged public test vocabulary and
rephrases one documentation sentence. The integrator compares all five final
files against the reviewed source plus the exact substitution map: equal.
The privacy guard is unchanged.

Focused Docker `selftest/20261116T162255Z-76595` passes seven Bats cases,
including the generated Python validator checks. Privacy
`private/20261116T162258Z-76924`, static `static-checks/20261116T162316Z-78569`
and similarity pass. The original similarity 71639 covers both source files;
the final renamed file is covered by `20261116T162317Z-78879`.
Both filtered reports have zero flags and exceptions.

On clean `9b8148fc`, `drive/20261116T162409Z-80271` passes all six normal
launch/home/status steps, with zero failures, skips or hierarchy issues.
The exact product embeds the approved profile and verified development cloud
configuration. Both app-window captures were inspected. The saved VM collection
is unsupported, so the app shows its file error and disables Sync Now. This is
launch/capture verification, not a working collection or sync proof. A fresh
receiver will replace only that disposable development collection.

Production source remains `fefef71ad`. The full gate remains failed, with its
original integration base retained. No Unit, milestone or release is accepted.

## Physical peer and next check

The physical Debug build completes in `drive/20261116T163422Z-88591`, but
Xcode explicitly waits for a locked device before test interaction. The owned
blocked xcodebuild receives SIGINT after its run path and exact test selector
are checked. The result remains incomplete, not a test or sync pass. No
unchanged automation attempt is repeated. The owner unlock request remains pending.

`fixtures/20261116T164013Z-94927` generates the public cloud-small collection
with 20 cards and five valid PNG files. The next VM check will use
that disposable collection or an empty receiver, through native Settings,
without a synthetic screen, clock or fabricated tracking/binding state.
Publication, adoption, saved reviews, media and cold-reopen convergence still
need actual native verification.

Independent evidence review is clear against permitted source and the supplied
receipt summaries. Only public wording changed after that review.
