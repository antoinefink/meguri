# Collection lifecycle, generations and arbitration

Spec: engine/lifecycle
Requirements: R171 (core arbitration), R1489 (generation side), R1670 (core side), R1706, R1797 (waits); supports R458, R1659, R2056 (`CloudAdopt`)
Anki: none (Meguri design: no Anki behavior; written by the implementer, U24 execution note)
Scenarios: none; tests in `core/crates/meguri-engine/tests/`
Status: Approved 2026-10-27 (R2066) for the `CloudAdopt` and CloudKit parts of L4, L5, L6, L9 and L11, with `sync/policy`, `sync/cloudkit-records` and `sync/cloudkit-recovery`. Every other section is in force: L1's transitions to and from `Failed{Corrupt}`, L2's recorded corrupt verdict and the Check Database parts of L4 and L5 were updated for `maintenance/optimize-integrity#M7.2` (U132, U128) and are in force; the rest is unchanged.

The states of the collection the engine holds (C7), the generation that keys every handle, which ops the executor admits in which state, the long-operation slot, the availability wait the local servers use, the exclusive phase, and the generation change after a collection swap. Ops are `engine/op-model`; jobs are `engine/jobs`.

## L1. States

| State | Meaning |
|---|---|
| `NoCollection` | nothing open |
| `Opening` | the open gate is running (`storage/open-and-connections` C2), then the open steps (L2) |
| `Upgrading{from, step}` | the gate's upgrade is running step `step` (`storage/schema-upgrade`) |
| `Ready{generation}` | open; ops run |
| `ExclusiveBusy{op, phase, cancellable}` | a replacement op runs its exclusive phase (L7); `phase` is `Draining`, `Working` or `Replacing` |
| `Failed{reason}` | the open failed (the gate's refusal or error, or an open step's error), or the collection is corrupt (`Corrupt`, set by U132) |

Transitions: `NoCollection`/`Failed` → `Opening` (open) → `Upgrading` (only when the gate upgrades) → `Ready` or `Failed`; `Ready` → `ExclusiveBusy` → `Ready` (with a new generation after a replacement, the same one after a cancel); `Ready` → `Failed{Corrupt}`; `Opening` → `Failed{Corrupt}` (a recorded corrupt verdict, L2); `Failed{Corrupt}` → `Ready` with the same generation (a sound verdict of Check Database, L4); any open state → `NoCollection` (close). Each change is published as a `Lifecycle` event (`engine/change-stream` S3).

## L2. Opening

