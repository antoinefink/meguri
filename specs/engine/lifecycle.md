# Collection lifecycle, generations and arbitration

Spec: engine/lifecycle
Requirements: R171 (core arbitration), R1489 (generation side), R1670 (core side), R1706, R1797 (waits); supports R458, R1659
Anki: none (Meguri design: no Anki behavior; written by the implementer, U24 execution note)
Scenarios: none; tests in `core/crates/meguri-engine/tests/`

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

Transitions: `NoCollection`/`Failed` → `Opening` (open) → `Upgrading` (only when the gate upgrades) → `Ready` or `Failed`; `Ready` → `ExclusiveBusy` → `Ready` (with a new generation after a replacement, the same one after a cancel); `Ready` → `Failed{Corrupt}`; any open state → `NoCollection` (close). Each change is published as a `Lifecycle` event (`engine/change-stream` S3).

## L2. Opening

1. The state becomes `Opening`; the gate's upgrade progress moves it to `Upgrading` and back to `Opening`.
2. Once the gate succeeds, the generation increases by one and the writer thread starts, owning the gate's writer connection. Readers open lazily (L11).
3. The open steps registered at open (U43's pending-answer replay) run in order, each with a privileged handle (L4) of the new generation: their ops are the only ones admitted.
4. The engine's own last open step: the day clock's first check (`sched/day-clock` D8), through the same privileged handle. It writes and unburies before anything can read counts, and publishes no day clock event.
5. The state becomes `Ready{generation}`.

A gate refusal or error, or an open step's error, leaves `Failed{reason}` with nothing open.

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
- From `Failed{Corrupt}` only restore and U132's recovery download (a one-way download) may enter `ExclusiveBusy`.

## L5. The long-operation slot

- One holder at a time among one-way sync, restore, `.colpkg` import, `.apkg` apply and Check Database (R171). A second request gets `Busy{running_op}`, naming the holder; it never queues.
- Holding the slot leaves the state `Ready`: answers and edits still commit, normal and media sync still run.
- Only the replacement ops (one-way sync, restore, `.colpkg` import) may then enter `ExclusiveBusy` (L7) (KTD17).
- The slot can be taken in `Ready`; in `NoCollection` or `Failed` only by a replacement op (it creates or replaces the collection); in `Failed{Corrupt}` only by restore and one-way sync.
- The slot belongs to one generation: the current one when it was taken (with nothing open, the last one), then the one its holder's own replacement makes (L8). Closing the collection frees the slot (L10). A holder whose slot a close freed gets `CollectionUnavailable` when it tries to enter the exclusive phase; a holder whose generation is no longer current (another collection was opened) gets `StaleGeneration`. Such a holder never acts on another collection, and dropping it does not free a slot taken since.
- Releasing the slot (the holder drops it) may make the collection available (L6).

## L6. Availability and waits

- The collection is available when the state is `Ready` and the slot is free.
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
4. If the swap succeeded, or failed after it changed the file (any of the noted values differ): the undo history is cleared with `ClearedByOneWay` on the writer, before any op of the new generation (`engine/undo` M6). The generation increases by one, and `CollectionReplaced{generation}` takes the next seq, so it precedes every event of the new generation. Still on the writer, the day clock starts again as at an open (`sched/day-clock` D8): it forgets its cached day and what it reported, and its first check on the new file commits (writes, unbury, its change set after `CollectionReplaced`); a failure there is logged and the next check computes afresh. Only then do readers open, lazily, on the new file, `CollectionReplaced` is delivered, once, and every `GenerationListener` is told (U26 invalidates derived data, R1489). So whoever hears of the new generation, or reads it, finds its day computed and its cards unburied, never the old collection's cached day. A failed swap still returns its error, and the exclusive phase's handle is the new generation's. A swap that failed before changing the file leaves the generation, and every handle, as they were.
5. Handles of older generations now fail with `StaleGeneration` (L3).

## L9. Which job takes which tier

| Job | Slot | Exclusive phase |
|---|---|---|
| one-way sync (upload, download, recovery download) | yes | yes |
| restore | yes | yes |
| `.colpkg` import | yes | yes |
| `.apkg` import (apply) | yes | no |
| Check Database | yes | no |
| normal sync, media sync | no (they overlap) | no |
| Check Media, Empty Cards, export, backup, optimizer, bulk ops, login, `meta` probe | no | no |

## L10. Close

Admission stops (queued ops still commit), parked ops fail with `CollectionUnavailable`, readers close, the undo history is dropped (`engine/undo` M6), the writer checkpoints and closes (C10), the writer thread ends and the state becomes `NoCollection`. The long-operation slot is freed and an exclusive phase ends (L5, L7). Once the writer is stopping, every task submitted to it fails at once with `CollectionUnavailable`, a replace's included, so nothing waits on a writer that is gone.

## L11. Readers

- The engine hands out read-only connections by role (KTD3, `storage/open-and-connections` C6): interactive, query, one per search worker, sync, maintenance, two server readers. Each is opened when first asked for and kept until the collection closes or is replaced; one user holds it at a time.
- Long-lived snapshots go to the sync reader (normal and one-way sync) or the shared maintenance reader (backups, `VACUUM INTO`, stats cache builds, derived rebuilds, planning heavy ops), never to the interactive, query or server readers. A sync holding the sync reader therefore never delays a backup, a stats build, a search or an interactive read.
