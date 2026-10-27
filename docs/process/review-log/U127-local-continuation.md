# U127 local continuation handoff

- Role/session: implementer, `s-20261025T134357Z-235af9a2`.
- Supplied unit start: `134a5747`. Original integration batch base `1416a520`
  remains pending. Continued the committed first slice; did not recreate its
  structural admission/coalescing/retention protections or weak provider owner.
- No forbidden sources/history/network, jobs, tests, builds or commits. U49's
  engine/FFI deck_options and Swift DeckOptionsBridge files were not edited.
- This is source for root's compilation, focused verification, independent
  review/fix, provenance/privacy/similarity checks and gates. No U127 completion
  or acceptance claim. The single source consistency receipt is in the handoff.

| Requirement | Production source | Focused source tests (unrun) |
| --- | --- | --- |
| T2/T3 exact interval and durable snapshot reference; cached single deadline, commits/undo, lifecycle/exclusive boundaries, no autonomous wakeups | engine `maintenance.rs`, lifecycle, executor writer/external-change notifications, existing job pool | `maintenance_scheduler/periodic.rs`, included by `maintenance_scheduler.rs` |
| T3 trigger/failure/floor/suspend policy; bounded cooperative Quit cancellation | engine maintenance participant and cancel token shared ownership/deadline | same policy tests; root must measure the R1848 150k budget |
| T5 real forced writer, typed receipt/error, Ready/exclusive/Failed admission | engine maintenance API/BackupService, package `backup.rs`, admission-checked maintenance reader | package `backup_service.rs` |
| T7 stable window, main/nonempty WAL only, SHA-1/size readback, WAL-first exclusive publication, inode-owned rollback, no reference | existing package raw-copy path factored with strict local mode; engine declared damaged-copy maintenance task | package `restore.rs` raw-copy tests |
| T8 recognized name before reads, slot, combined capacity, inspection before safety, both review checks, protected safety, initial/Failed recovery, verified R6/R7 swap/cancel fence | package `restore.rs`, host restore entry, slot review admission, existing replacement cancellation seam | package `restore.rs`; appended engine slot admission race test; existing engine R6 recovery matrix remains required |
| H1/H2 local scenario adapter and exact capabilities | testkit package/storage adapters and local recovery support; CLI expectation | `backup_triggers.rs`: all six prepared cases, unchanged goldens/deviations |
| Shared event compilation | engine MaintenanceDeadline, mechanical Rust FFI/Swift CoreEvent mapping | existing event suites plus publish-refusal case; root owns compilation |

## Review notes and remaining evidence

- All new tests are source only. Native compilation, formatting/lints, focused
  runs, independent review and gates remain with root. Keep earlier receipts;
  do not reinterpret them as evidence for this continuation.
- The legacy low-level verified Restore deliberately retains active-session
  behavior. The local T8 consumer uses its explicit slot fence before entry and
  after drain; the existing legacy regression expectations remain intact.
- Manual-transfer raw-copy publication retains its pre-copy witness and its
  uncertain-publication behavior. Strict local T7 mode alone owns cancellable
  streaming hashes and rollback of its published pair. No server tests added.
- New deterministic coverage includes reference retention/reopen/relaunch,
  armed-commit caching and event changes, floor boundaries, background/quit,
  suspend cancellation, real unchanged forced backups, five real answers and
  actual backup/restore/reopen, package refusals/capacity/session/busy/cancel,
  raw WAL/SHM/hash/rollback, NoCollection/Failed recovery and post-rename fence.
- Still required in root's verification: actual R1848 150k timing, compilation
  and platform creation-time coverage, full existing R6 crash/recovery matrix,
  and independent review of worker/suspend/replacement races. The new tests do
  not claim an exhaustive fault/cancellation matrix or process-kill execution.
- Future real-operation tests for U34/U35/U121 remain explicitly pending; no
  stand-in operation was invented. U133a services/commands, Swift UI/timer and
  U113/U114/CloudKit execution remain outside this local source handoff.