1. The state becomes `Opening`; the gate's upgrade progress moves it to `Upgrading` and back to `Opening`.
2. Once the gate succeeds, the generation increases by one and the writer thread starts, owning the gate's writer connection. Readers open lazily (L11).
3. The open steps registered at open (U43's pending-answer replay) run in order, each with a privileged handle (L4) of the new generation: their ops are the only ones admitted. The derived store's step comes first, before any op of the open commits (`storage/derived-store` D5).
4. The engine's own last open step: the day clock's first check (`sched/day-clock` D8), through the same privileged handle. It writes and unburies before anything can read counts, and publishes no day clock event.
5. The state becomes `Ready{generation}`.

A gate refusal or error, or an open step's error, leaves `Failed{reason}` with nothing open.

When `lastQuickCheck.passed` is false (`maintenance/optimize-integrity#M5.1`):

- Read this result before the storage gate may alter the collection.
- Probes may read. The normal open may not upgrade the schema, roll back a journal, run a checkpoint, set journal mode or otherwise write the collection.
- Enter `Failed{Corrupt}` with a new generation. Skip every open step.
- Keep the collection owner so L4 recovery requests, read-only readers, maintenance status (`maintenance/optimize-integrity#M10`) and close remain available. Neither a writable connection nor a completed probe is needed to keep this owner.
- If a read needs journal recovery, return its error without recovery. Keep the owner for a later recovery request.
- A corrupt result found in `Ready` (U132) also enters `Failed{Corrupt}`. Keep that generation and stop ordinary collection writes.
- Remedies and file replacement retain their existing contracts.

## L3. Generations

- A generation is a number that names one opened collection: 0 before anything was opened, then one more at every successful open and every replacement (L8), for the life of the engine.
- Every collection handle carries the generation it was made for. An op submitted through a handle carries it as its planned generation; an op, a reader request or a snapshot through a handle of an older generation fails with `StaleGeneration{planned, current}`. A handle for the current generation comes from the engine at any time.
- A reader request checks the generation while it holds the reader, never before taking it. A replacement closes every reader before the generation changes (L8), so a request that waited while a replacement completed fails with `StaleGeneration`, and the generation cannot change while a reader is held.

## L4. Admission

When an op is submitted, the executor decides by the current state:

| State | Admitted | Others |
|---|---|---|
| `Ready` | every op of the current generation | |
| `Opening` | ops of the open steps (privileged) | refused or parked |
| `ExclusiveBusy` | ops and maintenance tasks of the exclusive job (privileged) | refused or parked |
| `NoCollection`, `Upgrading`, `Failed` | none | refused or parked |

- Refused means the result `CollectionUnavailable{reason}` at once. Parked means the op waits outside the queues until the state is `Ready` again; it is then checked again: if it was planned on an older generation it fails with `StaleGeneration`, otherwise it is queued and runs on the new generation. Ops are refused unless their spec asks to be parked. Parked ops fail with `CollectionUnavailable` when the collection closes or fails instead.
- A privileged handle carries a token the lifecycle issued for the open steps or the exclusive phase; it stops working when that phase ends.
- From `Failed{Corrupt}` only restore, the adoption of an iCloud collection (`CloudAdopt`, `sync/cloudkit-recovery#V19`) and the recovery download of `sync/one-way` (a one-way download; a manual Anki transfer, frozen, KTD26; not a remedy of U132, `maintenance/optimize-integrity#M7.1`) may enter `ExclusiveBusy`. `CloudAdopt` was added for CloudKit; the other two do not change.
- In `Failed{Corrupt}`, Check Database (U128, once it lands) may take the slot (L5) and run its integrity stage on the maintenance reader (`maintenance/optimize-integrity#M7.2`; `conformance/collection-check` K3, K10). Nothing else is admitted meanwhile: no op and no maintenance task. A corrupt verdict leaves the state `Failed{Corrupt}`. A sound verdict returns the state to `Ready` with the same generation, since no file was replaced, and Check Database goes on as from `Ready`; one `Available` event follows once its slot is released (L6). Check Database never enters `ExclusiveBusy`.
- CloudKit ops (`CloudIncoming`, `CloudRekey` and the barriers, `engine/op-model#O1.1`) are admitted only in `Ready`, and refused, never parked, in every other state.

## L5. The long-operation slot

- One holder at a time among one-way sync, restore, `.colpkg` import, `.apkg` apply, Check Database (R171) and `CloudAdopt` (Replace local collection from iCloud, also at a first connection; `sync/cloudkit-recovery#V14`). A second request gets `Busy{running_op}`, naming the holder; it never queues.
- Holding the slot leaves the state `Ready`: answers and edits still commit, and otherwise admissible unrelated jobs still run. Normal and media sync remain subject to independent protocol admission (`engine/jobs` J3). CloudKit sends, fetches and applies take no slot and are not refused because of a holder (L9).
- Only the replacement ops (one-way sync, restore, `.colpkg` import, `CloudAdopt`) may then enter `ExclusiveBusy` (L7) (KTD17).
- The slot can be taken in `Ready`; in `NoCollection` or `Failed` only by a replacement op (it creates or replaces the collection); in `Failed{Corrupt}` only by restore, `CloudAdopt`, one-way sync and Check Database (U128, `maintenance/optimize-integrity#M7.2`; it creates or replaces nothing, L4).
- `CloudAdopt` takes the slot at `sync/cloudkit-recovery#V14.2` step 3 and holds it until step 9 or until the op ends. Its staging (step 4) runs while it holds the slot, in `Ready`, `NoCollection` or `Failed{Corrupt}` (`sync/cloudkit-recovery#V19`); in `Ready` study and edits continue, and only step 7's exclusive phase stops them. Until that phase starts, its hold does not make the collection unavailable (L6). The prefetch of a connection decision (`sync/cloudkit-recovery#V13.2`) and Replace iCloud collection (`#V15`) take no slot.
- **Slot lease.** Taking the slot returns a lease: an owned value that names its holder, which `Busy{running_op}` reports, and the generation it belongs to. A job started with a lease runs under it and does not take the slot again (`engine/jobs` J3). Dropping or releasing the lease frees the slot; a close frees the slot and the lease then stops working (L10), as below. The FFI exposes it as an opaque token, so a holder made of several calls, such as U113's `CloudAdopt` op, keeps it between them. A holder that is one job keeps the lease inside the job, as today.
- One-way sync in this section is a manual Anki transfer (frozen; KTD26). `CloudAdopt` was added for CloudKit; the rules for the other holders do not change.
- The slot belongs to one generation: the current one when it was taken (with nothing open, the last one), then the one its holder's own replacement makes (L8). Closing the collection frees the slot (L10). A holder whose slot a close freed gets `CollectionUnavailable` when it tries to enter the exclusive phase; a holder whose generation is no longer current (another collection was opened) gets `StaleGeneration`. Such a holder never acts on another collection, and dropping it does not free a slot taken since.
- Releasing the slot (the holder drops it) may make the collection available (L6).

## L6. Availability and waits

- The collection is available when the state is `Ready` and the slot is free, or held only by a `CloudAdopt` that has not entered its exclusive phase (L5): its staging and its wait at step `ready` (`sync/cloudkit-recovery#V14.2` steps 4 to 7, V20) do not touch the collection, so local servers are served meanwhile. When it enters the phase the collection becomes unavailable, as for any exclusive phase (L7).
- A waiter (U135 wraps it for both local servers) blocks until the collection is available, with a timeout the caller chooses: 5 s for Migaku (R1706, U139), 30 s for AnkiConnect (R1797, U141). On timeout it gets `CollectionUnavailable` (a refusal naming the slot holder or the state). When available it gets a handle of the current generation.
- Whenever the collection becomes available after not being available (a slot release, the end of an exclusive phase, an open), one `Available` event is published (`engine/change-stream` S3), and every waiter wakes.

## L7. The exclusive phase

The core half of KTD17. A replacement op holding the slot enters it:

1. Each registered `SuspendParticipant` is asked to flush (U43 flushes its answer overlay into ops) while the state is still `Ready`.
2. The state becomes `ExclusiveBusy{op, Draining, cancellable}`: from now on only the exclusive job's ops are admitted (L4).
3. Every op queued before the switch commits (the drain). Then the phase is `Working` and the job gets its privileged handle. The job's safety backup (U127) and any `VACUUM INTO` snapshot come only after this point.
4. The phase ends either by a replacement (L8) or by the job dropping it (cancel or failure). Either way the state returns to `Ready` and parked ops are checked again (L4).
5. Closing the collection also ends the phase (L10). A replace after that fails at once with `CollectionUnavailable` and never runs the swap; dropping the phase then changes nothing.

## L8. Generation change

Every swap of the collection file goes through this procedure; U124's verified-replace primitive is the swap itself:

1. The phase becomes `Replacing`. Every reader is closed (waiting for readers in use), then the writer: checkpoint and close (`storage/open-and-connections` C10).
2. The swap runs with the collection path and nothing open. The engine notes which file the path names, its size, its modification and change times and its WAL's size before the swap, and compares them after it.
3. The file is opened through the open gate, whether the swap succeeded or not. If that fails the state becomes `Failed{reason}`.
4. If the swap succeeded, or failed after it changed the file (any of the noted values differ): the undo history is cleared with `ClearedByOneWay` on the writer, before any op of the new generation (`engine/undo` M6). The generation increases by one, and `CollectionReplaced{generation}` takes the next seq, so it precedes every event of the new generation. Still on the writer, the day clock starts again as at an open (`sched/day-clock` D8): it forgets its cached day and what it reported, and its first check on the new file commits (writes, unbury, its change set after `CollectionReplaced`); if it fails, the replace fails as a failed reopen does: the writer closes the new file, no reader opens, `CollectionReplaced` is not published, no `GenerationListener` is told, the generation number stays used, and the state becomes `Failed{reason}` with the check's error (the swap's, when the swap failed too). Only then do readers open, lazily, on the new file, `CollectionReplaced` is delivered, once, and every `GenerationListener` is told (U26 invalidates derived data, R1489). So whoever hears of the new generation, or reads it, finds its day computed and its cards unburied, never the old collection's cached day. A failed swap still returns its error, and the exclusive phase's handle is the new generation's. A swap that failed before changing the file leaves the generation, and every handle, as they were.
5. Handles of older generations now fail with `StaleGeneration` (L3).

