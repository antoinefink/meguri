# Hold-back sync: snapshot and stage

Spec: sync/hold-back
Requirements: R1495, R1845, R2006 (not delayed by sync), R1520 (undo and queues), R1530 (the outgoing view), R2012, R2060, R2062, R2063; KTD6, KTD7, KTD26; AE9 (core part), AE27 (core part), AE68 (first clause)
Anki: none. Meguri design from KTD6, KTD7 and U107; Anki does not sync during a review session (R726) and blocks its collection while it syncs (R1495), so nothing here comes from Anki. The data a round leaves on both sides is `normal-sync.md`'s.
Scenarios: conformance/scenarios/sync/holdback/ (with the deviations of H17); mechanism tests in core/crates/meguri-sync/tests/normal_sync.rs (H18)

Manual-transfer compatibility (KTD26; frozen with U107): this is the hold-back of a manual Anki normal sync. CloudKit hold-back during review (R88, R726, R2047) is `sync/policy#P8`, and background expiry (R1554) is `sync/policy#P11.2`. Where this file cites R88, R726 or R1554, it means only the manual-transfer case.

How Meguri runs the normal sync of `normal-sync.md` without ever making an answer wait, while keeping the newest answer run of a review session undoable and off the server. The round reads what it sends from a read snapshot taken between two ops, with the held-back run's rows replaced by their earlier images; it stages what the server sends in a scratch file; and it applies the stage in one short write that respects every write made meanwhile. The undo side of the contract (the held-back run, upload states, the merge rule) is `engine/undo` M8 and M9; ops, sources and declared writes are `engine/op-model`.

`normal-sync.md` N15 governs entry. These mechanics do not start automatic Anki
work or bypass the current directional transfer's active-session guard.

Own-provider exception (2026-11-16): `sync/meguri-provider` MP1–MP5 supplies the own-Meguri-server caller's admission and scheduling. Reuse these hold-back mechanics without changing CloudKit's P8 rules or the legacy directional transfer's active-review guard. Legacy Anki work remains frozen.

## H1. What the job guarantees

1. Answers and edits made while the job runs commit at once through the op executor and are durable within 1 s; none waits for the network phase (R1495, R1845, R2006). During the job's own write (H10) answers go to U43's overlay and journal (H10.5).
2. The server receives exactly what `normal-sync.md` would send from the **outgoing view** V (H4): the collection as of the snapshot, minus the held-back run. Nothing written after the snapshot, and nothing of the held-back run, reaches the server in this round.
3. The client's sanity counts describe V merged with the server's answer, which is what the server holds after the round (H8), so a correct round never fails `sanityCheck2` because of hold-back.
4. After the commit, every row the round sent and that nobody changed since the snapshot carries the server USN; every other local change is still pending and goes up with a later round (H10.3).
5. No step whose rows the server holds is ever undone (`engine/undo` M9.5); the held-back run stays undoable unless the server's answer touches it (H11).
6. A failure before `finish` was sent leaves the collection as it was and every undo step as undoable as before (H12).

## H2. Phases

| Phase | Where | Holds the writer |
|---|---|---|
| Preflight (H3) | job thread, then one `Rollover` op | only for the rollover op |
| Snapshot barrier (H4) | the writer, between two ops | for the barrier only (no row read beyond the first) |
| Network phase: `start` to `finish` (H5 to H9) | job thread and meguri-sync's tokio runtime, reading the sync reader | no |
| Commit (H10) | one `SyncIncoming` op | for its write phase |
| Reconciliation (H11) | inside the commit op and right after it | no |

The job runs as a core job (`engine/jobs`), started from the maintenance lane, holding no long-operation slot; a media sync may run beside it. A second normal sync job of the same collection is refused with `exclusive_busy` while one runs.

## H3. Preflight and lifecycle checks

