# U126 filtered implementation handoff

- Role: implementer; session `s-20261025T134357Z-235af9a2`.
- Starting base: `master187926b3`, recorded by root before this work.
- Extended the existing forced writer with postpublication retention and an additive Rust result. No new writer, sidecar, scheduler, restore, server operation or cross-language surface.
- Added canonical local naming/listing, the pure protected-name anchor walk, synced limits via storage config writes and Settings semantics, and F2–F6 production adapters. Only `package/backups/*` is activated; prepared inputs, goldens, deviations and U127 triggers stay unchanged.
- Added explicit AE69 sets, retention properties, config and publication integration tests, and the MG-discovered 150k streamed backup/retention benchmark with the existing two-second contract.
- Source-only handoff: no builds, tests, jobs, network, branches, worktrees or commits in this copy. Root owns focused checks, both conformance lanes, T1, idle-machine measurements, independent review and all gates. No acceptance claim.
- Existing host lifecycle tests remain the evidence targets for 61-name exhaustion, bounded races, WAL snapshots, missing-parent/capacity refusal, strict device metadata and lease-before-replace safety. New tests focus on their U126 retention integration.
- One source consistency pass: `git diff --check` passed and changed integration surfaces were inspected. Obvious follow-up edits preserve the old benchmark helper's snapshot location and run retention after attempted durable metadata recording, including when recording fails. No second check or execution was run. Root must format/compile and refresh dependency lock metadata as required.

## Independent R1 fixes

- Role: fixer; session `s-20261025T162405Z-9dc656e4`; U126 starting commit `187926b3`. Resumed the supplied prepared patch; preserved the current master U124a host-test repairs and all unrelated work.
- U126-R1-F1: imported `meguri_model::clock::Clock` for the scenario clock's `now_ms` call.
- U126-R1-F2: pin the backup directory and parent identity before publication, retain pin failures for postpublication reporting, and revalidate the original namespace before enumeration and each deletion. Reuse package-owned descriptor enumeration/open/no-follow stat helpers; deletion uses `unlinkat` on the pinned root. Never reopen a replacement root for retention. Directory errors remain nonfatal; metadata errors stay primary and late cancellation keeps the existing behavior.
- Added a deterministic sidecar publication regression covering symlink and real-directory replacement of both the root and an ancestor. Exact byte maps protect all target files and the original directory's old/new backup bytes; the result must retain a root-level failure and its durable metadata record. No sleeps or retries.
- U126-R1-F3: kept complete exact CLI support arrays and added all active collection package operations and `package_listing`, with explicit language/support positive and negative assertions alongside U126 coverage. Production capability registration is unchanged.
- Fixtures, goldens, deviations, typed metadata/fuzz, U127 trigger inactivity and server behavior are unchanged. No builds, tests, jobs, network, history or commits in this fixer copy; no runtime verification or unit acceptance claimed. Root owns formatting/compilation, focused cases, budget and integration gates. One final source consistency check is reported in the exported final handoff.
