# Jobs, cancellation, progress, threads and readers

Spec: engine/jobs
Requirements: R170 (core side), R171, R1494, R1858, R1860
Anki: none (Meguri design: no Anki behavior; written by the implementer, U24 execution note)
Scenarios: none; tests in `core/crates/meguri-engine/tests/`
Status: Approved 2026-10-27 (R2066) for J2.1 and the CloudKit and `CloudAdopt` bullets of J2 and J3, with `sync/policy`, `sync/cloudkit-records` and `sync/cloudkit-recovery`. Every other section is in force and unchanged.

Long work in the core runs as jobs (KTD9): a short call starts one and returns its handle at once; the work runs on core-owned threads; its end is one event. This spec also covers the cancel token and the progress sink every long operation takes, and the thread registry every core thread is created through.

## J1. Jobs

- Starting a job returns `JobHandle{id, cancel, progress}` at once. Ids are unique for the life of the engine, from 1 up.
- Every started job ends with exactly one `JobFinished{id, outcome}` on the change stream (`engine/change-stream` S3): `Succeeded`, `Cancelled` (the work returned `Cancelled`), or `Failed{error}`. A panic in the work is caught and ends the job as `Failed{Internal}` with the panic message.
- A job refused at start (J3) never runs and publishes nothing: the start call itself returns the refusal.
- A job's id can cancel it while it runs (`cancel_job`): this sets its token and returns whether a job with that id was running.
- The running jobs can be listed (U32's `suspend` cancels them through this list).

## J2. The job pool

- Job work runs on a bounded pool of worker threads (4) created through the thread registry (J6) at user-initiated QoS: the user started these jobs (import, export, sync, Check Database) and watches their progress. Background-only work keeps its own utility threads (the optimizer's pool, the derived writer). A job started while every worker is busy waits in FIFO order; a cancel while it waits ends it as `Cancelled` when a worker takes it, without running the work.
- Sync and media sync run their network work on meguri-sync's tokio runtime (KTD14); they still register as jobs here for their handle, cancellation and `JobFinished`. This is the manual Anki transfer path (U105 to U111, frozen; KTD26). CloudKit sync works as J2.1 says.
- A job never waits for another job queued behind it: with 4 workers that can deadlock.

### J2.1 CloudKit work (KTD25)

Added for CloudKit. J2 does not change for other jobs.

- CloudKit network work runs in Swift: U114's actor owns `CKSyncEngine`, the Apple network operations, their retries and backoff. None of it runs on the tokio runtime, in the job pool, or in a Rust worker that waits for a Swift callback.
- U151's `SyncCoordinator` drives it. It executes each action the policy returns (`sync/policy#P4`) through the actor, then reports the outcome to the core as a policy input (`sync/policy#P3`). It calls `sendChanges` and `fetchChanges` only from its own tasks (`sync/policy#P2`).
- The core's side is short calls, ops and a few jobs:
  - short calls, off the main thread and never on the writer: the policy's `step`, batch builds, and the tracking commits that record save results, fetched records and engine state (`sync/cloudkit-records#K10.1`);
  - ops on the writer: barriers, `CloudIncoming` and `CloudRekey` (`engine/op-model#O1.1`);
  - jobs, as J1 says, for long core work of the recovery ops: the validation of a staged collection, and the exclusive phase with its safety backup and replace (`sync/cloudkit-recovery#V14.2` steps 5, 7 and 8). Each has a handle, a cancel token, progress and one `JobFinished`.
- A Sync Now exchange is not a job. It has no `JobHandle` and publishes no `JobFinished`. It is cancelled through `SyncNowCancelled`, and it ends with the `ExchangeEnded` input and its result (`sync/policy#P12`). Background and quit flushes are not jobs either (`sync/policy#P11`).
- CloudKit work takes no thread of its own in the core; the threads of J6 do not change.

## J3. Arbitration (C7)

- Jobs that take the long-operation slot (`engine/lifecycle` L5, L9) take it when they start. If it is held, the start call returns `Busy{running_op}` naming the holder, synchronously. The job holds the slot until its work returns; the slot is released before `JobFinished` is published, so a caller that starts another job when it sees the event is not refused.
- Normal and media sync take no slot and may run together, subject to independent protocol admission.
- CloudKit sends, fetches and applies take no slot, and a slot holder never refuses them (`engine/lifecycle#L9`). The policy's gate decides when they run (`sync/policy#P6`).
- `CloudAdopt` (`sync/cloudkit-recovery#V14`) is a recovery op made of several core calls and jobs, not one job. It takes the slot at V14.2 step 3 with a synchronous call that returns the slot lease (`engine/lifecycle#L5`), or `Busy{running_op}` when another holder has it (`sync/cloudkit-recovery#V21`). U113's op keeps the lease, an opaque FFI token, across its calls and jobs until V14.2 step 9 or until the op ends, then releases it; dropping the token releases it too.
- A job start may take a slot lease. Such a job runs under the lease: it does not take the slot, is not refused because the slot is held, and leaves the slot held when it finishes. A lease freed by a close, or of a generation that is no longer current, refuses the start with `CollectionUnavailable` or `StaleGeneration` (`engine/lifecycle#L5`). A start without a lease takes the slot as the first bullet says.
- (Manual Anki transfers, frozen.) An independently started normal or media sync cannot overlap a continued-processing one-way sync: before its work runs, its start returns synchronous `Busy{running_op}`, naming the one-way slot holder. This applies the transfer serialization of `sync/anki-transfers` M2 and `sync/normal-sync` N15, and the media exclusion of `sync/one-way` O4 step 6. The same authorized transfer's internal protocol phases are not another independent operation. This rule does not make normal or media sync a slot owner or prescribe their admission against other slot holders.

## J4. Cancel tokens

- A token is an atomic flag plus an async waiter: blocking work polls it between steps; async work (sync, on tokio) awaits it. Requesting cancellation is idempotent and never blocks.
- Swift task cancellation never reaches Rust: Swift cancels only through the token (U6 exports it).
- What cancellation means for an op (before and after its write phase starts) is `engine/op-model` O5. A cancelled operation leaves the collection unchanged (R1860).

## J5. Progress

- A progress sink carries a phase, completed and total counts (total unknown is allowed), and an ETA.
- Phases: `Planning` (cancellable work before any write), `Committing` (the write phase: the UI stops offering Cancel), and named phases a job defines for its own stages.
- Throttling (R1858): the sink calls its listener at most once per 100 ms; reports in between are dropped, so the next delivered report carries the newest counts. A phase change is always delivered at once, whatever the time since the last delivery. Reports made before a listener is attached are dropped; the newest phase is kept and goes with the next delivered report.
- ETA: within a phase with a known total, the time left at the phase's average rate so far (`elapsed / completed × remaining`); none until something completed.
- The listener is called on the reporting thread with no lock of the sink held; the Swift implementation only yields into an `AsyncStream` (KTD9). A listener attached later replaces the earlier one.

## J6. Threads

- Every thread the core creates goes through the engine's thread registry: it names the thread, sets its QoS class at the thread's start (the pthread QoS API on Apple platforms; elsewhere the class is recorded only), and tracks whether it is busy.

  | Thread | Name | QoS |
  |---|---|---|
  | collection writer | `meguri-writer` | user-initiated |
  | job pool workers | `meguri-job-<n>` | user-initiated |
  | event dispatcher (U6) | `meguri-events` | user-initiated |
  | search workers (U85) | | user-initiated |
  | derived writer and builder (U26, `storage/derived-store` D12) | `meguri-derived-writer`, `meguri-derived-builder` | utility |
  | stats cache builds, optimizer pool (U47) | | utility |

- The registry lists the live threads with their name, the QoS they asked for, the QoS the OS reports for them (Apple) and whether they are busy. U32 checks after `suspend` that every registered thread is idle.