1. When the lifecycle is `Failed{Corrupt}` (U132), the job fails with `collection_corrupt` and sends nothing, not even `meta` (R2012).
2. It runs `checkRollover()` (`sched/day-clock` D8) and waits for the `Rollover` op it may submit to commit, so the unbury of `normal-sync.md` N2 is in the snapshot.
3. It runs the status check (`meta-status.md` M2, M3) on the sync reader, holding no write lock. `lnewer` is computed from the `col.mod` read for it. Unless the check calls for a normal sync, the job ends there.
4. Before the graves step (its first `applyGraves`, or `applyChanges` when it has no grave to send), before the first `applyChunk`, and before `finish`, the job reads the lifecycle again. When it became `Failed{Corrupt}` (a background quick check failed meanwhile), the round fails with `collection_corrupt` as at any failure before `finish` (H12): `abort` is sent, no `finish`, nothing is applied.

## H4. The snapshot barrier and the outgoing view

### H4.1 The barrier

Right after the status check, the job asks the writer for a barrier: an op that writes nothing and runs between two queued ops. On the writer, before the next op is taken, it:

1. begins the read transaction of the sync reader (`storage/open-and-connections` C6) and runs one read on it, so the snapshot is fixed at that point;
2. records S, the seq of the last committed op (`engine/op-model` O4);
3. calls `begin_sync_hold(session)` (`engine/undo` M9.1) with the active review session that U43's registry reports, or none: it returns the held-back run R as a value and flags every other step up to S that is not yet uploaded as `UploadPending`.