## L9. Which job takes which tier

| Job | Slot | Exclusive phase |
|---|---|---|
| one-way sync (upload, download, recovery download; manual Anki transfer, frozen) | yes | yes |
| restore | yes | yes |
| `.colpkg` import | yes | yes |
| `CloudAdopt` (Replace local collection from iCloud; adoption at a first connection) | yes, from `sync/cloudkit-recovery#V14.2` step 3 to step 9 | yes, step 7 to step 8 |
| `.apkg` import (apply) | yes | no |
| Check Database | yes | no |
| normal sync, media sync (manual Anki transfers, frozen) | no (they may overlap each other, subject to J3 admission) | no |
| CloudKit sends, fetches, barriers, `CloudIncoming` applies and `CloudRekey` ops | no | no |
| CloudKit prefetch for a connection decision (`sync/cloudkit-recovery#V13.2`), Replace iCloud collection (`#V15`), conflict-copy operations (`#V17`) | no | no |
| Check Media, Empty Cards, export, backup, optimizer, bulk ops, login, `meta` probe (login and `meta` are manual Anki transfers, frozen) | no | no |

The CloudKit rows were added for CloudKit (`sync/policy#P23`); the others do not change.

## L10. Close

Admission stops (queued ops still commit), parked ops fail with `CollectionUnavailable`, readers close, the undo history is dropped (`engine/undo` M6), the writer checkpoints and closes (C10), the writer thread ends and the state becomes `NoCollection`. Closing from `Failed{Corrupt}` instead performs no checkpoint, journal recovery or other collection write; it releases connections and ownership without changing the collection or its existing recovery journals. The long-operation slot is freed and an exclusive phase ends (L5, L7). Once the writer is stopping, every task submitted to it fails at once with `CollectionUnavailable`, a replace's included, so nothing waits on a writer that is gone.

