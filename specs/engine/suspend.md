# Suspend protocol

Spec: engine/suspend
Requirements: R14, R1453, R1454, R1491, R2006, R2007, R2011, R2014
Anki: none (Meguri design, U32)
Scenarios: tests/suspend.rs, tests/external_change.rs, tests/kill_loop.rs

## S1. Control lane

Engine lifecycle control calls are serialized. `flush_for_background()` flushes all SuspendParticipants while writes remain admitted, drains accepted writes, then full-syncs the WAL. It preserves undo and accepts following edits. Participants must hand pending writes to the executor before returning.

`suspend(timeout)` is final, idempotent after success, and returns Suspended or Deferred{job_id}. Cancel every ordinary job. Continued-processing is explicitly selected at job start, permitted only for OneWaySync, ColpkgImport and ApkgImport. If one remains, return Deferred without closing admission or suspending participants. A cancelled continued job is still deferred until it actually releases its connections. No new job starts during the final quiesce. The registry gate also covers direct job starts: those return an already-cancelled handle and a Cancelled completion without running work. Resume reopens that gate.

Otherwise flush participants, close admission, drain accepted ops, quiesce participants, wait for cancelled jobs, close the derived writer connection and every reader, TRUNCATE-checkpoint, save a clean sidecar and close the writer's SQLite connection. Keep the engine, undo history and advisory process lock. The thread registry must report every core thread idle before success. Any remaining busy thread makes the attempt fail at its deadline; it cannot silently escape enumeration. Reader leases and participants have deadline-aware waits. Default participants must return promptly; implementations with lengthy work override suspend_until. Deadline failure never claims success or a clean sidecar and leaves admission closed for retry or resume.

Connection release has a separate `release_connections_until` participant hook: quiesce workers first, close engine readers and their attached databases, then release participant-owned connections. In particular, the derived writer must outlive its readers so its final close can remove its WAL. A flush failure closes admission too and remembers that flushing is incomplete; a retry repeats the write-admitting flush phase before final quiescence.

## S2. Resume

`resume()` reopens SQLite if suspended and durably marks the sidecar unclean, then performs external-change checks and rollover, in that order, before admitting public ops. It returns ResumeReport { external_change, rolled_over, next_day_at } (epoch seconds). It invokes no sync, queue rebuild or widget work: U151 schedules those. Already-active resume still checks the writer data_version and rollover. Participants resume after the core checks; U26 can then rebuild invalidated indexes. A failed resume never admits writes.

## S3. External change

On change, reset undo/redo with ChangedExternally, discard cached day values and optimistic touched-table assumptions, invalidate U26's entire derived store, and publish a ChangeSet marked whole_collection. Notify in-core observers with the same set before publishing. This is a conservative invalidation, not a generation change. Open uses the prior sidecar mod/scm; before-write checks run after BEGIN IMMEDIATE, so a foreign commit cannot slip between the check and an op's transaction. An op planned before an external change must replan (Stale) rather than overwrite values planned from stale caches. The next op may run against current rows.

A failed reopen retains the original suspended-file stamp for the next resume attempt. Its held file descriptor is released before opening SQLite again, so a later descriptor close cannot drop SQLite's POSIX locks.

## S4. Close and deadlines

Close uses the same participant flush/quiesce, job cancellation and connection drain, then terminates the writer. Timeout errors identify the phase. A caller retries suspend or resumes to recover; no detached helper may keep SQLite work running after a successful suspend. The app must supply its remaining background budget and call again after Deferred completes.

Close cancels continued-processing jobs too and uses a five-second quiescence budget. Cancellation precedes participant flush/quiescence; admission closes and accepted writes drain before participants quiesce. Close retains the collection on quiescence failure so the caller can retry or resume.