The job keeps the read transaction until its network phase ends (seconds; it reads rows in chunks of at most 250, so memory stays bounded), and keeps R until the commit. Later changes to the undo history (a rollover clearing it, the user undoing a step of R) change neither S, the snapshot nor R: the job uses only the values it captured (H10.3 sees to the rows R's steps touched).

### H4.2 The outgoing view V

V is the snapshot with R's rows put back as they were before R:

- every row of R's journals (`engine/undo` M8: the before-image from the oldest step of R that touched it) replaces the snapshot's row with that key; a row R created is absent from V; a row R deleted is present in V as it was;
- graves are rows like any other: a grave R added is absent, so a deletion held back sends no grave;
- review rows R added are absent; `config` keys R changed have their earlier value, `usn` and `mtime_secs`;
- the `col` row is the snapshot's; R never holds it.

V's pending rows are the rows of V with `usn` -1. A before-image may itself be pending (a change made before R and not yet sent): it is sent. A before-image with a server USN is not.

| In the session, before the snapshot | V holds | Sent |
|---|---|---|
| 30 answers; R = the last answer | cards 1 to 29 answered, card 30 as before its answer, 29 review rows, the deck after answer 29 | 29 cards, 29 review rows, the deck (AE9) |
| answer #c1, answer #c2, then flag #c2; R contains the two newest steps | #c2 keeps its earlier row; the deck reflects only answer #c1 | #c1, its review row, the deck |
| answer of #c1, then Delete Note of its note; R = both | the note and #c1 as before the answer, no grave, no review row | nothing of them |
| an answer, with no session | R empty: V is the snapshot | everything pending |

## H5. What the round sends

The round sends what `normal-sync.md` N4.2, N5.1 and N7 select, from V: V's pending graves, note types, decks, presets and tags; with `lnewer`, V's whole `config` table and `crt`; V's pending review rows, cards and notes, collected when the `chunk` loop has ended. Each sent object carries the server USN (`wire-format.md` W6.2). The job records the keys it sent, by table, for H9 and H10.3.

Nothing is stamped while sending: the snapshot is read-only. The stamps of `normal-sync.md` N9.2 are applied by the commit (H10.3).

## H6. The stage

What the server sends (its graves from `start`, its `applyChanges` answer, every `chunk` answer) is written, as it arrives and once decoded, into a scratch SQLite file, the stage, which the commit reads.

- The stage is `meguri-sync-stage.db` in the collection's directory, with its journal files. A job deletes any stage it finds before it creates its own; only one normal sync job runs at a time (H2). The launch sweep removes it (R1493, U32), and so does the job when it ends, whatever the outcome.
- The job creates the stage after the barrier and before it sends `start`, once U32's free-space check has found room for it; a refusal ends the job before `start` with `disk_full` (`errors.md`). A write to the stage that fails for lack of space later is a failure like any other (H12).
- Staged notes carry the `flds` (control characters removed), `sfld` and `csum` of `normal-sync.md` N6.3, computed while staging (R1528) against the note type the commit will use: the staged incoming note type when H7 accepts it, otherwise the snapshot's.
- The stage is attached to the writer connection only inside the commit op (H10) and detached before that op ends. Nothing else reads it.

Repeated card/note/review IDs retain N6.5's received-order meaning. Scratch
view M can reduce a bounded page against the row state it currently holds,
then merge at most one effective row per table/key. The incoming target set
still records every distinct received key, including rejected or identical
occurrences; repetitions do not multiply targets or skipped-ID diagnostics.

For each incoming card/note key, keep a bounded summary: the greatest received
`mod`, and the effective row produced by N6.5 for that key starting absent.
Compute that row in received order, using its current pending state at each
occurrence. If every incoming tuple is nonpending, it is simply the final
tuple; a tuple with pending USN may instead keep an earlier effective row.
Across chunks, continue the same summary rather than starting it over.

At commit, select the effective incoming row if the key is absent, the stored
row is nonpending, or its pending `mod` is below the received maximum.
Otherwise keep the live pending row. This reproduces received-order merging:
every initial pending row below that maximum accepts the first maximum-mod
occurrence and reaches the same later state. A row at or above the maximum
accepts none. Thus pending live `mod` 20 with clean incoming `mod` 30 then 10
ends at 10, even if scratch M initially protected a different pending row.

For review keys, retain the first received row, independently of whether M
already had that ID. Store these summaries in the existing scratch stage,
with at most one effective payload per key, and process bounded pages. No
full incoming row-history table or all-stream RAM buffer is required. These
summaries decide row values, not acceptance-dependent note errors.

H10.2 step 7 compares post-S cards/notes by the live pending/mtime rule; a
post-S touch does not protect the whole key from an incoming merge. For exact
validation at that live scalar state, keep one compact validation
record per received note occurrence in the existing scratch stage: note ID,
global receive ordinal, incoming `usn`/`mod`, and either success or the existing
typed fault with numeric note/notetype IDs. No note body, field text, tags,
SQL, path or arbitrary error string is repeated in these records. Across
chunks the ordinal increases and the scalar state continues.
A fault accepted on M's own path still fails staging before `finish`.
Other acceptance-dependent faults stay deferred until live eligibility is
known; a hypothetical absent-state fault cannot fail staging by itself.

Validation records have at most one fixed-size record per received note
tuple and are read in bounded pages, with ordinary stage resource/cancellation
failure handling. This is input-sized scratch metadata, not a retained history
service or a set of full row versions. Do not call it constant per-key disk
space. RAM stays page-bounded and full incoming payload storage stays at most
one effective row per key. An exact equivalent compact representation is
allowed, but cannot omit conditional faults at other live thresholds.

Preparing a received note for this metadata is side-effect-free. Field
cleanup, checksum/sort calculation and tag splitting produce row values only;
no tags are globally registered and no collection/journal stamp changes.
An invalid hypothetical version records its typed fault without surfacing it
unless that occurrence is accepted on M or the eligible live path. A locally
rejected note must not fail merely because validation was computed internally.

## H7. Checks made while staging

Each check fails the round before `finish` is sent (H12):

| Check, against the snapshot and the stage | Kind |
|---|---|
| an incoming note type that `normal-sync.md` N5.2 accepts against the snapshot's note type with that id, whose field or template count differs from it | `notetype_schema_changed` (R1525) |
| a note accepted on M's N6.3 path whose `mid` is neither a note type of the snapshot nor a staged incoming one | `missing_notetype` (R1528) |
| a note accepted on M's N6.3 path whose field count differs from that note type's | `note_fields_mismatch` |

The additional live-path checks of H6 are conditional and are evaluated
at H10.2; they do not make a note rejected on M fail staging unconditionally.

## H8. Sanity counts (R1530)

The counts tuple the round sends (`normal-sync.md` N8 step 2) counts the rows of M, the state `normal-sync.md` N4 to N7 would produce from V and the stage: V, minus the rows the server's graves remove, plus the server's graves, note types, decks, presets, cards, notes and review rows that are not already in V (by id; graves by id and type). Only presence counts, so the merge rules that choose between two versions of a row change nothing here.

M is what the server holds once it has applied the round. M is not what the commit leaves in the collection: H10 also keeps the rows of R and every write made after S, and its skip rule (H10.2) can leave out an incoming row M counts. Those rows go up with later rounds, whose counts are taken the same way.

## H9. The pending check (R1530)

Before `sanityCheck2`, the job checks that it sent every pending row of V: for each of the eight tables of `normal-sync.md` N8, V's pending keys minus the keys it recorded as sent (H5) must be empty. A key left over is a bug: the round fails with `internal`. Rows kept pending on purpose (R's, and those written after S) are not in V's pending set, so they never fail this check.