## L11. Readers

- The engine hands out read-only connections by role (KTD3, `storage/open-and-connections` C6): interactive, query, one per search worker, sync, maintenance, two server readers. Each is opened when first asked for and kept until the collection closes or is replaced; one user holds it at a time.
- Long-lived snapshots go to the sync reader (normal and one-way sync) or the shared maintenance reader (backups, `VACUUM INTO`, stats cache builds, derived rebuilds, planning heavy ops), never to the interactive, query or server readers. A sync holding the sync reader therefore never delays a backup, a stats build, a search or an interactive read.
- CloudKit barrier snapshots, batch builds and the reconcile scan (`sync/cloudkit-records#K11.4`) use a reader role of their own, `cloud` (`sync/policy#P7.1`), so a manual transfer's long snapshot on the sync reader never blocks them, and they never wait for a backup. A scan holds it while no batch can be built (`sync/policy#P6`). The `cloud` role is added for CloudKit; `storage/open-and-connections` C6 adds it with `query_only` on, since it never runs `VACUUM INTO` (`sync/policy#P23`).
- The writer never waits for the cloud reader. The batch builder leases it off the writer, waiting there if a scan or an earlier build holds it, and then submits the barrier op with the lease; the barrier begins its read transaction on that lease (`sync/policy#P7.1` step 1) and hands the lease back with its result. A barrier op submitted without a lease fails with `InvalidArgument` and flags nothing.
- When a reader is leased, the derived store's attachment is checked for the lease's generation: the store's file is attached as schema `derived` when the store is bound to that generation, and detached otherwise (`storage/derived-store` D9).
