# C8 runner isolation repair

Start `51e07b93`. Original integration base remains
`1416a520542a8c5daa38a23bfe398da704c3d6c9`.

Gate `20261026T095453Z-62360` was cancelled after its Rust run recorded a real
review-journal durability failure in C8. Its receipt is incomplete, not passing.
The 1,000-answer forced-crash test passed. C8 did not pass.

The retained nextest event stream shows a peak of 13 live tests while C8 ran;
17 other tests started during it. Its four-slot setting did not meet
`core/AGENTS.md`'s rule that tests with wall-clock bounds run alone.
Filtered fixer `cleanroom/20261026T101732Z-89597` changes only this setting and
its comment. All 10,000 sessions, four internal workers, production deadlines,
sync operations and assertions stay unchanged. Independent R1 is clear.
This repairs the runner rule; the cause of the individual late sync remains
unmeasured. No product durability fix or passing result is claimed.

Privacy `20261026T101851Z-91646` and similarity `20261026T101852Z-91828` pass.
`core-test/20261026T102038Z-93316` ended by SIGTERM with low free space.
It is incomplete and supplies no pass. Focused runtime and the original-base full gate
remain required. The snapshot cleanup decision is pending; no backup was changed.
Evidence and reports are retained in `build/unit-intake/U162-package-proof/`.

On clean `0bc98afa`, static checks `20261026T102714Z-96936` pass all three
checks. This confirms source rules and trailers, not runtime durability.
The full-runner workload still needs a completed focused run.

## Completed focused run after storage recovery

The snapshot was absent at resume; the agent did not remove it. Doctor
`20261026T123147Z-6094` reports Docker and VM ready with 45 GiB free.
On clean `d5a0db5a`, `core-test/20261026T123255Z-7105` passes both prediction
tests with zero skips. C8 completes its full 10,000 sessions in 312.860 seconds.
The retained nextest stream confirms peak one live test and no test starting
alongside C8. See `c8-isolated-pass-evidence.json` in the intake directory.
Workload and production limits remain unchanged. This closes focused runtime
verification; the original-base full integration gate is still required.