## H10. The commit

### H10.1 The op

When `finish` has answered T, the job submits one op: source `SyncIncoming`, stamping mode `CallerSupplied` (`storage/ids-usn-mtime` S3), catalog entry `sync_incoming_merge` (`engine/op-model` O9). It attaches the stage, applies H10.2 to H10.4 in one transaction, detaches the stage and commits. It never touches a row it does not merge, mark or finalize.

Before anything else, the op compares the live `col.scm` with the snapshot's. When they differ (a schema change confirmed during the round, a Check Database repair that sets `scm`, or "force a one-way sync" (R1547)), the op applies nothing from the stage, marks nothing and does not finalize, and the job ends with "one-way sync needed" (`meta-status.md` M6.1). The server has committed the round, but the local `scm` already differs from the server's and exceeds `ls`, so the next status check calls for a one-way sync; the user confirms it (R951, R1476) and it settles both sides. As in Anki, no incoming change is applied after a schema change.

### H10.2 Merging the stage into the live collection

The rules are `normal-sync.md`'s, applied to the live rows (not to V), in this order:

1. **The server's graves** (N4.3): each listed note, card and deck row is removed and gets a grave with the server USN, insert-or-ignore.
2. **Skip rule.** An incoming card, note or deck (steps 3 and 7) whose own id has a live pending grave (`usn` -1) that this round did not send is skipped: a deletion that R holds back or that was made after S beats the server's version, as it would in Anki, where it would have gone up at `start`. The skipped ids are listed in the `SyncIncoming` change set's diagnostics. An incoming card whose note is absent locally for that reason is stored like any card whose note is missing (N6.2): skipping it would leave the server holding a card the client lacks, and the next round's sanity counts would differ.
3. **Note types, decks, presets** (N5.2 to N5.4, N5.9), against the live rows: the mtime rule, the `+` renames. The counts check of N5.2 was made in H7; the `scm` comparison of H10.1 guarantees that no live note type changed its fields or templates since.
4. **Config** (N5.7), when the server sent `conf`: the table becomes the received set, except the keys written by an op with a seq above S, which keep their live row (value, `usn` -1, `mtime_secs`) and stay pending. This gives the result Anki would give for a change made just after the sync.
5. **`crt`** (N5.6), when the server sent it.
6. **Tags** (N5.5).
7. **Cards and notes** (N6.2, N6.3), against the live rows: a live row that is pending with a `mod` equal to or greater than the incoming `mod` stays. Live rows of R and rows written after S are compared like any other.
8. **Review rows** (N6.1): inserted only when no live row has the id.

