# Legacy Epoch transport verification

Role: fixer. Session: `s-20261110T214025Z-20fa15fd`.
Unit follow-up start: `3973eb519646b94c926aa94e3feee22a5c8ebd21`.
Integration start: `87dae813a102e7213343c794bd5b951215961c72`.
Pending integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`.
Contracts: `sync/policy#P6`, P12.3; `sync/cloudkit-records#K12`; `sync/cloudkit-recovery#V26.1`.

Thirteen cloud regressions fail on unchanged master at `core-test/20261113T174743Z-54682`. Read-only diagnosis `cleanroom/20261113T180633Z-75749` finds that the legacy testkit drops fetched Epoch records and ignores Epoch reads. Its healthy fixtures confirm Active without establishing independent qualified CRT evidence. Production correctly refuses writes through that closed gate.

Six Rust files carry this change. Service Epoch replies retain ownership and page position. Fixtures establish CRT before the current Root proof. A record without `e` is delivered as unreadable. The tests check that CRT and Root confirmation remain independent. Existing guards, assertions and limits are preserved.

Author `cleanroom/20261113T181620Z-84875`; review `182555Z-93729` finds one P2 for missing-field omission. Repair `182936Z-95858` adds retained unreadability, scope and mixed-page cases. Review `183524Z-339` finds no remaining P0–P2. Compilation `core-test/20261113T183547Z-1597` stops at an out-of-scope JSON helper before tests; mechanical export `cleanroom/20261113T183638Z-2001` fixes that return.

Execution `core-test/20261113T183748Z-3673` passes 49 of 52. Repair `cleanroom/20261113T184019Z-5177` supplies actual Epoch evidence to both healthy setup paths, moves the recovery fixture's Epoch setup before its root check, and builds a canonical Publishing control. All original assertions remain. Final independent review `cleanroom/20261113T184226Z-6992` finds no P0/P1 and exports no code.

`core-test/20261113T184259Z-7936` passes all 52 affected integration cases, including every original failure, recovery, review roundtrip and large-payload checks. `core-test/20261113T184347Z-8795` passes staged-source lifetime and mixed-page ordering through the original terminal reply. Missing-field fetched coverage wraps an actual projected Service Read in a legacy callback; ordering also has synthetic IPC coverage. These supply no native owned FetchRead proof or actual missing-field Service FetchZone proof.

Broader `core-test/20261113T184501Z-9700` passes all 765 selected cloud Host/runtime tests; 278 unrelated tests are filtered. Correctness lint `core-lint/20261113T184903Z-12231` and static checks `static-checks/20261113T184913Z-12869` each pass three checks. Similarity `similarity/20261113T184907Z-12511` passes all six code files without flags. Privacy `private/20261113T184957Z-13995` passes all seven files. Original failed receipts remain. This does not close P24, global K10, U113/U114, a required gate, milestone or release acceptance. The earlier normal-app device and byte-transfer evidence stays separate at its original source and scope.
