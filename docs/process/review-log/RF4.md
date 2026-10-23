# RF4 — explicit self-hosted server transfers

Starting commit: `480fc88681143a143a2ee146378fe260512dc961`.

Delivery: manual Import from Anki / Export to Anki, with preview, confirmation,
safety copies, recovery, cooldown and explicit media scope. No AnkiWeb contact.
Full first-version media support remains required. Parent units stay open.

Reuse: U105/U106 code, prepared U109 contracts, existing backup and replacement.
Requirements: R118–R121, R126, R1548, R2060–R2064.

RF3 full gate: required; host setup is clear. RF4 acceptance: pending.

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

Fixer session `s-20261023T100422Z-89ddfab6`, run `20261023T100541Z-93375`:
both findings are fixed in source. The second review found no P0–P2 issues.
The live file uses mode 0600; the saved file uses 0400. Initial Import recovery
now checks ownership and current file state. Tests cover errors and process
exit before the rename, with the imported cloud decision still pending.
Format and privacy checks passed, including `20261023T101319Z-98773`.
Compile result `20261023T101316Z-98513` reports E0117 in the test transport and
an unused_parens warning. The tests have not run. Fix these checks while
keeping the existing test cases and assertions. No acceptance is claimed.
Server rig `20261023T100850Z-95458` passed eight setup checks. Free disk space
is now over 250 GB after the backup snapshot was removed.

Compile fixer `s-20261023T101509Z-4b27a32b`, run
`20261023T101648Z-729`, adds a local test transport type and removes the warning
without changing the recovery condition. Round 3 found no new P0–P1 issues.
Privacy `20261023T101846Z-2450` passed. Focused test result
`20261023T101843Z-2208`: 49 passed, two failed, five helper tests ignored.
The two failures are final upload freelist count and the initial Import error
assertion. Preserve both failures and assertions for the next repair.

Rig test `20261023T102305Z-3958` failed before login: the endpoint guard refused
the container host address produced by the current rig helper. Keep the guard
unchanged and use the existing local test routing facilities. Lint
`20261023T102308Z-4204` found three style errors; these join the same fix batch.
No runtime/rig acceptance is claimed while any check remains failed.

Check repair run `20261023T105836Z-9862`, fixer `s-20261023T103018Z-bbae3d27`:
the final compaction and replacement-error changes pass the original assertions.
Focused result `20261023T110808Z-15779`: all 51 passed; five subprocess helpers
remain ignored for direct runs. Privacy `20261023T110811Z-16023` passed.
Round 4 found no new P0–P1 issues. The endpoint guard and its rules are unchanged.

Rig compile `20261023T110920Z-17250` and lint `20261023T110924Z-17502` found
private field access in the new test helper. Similarity
`20261023T110927Z-17760` flagged the facts record at storage/one_way.rs25–44.
Rewrite that owned record from the spec and fix test access through the public
API; no scanner or guard exception. Runtime and full gate acceptance stay open.

Round 5 found no new P0–P1 issues in the grouped facts and endpoint helper.
Focused result `20261023T111820Z-23403`: 51 passed, five subprocess helpers
ignored for direct runs. Similarity `20261023T111823Z-23655` passed. Privacy
`20261023T111826Z-23906` reports one new documentation paragraph for rewriting.
Lint `20261023T112157Z-26779` passed.

Live rig result `20261023T112131Z-25947`: the real host collection transfer
case passed, as did all four routing cases and three other rig cases. Two
metadata stability tests failed while the host test changed their shared
server account. Preserve those assertions and isolate the rig tests. Earlier
attempt `20261023T112116Z-25500` could not reach Docker from the command sandbox;
the authorized host invocation reached OrbStack. No product endpoint rule was
changed. Doctor `20261023T112144Z-26269` reports no required setup step missing.
Free disk space exceeds 250 GB after Antoine removed his backup snapshot.

The final check repair reuses fixer session `s-20261023T111328Z-4f11cfe5`
with the owned seven-file patch supplied as a seed. It rewrites the new comment
and serializes shared rig tests. The source remains unaccepted until the new
checks and required full gate finish.

Final repair `20261023T112248Z-27305` applied. Round 6 found no new P0–P1
issues. The final source change is the comment and a shared-resource test
override; production behavior and test assertions are unchanged. Privacy
`20261023T112439Z-28457` passed. Live local-server result
`20261023T112440Z-28672`: all 11 selected cases passed, including the host
round trip, existing status/login cases and four routing cases. Other tests
were outside this focused selection. The earlier failed receipts are retained.
Final similarity `20261023T112453Z-29141` passed.

This is incomplete RF4 work, without a Unit trailer. Transfer orchestration,
native configuration/login/preview, collection/media execution and recovery,
focused server/native evidence and the required full gate remain open.