Within steps 7 and 8, repeated versions follow N6.5 in received order against
the live row after preceding occurrences, using H6's equivalent summaries.
Apply step 2's own-ID pending-grave
skip to the entire incoming key; it must not be re-created by a later
occurrence. R's rows and post-S rows keep the same pending/mtime protection
on each comparison. A live review row wins over every incoming duplicate;
when absent, only the first received review row is inserted. Incoming-target
tracking remains independent of which versions were accepted, so H11's undo
invalidation and the sorted unique skip diagnostics remain complete.

Before applying note summaries, determine accepted validation paths from
the actual live scalar rows after the grave phase. An own-ID unsent pending
grave masks all incoming versions and their deferred faults for that key.
For each unskipped key, stream its compact validation records in receive
order, testing acceptance against its current live scalar state;
an accepted valid occurrence updates the state with its incoming `usn`/`mod`,
and an accepted invalid occurrence is a fault. A rejected occurrence is not a
fault and does not update the state. Post-S touch alone never masks a fault.
Read one key's state and bounded record pages at a time, rather than loading
all validation records or simulated row states into RAM. An optional cached
S-path result requires proof that its base was captured before R's overlay,
adjusted for incoming graves, and exactly matches the live scalar base and
validation inputs. A post-S touch or guessed mtime equality is no such proof;
the ordinary replay path needs no S-derived shortcut.

Across eligible keys, select the earliest fault by global receive ordinal,
not by note ID or merge-page order. Return its existing typed note-validation
kind/IDs and roll back the entire local merge; publish no applied diagnostics,
USN marking or finalization. Once `finish` has been sent, H12's server-commit
rules still apply. Only when eligible paths have no fault may their effective
row summaries be merged. For M pending 40 and bad clean 30 then valid clean 10,
live pending 20 fails at the first tuple; live pending 40 rejects both and
remains unchecked. The result must hold within one chunk or across chunks.

Merges are set-based from the attached stage, through a meguri-storage bulk-merge path that records each changed row's before and after images in the op's journal and skips rows equal to the stored row (`storage/ids-usn-mtime` S4), so the `SyncIncoming` change set and every observer see each change.

### H10.3 Marking what was sent (R1529)

Every row and grave the round sent (H5) takes the server USN, and every `config` row it sent takes `usn` 0 (`normal-sync.md` N5.1), when all of these hold:

