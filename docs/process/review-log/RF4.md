# RF4 — explicit self-hosted server transfers

Starting commit: `480fc88681143a143a2ee146378fe260512dc961`.

Delivery: manual Import from Anki / Export to Anki, with preview, confirmation,
safety copies, recovery, cooldown and explicit media scope. No AnkiWeb contact.
Full first-version media support remains required. Parent units stay open.

Reuse: U105/U106 code, prepared U109 contracts, existing backup and replacement.
Requirements: R118–R121, R126, R1548, R2060–R2064.

RF3 full gate: pending host setup. RF4 acceptance: pending.

## Contracts and engine prerequisite

Canonical manual/one-way contracts: `87672292`; prepared media tracking/sync
contracts: `72bb8d6a`. Spec-writer session `s-20261023T081747Z-7505bb3d`.
Prepared U109/U110/U111 authorship is retained in those commit messages.
Privacy passes: `20261023T083517Z-18494`, `20261023T084059Z-22559`.
Contract static/provenance check: `20261023T083618Z-20050` passed.
Catch-up: `20261023T083554Z-19580` passed, no queued entries. Disk: 68 GB free.

Engine work starts at `87672292`; recorded implementer session
`s-20261023T083533Z-1769cf18`, filtered source `6eac53cc`, run
`20261023T083545Z-19227`. The patch separates package stamps from publication
decisions: FullDownload requires a fresh pending decision; FullUpload preserves
the prior decision. Verified manual transfers refuse active review sessions.

Focused replacement/recovery tests `20261023T084454Z-24357`: 24 passed, no skips.
Privacy `20261023T084457Z-24600`: passed. Similarity
`20261023T084509Z-24867`: 5 files, no flags or allowlisted matches.
Formatting `20261023T084434Z-23939`: passed. Lint
`20261023T084528Z-25281`: failed on a complex test evidence tuple; repair open.
Independent round 1 review follows below. No UI changed in this prerequisite.

Round 1 found the test tuple lint and a misleading admission comment. The public
check is valid after drain, but exclusive entry does not call it automatically;
transfer callers must repeat it before preparation or remote mutation.
Fixer session `s-20261023T084708Z-2448b237`, isolated run
`20261023T084720Z-26476`, corrects the comment and names the same test evidence.
Round 2 review found no P0–P2 issues. No assertion or executable guard was reduced.
Privacy `20261023T084854Z-27836` passed. Final full engine-package verification
and lint are pending; the earlier failed lint receipt remains retained.

Final engine checks passed:

- `core-test 20261023T084851Z-27593`: 720 passed, no failures; the existing ignored
  `undo_holdback_props::ten_thousand_random_histories` remains ignored for its
  separate U25 run. The enabled 10,000-session prediction stress test passed.
- `core-lint 20261023T085540Z-31118`: formatting, clippy, layering and binary scan
  passed; the earlier type-complexity finding is closed.
- `similarity 20261023T085543Z-31361`: five files scanned, no flags/allowlist.
- `static-checks 20261023T085617Z-31900`: passed.
- `catch-up 20261023T085646Z-32650`: passed, nothing queued. Disk: 63 GB free.

The engine prerequisite has no unresolved P0–P2 review findings. RF4 acceptance
and the required full integration gate remain open, including the host setup
block already recorded for RF3. No milestone or parent unit is accepted here.

Next isolated patch: typed transfer facts and staged-upload storage preparation.
Starting commit `8f227ed0dfead33aab8ebedb05a63a9fddbf9a04`; implementer session
`s-20261023T085242Z-6179d829`. It does not mutate the checkout while running.

Readiness `20261023T085837Z-34619` reports no required setup missing. Host first
launch is no longer blocked. VM is healthy; its older Xcode remains a recorded
warning for native verification. Resume the full integration gate from the
original RF3 base `81c207573c20c337c8834e4ae8b4ee8f29603dbf`, covering this committed
engine prerequisite too. Do not apply the isolated storage patch during it.

Gate `20261023T090045Z-36621` was cancelled after a missing host dependency
prevented the native durability build; all 2752 Docker Rust tests passed first.
Locked host dependencies are now available, and native check
`20261023T092220Z-65494` passed all 25 tests without skips or changed limits.
No passing full gate is claimed. Disk headroom is 42 GB after inactive Meguri
cache cleanup; a backup snapshot appears to retain deleted blocks, so Antoine
has been asked to handle that snapshot or free space before larger Apple work.

Storage prerequisite: implementer `20261023T085302Z-29621`, then fixer
`20261023T090756Z-56474` using `s-20261023T084708Z-2448b237`. Review found signed
SQLite count decoding and dedicated Sync-reader requirements; both are fixed.
The spec writer confirmed KTD3/C6 already require Sync: no contract was relaxed.
Round 2 source review has no P0–P2 findings; focused compile/tests remain pending.
The public package snapshot APIs remain Maintenance-only. Preserve the five tests.

The next collection-transfer backend run is `20261023T091316Z-60091`, reusing
implementer session `s-20261023T085242Z-6179d829` and the reviewed fixed seed.
Source base is `ff62b9b4d60f863117fb21a23258592c4a26c645`. It works in isolation
on manual collection Import/Export and policy. Exact accepted-byte activation
must bypass a second transforming preparation while retaining verification/R6.
No new code from these isolated runs has been applied to master yet.

Antoine removed the backup snapshot; free space returned to 88 GiB. The
workstation step is clear. Backend export `20261023T091316Z-60091` contains 29
core paths, including the preserved storage seed. Formatting
`20261023T095926Z-85746` passed. Initial compile/lint
`20261023T095942Z-86364` failed because the package crate lacked its SHA-1
dependency. Privacy scan `20261023T095946Z-86609` reported six new-source regions;
they need whole-region rewrites without inspecting private tokens.

Backend round 1 independent review found P1 read-only Export activation files
and P2 absent-collection Import recovery that can retain a permanent operation
guard. Both require fixes and regressions. No other findings were confirmed.
The formatted code is preserved for the fixer in the owned stash
`7254a3ae99cf5185981746dd54c9a86ed470d2b0` and an exported own-code seed patch.
No commit hook, check or acceptance boundary was bypassed. Focused tests and
the full gate remain pending until these findings are resolved.

This is incomplete RF4 work, without a Unit trailer. Transfer orchestration,
native configuration/login/preview, collection/media execution and recovery,
focused server/native evidence and the required full gate remain open.