- (a) its key is not among the rows R's steps wrote (the keys of R's journals, graves included);
- (b) no op with a seq above S changed its key (the job collects these keys from the change sets and journals published since S, the rollover op's unbury included);
- (c) its live `usn` is still -1 (a row the merge replaced already carries the server's `usn`; a row now deleted is not there).

A row that fails (a) or (b) keeps `usn` -1 and goes up again in a later round, which the server applies idempotently. Comparing the live row with the image actually sent, column by column, is an acceptable replacement for (a) and (b). Comparing modification times is not: they have one-second resolution, so an answer and a flag in the same second would look unchanged.

| Row | Result after the commit |
|---|---|
| card answered before the session, sent, untouched since | server USN |
| card of R's answer (its before-image sent because it was pending) | `usn` -1 |
| card answered at 10:00:05 and sent, then flagged at 10:00:05 after S | `usn` -1; the next round sends the flag |
| note sent, then edited during the network phase | `usn` -1; the server holds the old version until the next round |
| card sent, then unburied by a rollover during the round | `usn` -1 (b); sent again, unchanged |

### H10.4 Finalizing

1. `col.ls` = T, `col.mod` = T, `col.usn` = the server USN + 1 (`normal-sync.md` N9.2, R1531).
2. When, after the steps above, any live row of `cards`, `notes`, `revlog`, `graves`, `decks`, `deck_config`, `notetypes` or `tags` has `usn` -1, or an op with a seq above S moved `col.mod`, then `col.mod` = T + 1 ms. So the offline status reads "local changes pending" and the next status check finds `mod` different from the server's (`meta-status.md` M5, M3); without it, a change kept pending would look synced. A config row the day clock stamped after S does not count: such writes never make a sync necessary on their own (`sched/day-clock` D5).

### H10.5 Write phase, budget and cancellation

- The op declares its expected write phase from the staged row counts. When the estimate exceeds about 200 ms, the writer publishes `WriteDelay` (`engine/op-model` O8): U43 keeps answers made meanwhile in its overlay and pending-answer journal (durable within 1 s, R2006, R1491) and commits them as soon as the op ends, each with its `stateToken` precondition (R727 to R729). Other writes wait for the writer.
- The op checks the job's cancel token between batches. A cancel, or `suspend(deadline)` (C8, U32), rolls the whole op back; the job then ends as H12 says for a failure after `finish` was sent. A 150k-row merge rolls back within the suspend deadline.
- The op's write phase is timed against its declared budget (U242 records it on the reference iPhone).

## H11. Reconciliation (KTD7, R1520)

Inside the commit op, after H10.4:

- **Undo.** The op reports its merge to U25 (`engine/undo` M9.4): S, and every staged target (each staged incoming row's key and each grave's target), whether or not H10.2 applied it. When there is at least one target, U25 drops every step up to S except R, drops R whole when a target is in the touched set of one of R's steps, drops a step made after S when a target is in its own touched set, and reports `ClearedBySync` when it dropped the oldest step. So a staged deck change that H10.2 did not apply because the local deck is newer still drops a held-back answer of that deck (scenario `sync/holdback/deck-change-not-applied`).
- Upload states were settled when `finish` was sent (H12).

After the op commits:

- **Change set.** The op's change set, source `SyncIncoming`, reaches every observer: U26 applies incremental derived updates (R1489), U43 refreshes its queues in the background and keeps the shown card and the still-valid pre-renders, matched by `(cardId, stateToken, renderKey)`. A change to the card on screen makes U43 emit `CurrentCardChanged`, and an answer still carrying the old `stateToken` is refused by U43's R727 check (AE27). Refreshed heads retain `sched/review-session#C17.1`'s terminal authority; a separate committed deck query cannot select the review phase.
- **Day clock.** The job runs `checkRollover()` once more (R597, `sched/day-clock` D8).
- The job's result is "no changes" (`normal-sync.md` N12), or "one-way sync needed" after H10.1's guard. The stage is deleted.

## H12. Failures, cancellation and crashes (R1532, R1554)

| When | Local collection | Undo | Server | Job ends with |
|---|---|---|---|---|
| before `start` was sent | unchanged (the preflight unbury aside) | flags of H4.1 cleared (`engine/undo` M9.3) | nothing sent | the failure's kind |
| after `start`, before `finish` was sent | unchanged; stage deleted | flags cleared: those steps are undoable again | `abort` sent, not awaited (R1532); the server keeps nothing | the failure's kind (`sanity_check_failed`: H13) |
| `finish` sent, its answer missing or an error | unchanged; stage deleted; no marking, no finalizing | `UploadPending` steps become `Uploaded` (M9.2): the server may have committed | `abort` sent, not awaited | `interrupted` |
| `finish` answered, commit cancelled, suspended or failed | rolled back to before the commit op | as above | committed | `interrupted` (or `disk_full`, `local_storage` for a local failure) |
| the app is killed between `finish` and the commit | as before the round, at next launch; the stage is swept | lost with the process | committed | (none) |

- After `finish` was sent, the next round sends again every row the server already holds, because nothing was marked. The server applies them idempotently, and sends again what it sent, which merges into equal rows, so the parties converge with no sanity failure (U118 tests the crash case).
- `cancelled` is the kind of a cancel before `finish` was sent; the end of background time (R1554) and `suspend` are `interrupted` (`errors.md`).

## H13. A failed sanity check (R1533, R2019)

On `bad` the job discards the stage, sends `abort` (H12), and then submits one op of source `Maintenance` that sets `col.scm` to `now_ms` and nothing else. The op writes `scm` as a raw `col` write (`storage/ids-usn-mtime` S3), so `col.mod`, `ls` and `usn` keep their values, as Anki leaves them (`normal-sync.md` N8, AE64). Like every `Maintenance` op it clears the undo history (`engine/undo` M2). The error `sanity_check_failed` carries the answer's client and server counts, which the job hands to U109 for the one-way choice (R2019). The next status check finds `scm` different from the server's and calls for a one-way sync.

## H14. Writes during the round (R1495, R1845)

- Every op made after S commits at once. Its rows stay out of this round (H5), keep `usn` -1 (H10.3) and go up with the next one; H10.4 makes the status show them.
- Its undo step is not flagged, so it can be undone during the round. The commit drops it only when a staged target is in its touched set (H11).
- An undo of a step flagged `UploadPending` is refused with `UploadPending` while the round runs, with `AlreadyUploaded` once `finish` was sent, and allowed again after a failure before `finish` (`engine/undo` M4, M9).
- An answer made during the round on a card the commit replaces is refused by its `stateToken` check if it reaches the writer after the commit; U43 redisplays the card (R728).

## H15. Session scope

- Hold-back applies only while a review session is active in U43's registry, since only then can the undo history outlive the round (KTD6). The session ends when its summary or congratulations screen is dismissed (R99); U43 then stops passing it, and the next round sends the newest answer too.
- Quit and foreground changes do not start Anki transport. Routine lifecycle sync is CloudKit's. If a previously authorized normal round continues during a session, H4 captures that session's run. Ending a session does not schedule another Anki round; a later explicit authorized round can send the newest answer.
- With no session, R is empty and V is the snapshot: the round is `normal-sync.md`'s, run off the writer.

## H16. Progress (R1535)

The stage and the sent and received counts of `normal-sync.md` N11, at most 10 per second, through the job's progress sink (`engine/jobs`). Counts come from what the round sent and staged, not from the commit.

## H17. Differences from Anki and the conformance deviations

Hold-back is a deliberate deviation (R726: Anki does not sync during a review session; R1495: Anki blocks its collection while it syncs). In an actor scenario the oracle plays the Meguri actor with Anki's client, which has no review session: it uploads every answer at once and empties its undo history at every normal sync. `review_session` (`specs/conformance/sync-ops.md` S13) changes nothing on the oracle. So each `sync/holdback/` scenario lists, in `conformance/deviations.yaml`, Meguri's value at each step where the parties see the held-back run:

| Deviation ids | Scenario | Meguri's values |
|---|---|---|
| `sync-holdback-newest-answer-*` | `sync/holdback/newest-answer` | the answer still undoable after the sync; status `normal_sync`; client_c sees 29 review rows and card #c30 new |
| `sync-holdback-run-with-flag-*` | `sync/holdback/run-with-flag` | the flag step remains undoable; client_c sees #c2's earlier row and deck totals of one new card and 3000 ms |
| `sync-holdback-incoming-newer-card-*` | `sync/holdback/incoming-newer-card` | the review row still pending after the first round, status `normal_sync`, client_c without it |
| `sync-holdback-deck-change-not-applied-names` | `sync/holdback/deck-change-not-applied` | client_c sees the server's rename until the next round |
| `sync-holdback-deleted-note-*` | `sync/holdback/deleted-note` | two graves still pending; client_c still sees the note and no review row |

Every other observation of these scenarios, the final states included, equals Anki's. Other differences from Anki that hold-back brings and that no scenario shows: the undo history is not emptied by a sync (KTD7, R1520), and a sync does not drop the study queues.

## H18. Tests

| Test | Where |
|---|---|
| The normal and conflict scenarios of `normal-sync.md` N14, first with hold-back disabled (no session), then enabled | T1 actor scenarios |
| AE9 core, a flag after the newest answer, a newer remote change to a held-back card, an incoming change that targets the run without being applied, a held-back deletion | `sync/holdback/` scenarios |
| A bury after the newest answer: R holds both; the deck sent counts the answers before R; `sanityCheck2` passes (H4.2). `sync/holdback/run-with-flag` shows the same with a flag, because the scenario `bury` op is still a draft | `normal_sync.rs`, scripted stub |
| An answer made during a round whose incoming changes touch other notes: after the commit the answer is undone first, then the held-back answer below it (H11, H14) | `normal_sync.rs`, scripted stub |
| U32 leg: `suspend(2 s)` with 5 queued answers during a round: answers committed, round cancelled with nothing applied, no open transaction, before the deadline | `normal_sync.rs`, scripted stub |
| AE68 first clause: the stub delays `chunk` 3 s while 3 answers are made: each fsynced within 1 s, absent from the upload; status pending afterwards (H10.4); the next round uploads them | stub |
| A note edited after its image was sent: the server holds the old one, the row stays `usn` -1, the next round sends the new one (H10.3) | stub |
| Undo of a pending step during the round refused with `UploadPending`; after success `AlreadyUploaded`; after a failure before `finish`, allowed (H14) | stub |
| The stub commits `finish` and drops the connection: a pending answer and a pending deck deletion refused with `AlreadyUploaded`; the next round converges with no sanity failure (H12) | stub, then the rig |
| An answer and a flag on one card in one second, the flag after S: the card stays `usn` -1; the next round sends the flag; a fresh client sees it (H10.3) | stub, then the rig |
| A held-back answer on a card whose before-image was pending in the same second: the card stays `usn` -1 (H10.3 (a)) | stub |
| The stub delays `chunk` while note N, which the server holds in a newer version, is deleted locally: N stays deleted with pending graves; the next round sends them; both sanity checks pass; a fresh client converges (H10.2 step 2) | stub, then the rig |
| A field added to note type NT and confirmed while `chunk` is delayed, with a remote note of NT staged: nothing applied, `scm` > `ls`, outcome one-way; the next round returns one-way without sending NT (H10.1) | stub |
| Snapshot barrier: 1,000 ops racing the barrier: each is in the snapshot with a seq at most S or outside it, and the `UploadPending` flags match exactly; a rollover during the `chunk` loop leaves a held-back card at `usn` -1 (H4.1, H10.3) | `normal_sync.rs` |
| A session start that sets `curDeck` during a round in which the server's config is newer: `curDeck` keeps the local value, stays pending and goes up next round (H10.2 step 4) | stub |
| Corruption flagged by a background quick check during the `applyChunk` loop: `abort` sent, no `finish`, collection untouched (H3) | stub |
| `suspend(2 s)` during a 150k-row merge: rollback, overlay answers committed, no open transaction, deadline met; the next round converges (H10.5) | `normal_sync.rs` |
| 150k incoming card updates while answering every 2 s: no answer lost, each durable within 1 s after the merge commits, write phase recorded against its budget (H10.5) | `normal_sync.rs`, U242 on device |
| AE27 core: an incoming change to the card on screen: `CurrentCardChanged`, and an answer with the old `stateToken` refused (H11) | engine test with U43 |
| Cancel during the `chunk` loop: collection byte-identical to before, `abort` observed by the stub, the next round completes (H12) | stub |
| `sanityCheck2` answered `bad`: stage discarded, `abort` sent, `scm` > `ls`, next status one-way, server counts kept (H13) | stub, and `sync/conflicts/sanity-bad` |
| The U118 hold-back property test never sees `sanityCheck2` fail | U118 |
