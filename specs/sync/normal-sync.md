# Normal sync

Spec: sync/normal-sync
Requirements: R1519, R1520 (unbury), R1521, R1522, R1523, R1524, R1525, R1526, R1527, R1528, R1529, R1530, R1531, R1532, R1533, R1535, R1554 (normal sync), R2012 (sync side), R2019 (`scm` on a failed sanity check), R2060, R2062, R2063; AE61, AE64; KTD26
Anki: 26.09.3
Scenarios: conformance/scenarios/sync/normal/, conformance/scenarios/sync/conflicts/

The incremental sync of one collection with the server, as Anki 26.09.3's client runs it once the status check (`meta-status.md` M3) has called for a normal sync: the order of the requests, which local rows go up, how the server's rows are merged into the collection, the sanity check, the closing stamps, and what a failure leaves. This file states the result Anki produces as if the whole round ran in one transaction on a collection nobody else writes to. Meguri runs it as a snapshot-and-stage job so that answers never wait (KTD6); `hold-back.md` says how that job reproduces this result and what it adds (the held-back answer run, writes made during the round). Message shapes are `wire-format.md`, transport and errors `transport.md` and `errors.md`, the status check `meta-status.md`. One-way sync is U109's, media sync U111's.

Stamping terms (`usn` -1, `now_s`, `now_ms`, `col.mod`) are those of `storage/ids-usn-mtime`.

## N1. Terms

- **Round**: one normal sync, from `start` to the end of the local commit.
- **Server USN**: the `usn` of this round's `meta` answer (`meta-status.md` M4). The client stamps with it every row it sends (N9.2), and the graves, tags and `config` rows it writes from the server's answer and the objects it renames (N4.3, N5.5, N5.7, N5.9). Objects and rows that arrive whole keep the `usn` they carry.
- **Pending**: a row whose `usn` is -1, in the tables `cards`, `notes`, `revlog`, `graves`, `decks`, `deck_config`, `notetypes` and `tags` (`storage/ids-usn-mtime` S2). `config` rows are never chosen by their `usn` (N5.1).
- **Outgoing view**: the collection state the client reads what it sends from. In this file it is the collection at the start of the round, with the round's own writes as they happen. In Meguri it is the snapshot's virtual state (`hold-back.md` H4).
- **Merged state**: the outgoing view after every change of N4 to N7 has been applied to it: what the collection holds when the round commits, before N9's stamps.
- **Local is newer**: local `col.mod` greater than the server's `mod` from `meta` (`meta-status.md` M3), the `lnewer` of `start`.

## N2. Before the exchange

When the status check calls for a normal sync, and before `start`:

1. The rollover check runs: when the scheduler day changed since the last unbury, buried cards are unburied (`sched/day-clock` D7, R1520). The unbury changes no card's `mod` or `usn` (R599). It is committed before the round reads anything, so the outgoing view holds the unburied cards and the `lastUnburied` value it wrote. Meguri runs `checkRollover()` at the start of every sync job, before the status check, whatever the check then finds (`sched/day-clock` D8); Anki runs its day query only once the check has called for a normal sync.
2. Anki then empties its undo history and study queues. Meguri does not: `hold-back.md` H11 and `engine/undo` M9 say what the round does to undo, and U43 refreshes queues in the background (KTD7). This is a deliberate difference (R1520 with R365, R457).
3. A round never starts on a collection the engine holds as corrupt (lifecycle `Failed{Corrupt}`, U132): the job fails before `meta` with `collection_corrupt` and sends nothing (R2012).

## N3. Requests of a round (R1519)

After `meta`, in this order, on one session key (`transport.md`):

| # | Request | Sent | Section |
|---|---|---|---|
| 1 | `start` | once | N4.1 |
| 2 | `applyGraves` | zero or more times | N4.2 |
| 3 | `applyChanges` | once | N5 |
| 4 | `chunk` | until an answer has `done` true | N6 |
| 5 | `applyChunk` | until a request with `done` true was sent | N7 |
| 6 | `sanityCheck2` | once | N8 |
| 7 | `finish` | once | N9 |

- No request of a later row is sent before every request of the earlier rows has its answer. Each request waits for the answer to the previous one.
- After a failure at any point from `start` on, the client sends `abort` (N10), and nothing else of the round.
- A job sends at most one round: it never sends `start` twice.

## N4. Deletions

### N4.1 `start` (R1521)

- The request carries `minUsn` = local `col.usn`, `lnewer` = local is newer (N1), and `graves` null (`wire-format.md` W4).
- The answer is the server's graves object: the cards, notes and decks deleted on the server since `minUsn`. The client keeps it and applies it after sending its own deletions (N4.3).

### N4.2 Local deletions (R1522)

- The client sends every pending grave of the outgoing view with `applyGraves`, at most 250 ids per request, cards first, then notes, then decks (`wire-format.md` W5.2). No pending grave: no `applyGraves` request.
- The graves it sent take the server USN (N9.2).

### N4.3 The server's deletions

After its own graves went, the client applies the server's, in this order: every note id, then every card id, then every deck id. For each id:

- The row with that id is removed, whatever it holds: a pending local edit does not save it (deletion wins over edits).
- A grave for it is added with the server USN, by the insert-or-ignore rule (`storage/graves` G2): a grave that already exists for that id and type, pending or not, stays as it is. So a grave the client sent in N4.2 for the same object keeps the server USN it got there.
- Only that row goes. A note's removal takes none of its cards (the server sends a card grave for each card it deleted), a deck's removal moves or removes none of its cards and none of its child decks. An id that matches no local row still gets its grave.
- Nothing else is written: no deleted-notes log line (`notes/note-records` D1), no `col.mod` change.

| Local row | Server grave | Result |
|---|---|---|
| note with a pending edit | note | note removed; grave with the server USN |
| card 2 of a note the server deleted, created locally after the server's deletion | note and card 1 | note and card 1 removed; card 2 stays, without its note (Check Database repairs it, U129) |
| deck with cards | deck | deck row removed; its cards stay where they are until their own graves arrive |
| none | card | grave added |
| pending grave for the same note | note | the local grave was sent and stamped in N4.2; it stays |

## N5. Note types, decks, presets, tags and config (`applyChanges`)

### N5.1 What the client sends (R1523)

One `applyChanges` request (`wire-format.md` W6), built from the outgoing view:

| Member | Content |
|---|---|
| `models` | every pending note type, as its schema-11 object |
| `decks` | every pending deck, then every pending preset |
| `tags` | the `tag` text of every pending `tags` row |
| `conf` | only when local is newer: every `config` row (`wire-format.md` W6.4) |
| `crt` | only when local is newer: `col.crt` |

- Each object carries the server USN as its `usn` (W6.2). Order within a list does not matter; Meguri sends ascending ids and ascending tag text.
- The note types, decks, presets and tags it sent take the server USN (N9.2). When it sent `conf`, every `config` row whose `usn` is not 0 takes `usn` 0 (`storage/config-table` C9); their `mtime_secs` stays.
- The answer is the server's changes object. The client applies it in this order: note types (N5.2), decks (N5.3), presets (N5.4), tags (N5.5), `crt` (N5.6), config (N5.7). The answer was built on the server before it applied the client's request, so an object both sides changed comes back in the server's version (N5.8).

### N5.2 Note types (R1524, R1525)

Each incoming note type, read per `model/schema11-json` S4 (input):

1. When the collection holds a note type with that id whose `mtime_secs` is greater than the incoming `mod`, the incoming one is dropped. The local one stays whether or not it is pending.
2. Otherwise, when the collection holds one with that id and its number of fields or of templates differs from the incoming one's, the round fails with `notetype_schema_changed` (parameter `notetype_id`), before anything of the answer is kept, and the caller offers a one-way sync (R1525). Equal counts pass, whatever else differs.
3. Otherwise the incoming one is accepted: its name is made unique (N5.9), and it is written under its id with the incoming `usn` and `mod` as its stamps (unless N5.9 renamed it): the `notetypes` row, and its `fields` and `templates` rows replaced by the incoming ones (`templates` rows with `mtime_secs` and `usn` 0, `storage/ids-usn-mtime` S1).

- Ties go to the server: equal `mtime_secs` and `mod` accept the incoming one.
- Accepting a note type changes no note and no card: no sort field or checksum is recomputed, no card is generated or removed. (A change of sort field, fields or templates on the other device is a schema change there, so it reaches this client by a one-way sync.)

### N5.3 Decks (R1524)

Each incoming deck, normal or filtered (`model/schema11-json` S7 to S9, its name through S11):

1. When the collection holds a deck with that id whose `mtime_secs` is greater than the incoming `mod`, the incoming one is dropped, pending or not.
2. Otherwise its name is made unique (N5.9) and the deck row is written under its id with the incoming `usn` and `mod` (unless N5.9 renamed it). A normal deck can replace a filtered one and the reverse.

- Ties go to the server.
- No other row changes: no parent deck is created for a name whose parent is missing, no card moves, no child deck is renamed. The default deck (id 1) is replaced like any other.

### N5.4 Presets (R1524)

Each incoming preset (`model/schema11-json` S10): dropped when the local preset with that id has a greater `mtime_secs` than the incoming `mod`; otherwise written under its id with the incoming `usn` and `mod`. Names need not be unique, so no preset is renamed. Ties go to the server.

### N5.5 Tags

Each incoming tag name is normalized and spelled as `tags/tags` T2 and T3 say, then:

- when a row equal to it exists (`tags/tags` T1), nothing changes: that row keeps its spelling, `usn`, `collapsed` and `config`;
- otherwise a row is added with `usn` = the server USN, `collapsed` 1, `config` NULL.

T3's deviation from Anki (an implicit parent's spelling) applies here as for any registration; no scenario shows it for sync.

### N5.6 `crt`

When the answer carries `crt`, `col.crt` takes it. The day clock computes afresh (`sched/day-clock` D8).

### N5.7 Config (R1526)

When the answer carries `conf` (the client sent `lnewer` false), the client's `config` table becomes the received set (`storage/config-table` C9):

- every local key absent from the set is removed;
- every received key is written with its value in compact form (`storage/config-table` C5), `usn` = the server USN, and `mtime_secs` = `now_s` at the merge, also when the value did not change.

No undo record and no grave is made for any of it.

### N5.8 Objects both sides changed

The server applies the client's objects by the same rules (N5.2 to N5.4, with the client's objects as incoming), before the client applies the answer. With local stamp L and server stamp S (`mtime_secs`):

| Stamps | Server ends with | Client ends with |
|---|---|---|
| L < S | its own | the server's |
| L > S | the client's | its own |
| L = S | the client's | the server's |

So equal stamps leave the two sides holding each other's version, until a later round in which one of them changes it again. Anki behaves this way, and Meguri reproduces it (scenario `sync/conflicts/object-mtime`). The same holds for a local object that is not pending but has the greater stamp: it is neither sent nor replaced, so the sides differ (same scenario).

### N5.9 Unique names (R1524)

Before an accepted note type or deck is written, its name is compared with the names of every other note type (or deck) in the collection, under the `unicase` collation (`storage/schema-18` S4), a deck by its full name. While another row with a different id holds an equal name, `+` is appended to the incoming name (to the end of a deck's full name, so to its last component: `Words::Kanji` becomes `Words::Kanji+`), and the object is stamped `usn` = the server USN, `mtime_secs` = `now_s`. Because it is not pending, it is not sent back: the server keeps the unsuffixed name.

| Local rows | Incoming | Stored |
|---|---|---|
| deck 7 `Gamma` | deck 5 `Gamma` | deck 5 `Gamma+`, stamped |
| deck 7 `gamma`, deck 8 `Gamma+` | deck 5 `Gamma` | deck 5 `Gamma++` |
| deck 5 `Alpha` | deck 5 `Gamma` | deck 5 `Gamma`, with the incoming stamps |
| note type 3 `Shared` | note type 2 `Shared` | note type 2 `Shared+`, stamped |

When two clients give two different objects the same name and sync one after the other, the server renames the second one it receives (its own N5.9) while that client renames the first one: both sides end with the two names swapped (scenario `sync/conflicts/name-collisions`).

## N6. Rows from the server (`chunk`)

The client asks `chunk` until an answer has `done` true (`wire-format.md` W7) and applies each answer before asking for the next: its review rows first, then its cards, then its notes. A row whose decoding fails fails the round (`errors.md` E6).

### N6.1 Review rows (R1527)

A review row is inserted when no `revlog` row has its id, with every value as received (its `usn` the tuple's, W10). When one exists, nothing changes, whatever it holds (`storage/ids-usn-mtime` I4): two devices that wrote a review at the same millisecond keep one row each under one id, and each side keeps its own (scenario `sync/conflicts/revlog-id-taken`).

### N6.2 Cards (R1527)

An incoming card replaces the local card with its id, or is inserted when there is none, unless the local card is pending and its `mod` is equal to or greater than the incoming `mod`. In that case the local card stays and goes up in N7.

- The stored row is the tuple's columns (`wire-format.md` W8): its `usn` the tuple's, its `mod` the incoming one; `data` is the incoming text parsed per `model/card-data-json` C3 and written per C4.
- Nothing is checked against other rows: a card whose note or deck the collection lacks is stored as it is.

| Local card | Local `mod` vs incoming | Result |
|---|---|---|
| pending | greater | local kept, sent in N7 |
| pending | equal | local kept, sent in N7 (AE61: local wins ties) |
| pending | smaller | replaced, not sent |
| not pending | greater | replaced |
| absent | | inserted |

### N6.3 Notes (R1527, R1528)

The same rule as N6.2 decides whether an incoming note replaces the local note. An accepted note is stored as:

| Column | Value |
|---|---|
| `id`, `guid`, `mid`, `mod`, `usn` | the tuple's (`wire-format.md` W9) |
| `tags` | the tuple's tags split as W9.1 says, joined in `model/row-records` R3's form; not normalized and not registered (tags arrive in N5.5) |
| `flds` | the tuple's fields, split at U+001F, each with `text/checksum-and-sort-field` C1 step 1 applied (control characters other than TAB and LF removed) and never NFC, whatever `normalize_note_text` holds |
| `sfld`, `csum` | computed from those fields and the note type the collection holds after N5 (C2 to C4) |
| `flags`, `data` | 0 and the empty string |

- When the collection has no note type with the note's `mid` after N5, the round fails with `missing_notetype` (parameters `notetype_id`, `note_id`) (R1528).
- When the number of fields differs from that note type's field count (at least 1), the round fails with `note_fields_mismatch` (parameters `notetype_id`, `note_id`).
- A note that the rule keeps local is not checked.
- No card is generated or removed, and nothing goes to the deleted-notes log.

### N6.4 Rows both sides changed

The server applies the client's cards and notes from `applyChunk` by the same rule, counting its own row as pending when its `usn` is at or above the client's `minUsn` (the row changed since the client last synced). Its own rows went out in its `chunk` answers before that. With local `mod` L, server `mod` S, both rows changed:

| Stamps | Server ends with | Client ends with |
|---|---|---|
| L < S | its own | the server's |
| L > S | the client's | its own |
| L = S | its own | its own |

So equal stamps leave each side with its own version, until a later change of that row on either side (scenario `sync/conflicts/tie-local-wins`). AE61 expects the server to end with the client's version on a tie; Anki's server keeps its own, and Meguri, which does not control the server, matches Anki: it keeps its local row and sends it.

### N6.5 Repeated row IDs

Repeated IDs in a chunk's card, note or review array are supported. Apply
each array in its received order, including repeated occurrences; finish
that chunk before the next one. An occurrence sees the result of earlier
occurrences of that ID, also in earlier chunks. IDs in different tables
remain independent. This is not a duplicate JSON-member rule.

For cards and notes, use N6.2/N6.3 against that current row each time. An
accepted tuple keeps its supplied `usn`, so it may change whether the next
occurrence faces a pending row. Do not compare all occurrences against the
row at the start of the chunk, choose the largest `mod`, or choose the last
tuple before applying the acceptance rule. For example, all incoming tuples
below have a nonpending USN:

| Row before | Incoming `mod`, in order | Final row |
|---|---|---|
| absent | 30, 10 | second incoming row, `mod` 10 |
| nonpending, `mod` 40 | 30, 10 | second incoming row, `mod` 10 |
| pending, `mod` 20 | 10, 20 | original pending row |
| pending, `mod` 20 | 30, 10 | second incoming row, `mod` 10: the first accepted row was no longer pending |
| pending, `mod` 20 | 10, 30 | second incoming row, `mod` 30 |

The result is identical if the two tuples arrive in separate chunks. If a
tuple carries a USN that is pending under the receiver's existing rule, later
occurrences must respect that pending state too; this adds no USN restriction.
On the server, N6.4's pending-USN test still applies.

For review rows, an existing row always stays. If initially absent, the first
occurrence inserts its complete row and every later occurrence is ignored,
even if its columns differ. Never assign another ID or use the last tuple.

Normal decode and validation failures remain failures. In particular, an
accepted note that fails N6.3 still fails the round even if a later occurrence
would replace it. Notes kept by the pending-local rule still follow N6.3's
no-check rule. Row compaction cannot turn an accepted-note failure into success.

Bounded per-chunk reduction may pass one effective row per table/key to a
set-based storage merge, provided it produces these ordered results. The
scratch merged view, outgoing pending rows and sanity counts must agree
with them. H6/H10 retain separate acceptance-dependent validation data for
the live path; the row-choice summary alone does not preserve earlier errors.

## N7. Rows to the server (`applyChunk`, R1529)

- The client collects the pending review rows, cards and notes of the outgoing view after N6, so a local row that N6 replaced is not sent, and one that N6 kept is.
- It sends them in chunks of at most 250 objects, taken in turns as `wire-format.md` W7.2 says, `done` true exactly on the last chunk, which may be empty. Each object carries the server USN as its `usn` (W8 to W10).
- The rows it sent take the server USN (N9.2).

## N8. Sanity check (R1530, R1533)

1. Before `sanityCheck2`, no row of the merged state in `cards`, `notes`, `revlog`, `graves`, `decks`, `deck_config`, `notetypes` or `tags` may be pending once the stamps of N9.2 are counted: every pending row of the outgoing view was sent. A pending row left over is a bug; the round fails with `internal` and sends `abort`. (Rows that Meguri keeps pending on purpose are outside the outgoing view, `hold-back.md` H9.)
2. The client sends the counts tuple of the merged state (`wire-format.md` W12.1): due counts `[0,0,0]`, then the row counts of `cards`, `notes`, `revlog`, `graves`, `notetypes`, `decks` and `deck_config`.
3. `ok`: the round goes on with N9.
4. `bad`: the round fails with `sanity_check_failed`, carrying the answer's `c` and `s` counts when they decode. The client undoes the round and sends `abort` (N10), then sets `col.scm` to `now_ms`, leaving `mod`, `ls` and `usn` as they were before the round. Since `scm` now differs from the server's, the next status check calls for a one-way sync (R1533, AE64). The server's counts go to the one-way choice (R2019, U109).

The official server compares the counts with its own after setting the due counts and the graves count to 0 on both sides (W12.2), so only the six table counts must agree.

## N9. `finish` and the end of the round (R1531)

### N9.1 `finish`

The client sends `finish`. Its answer T is the server's new modification time (`wire-format.md` W13). The server has committed the round when it sends T.

### N9.2 Stamps

The round's local result is the merged state with:

- every row and grave the client sent in N4.2, N5.1 and N7 stamped `usn` = the server USN, its modification time unchanged (`storage/ids-usn-mtime` S3); `config` rows as N5.1 says when it sent `conf`;
- `col.ls` = T, `col.mod` = T, `col.usn` = the server USN + 1.

The whole result is committed as one transaction (R1519): either all of N4 to N9 is in the collection, or none of it.

## N10. Failures and cancellation (R1532, R1554)

- Any failure from `start` on (a refused or failed request, an answer that does not decode, a refusal of N5.2, N6.3 or N8, a local error, the job's cancel token, the end of background time) ends the round:
  - nothing of N4 to N9 is kept: the collection holds exactly what it held when the round began (the unbury of N2 and N8's `scm` aside);
  - the client sends `abort` without waiting for or reading its answer (`wire-format.md` W13); the job's error is the original failure, never the outcome of `abort` (`errors.md` E7).
- The official server discards the round when it receives `abort`, when one of the round's requests fails on its side (it answers 400 and drops the round at once), or, when `abort` is lost, when the next `start` begins another round. So it keeps nothing of a round that did not reach `finish`.
- When the failure comes after `finish` was sent (no answer, an error answer, a local failure while committing), the server may already have committed the round. The client still keeps nothing. The next round sends the same rows again; the server's application of them is idempotent (graves are insert-or-ignore, objects and rows replace equal content, review rows are inserted only when absent), and the server sends again what it sent, which the client applies by the same rules. So the two sides converge without a one-way sync. `errors.md` gives the kind (`interrupted`, `hold-back.md` H12).
- A cancel or the end of background time is a failure like any other (R1554): the round is undone and `abort` is sent.

## N11. Progress (R1535)

The job reports a stage and two counts, at most 10 times per second:

| Stage | From |
|---|---|
| connecting | the job's start, through `meta` |
| syncing | `start` |
| finalising | `sanityCheck2` |

- **Sent**: the ids in every `applyGraves` request, plus the note types, decks, presets and tags in the `applyChanges` request, plus the objects of every `applyChunk` request.
- **Received**: the ids of the server's graves, plus the note types, decks, presets and tags of the `applyChanges` answer, plus the objects of every `chunk` answer.
- `conf` and `crt` are not counted. Counts only grow during a job.

## N12. Outcome

A round that commits reports "no changes" (`specs/conformance/sync-ops.md` S4.1 `no_changes`): the protocol round completed. The status remembers it (`meta-status.md` M6.1), subject to `hold-back.md` H10.4's retained pending changes. N5.8 and N6.4 retain their specified tie outcomes. The server's media USN from `meta` is available to the same authorized operation's media phase (R1534, U111); N15 governs admission.

## N13. Errors of a round

| Failure | Kind (`errors.md`) | Next status check |
|---|---|---|
| lifecycle `Failed{Corrupt}` before or during the round (N2, `hold-back.md` H3) | `collection_corrupt` | not run until the collection is restored or repaired |
| an accepted note type with other field or template counts (N5.2) | `notetype_schema_changed` | normal again; the app offers a one-way sync (R1525) |
| a note of a note type the collection lacks (N6.3) | `missing_notetype` | normal again, and fails the same way until repaired |
| a note whose field count differs from its note type's (N6.3) | `note_fields_mismatch` | the same |
| a pending row left after sending (N8 step 1) | `internal` | normal again |
| sanity check `bad` (N8) | `sanity_check_failed` | one-way (`scm` set) |
| HTTP 400 on a request of the round (the server could not apply it) | `db_check_required`, origin `server` | normal again, and fails the same way until repaired |
| any transport, status or decode failure | as `errors.md` E4 to E6 | normal again |

## N14. Scenarios

Actor scenarios (`specs/conformance/sync-ops.md`), an Anki client running the setup and uploading first so that every actor holds Anki's stock note types. They stay `pending` until Meguri's executor syncs (sync-ops.md S10.2).

| Scenario | Shows | Golden |
|---|---|---|
| `sync/normal/two-way` | client_a's notes and reviews reach the Meguri actor, whose own notes, review and edit reach client_a; all converge (N4 to N9) | recorded |
| `sync/normal/config-whole` | the config travels whole from the newer side, with its stamps on each side, and `crt` with it (N5.1, N5.6, N5.7) | recorded |
| `sync/normal/graves` | deletions in both directions, with the receiver's grave stamps (N4) | recorded |
| `sync/normal/many-objects` | 260 pending graves and 602 objects: two `applyGraves` and three `applyChunk` requests, all accepted, counts equal (N4.2, N7, N8) | recorded |
| `sync/normal/unbury-before-sync` | a card buried on an earlier day is restored before the exchange, with its `mod` and `usn` (N2) | recorded |
| `sync/conflicts/tie-local-wins` | AE61: a pending card and an incoming one with the same `mod`; the client keeps and sends its own, the server keeps its own (N6.2, N6.4) | recorded |
| `sync/conflicts/newer-wins` | notes edited on both sides: the newer edit wins on every party; a newer card that is not pending is replaced (N6.2, N6.3) | recorded |
| `sync/conflicts/deletion-vs-edit` | a server deletion beats a pending local edit, and a local deletion beats a server edit (N4.3, N7) | recorded |
| `sync/conflicts/object-mtime` | deck, preset and note type rules: older replaced, newer kept, ties swapped, a newer object that is not pending kept (N5.2 to N5.4, N5.8) | recorded |
| `sync/conflicts/name-collisions` | `+` suffixes on decks and note types, and the swapped names (N5.9) | recorded |
| `sync/conflicts/revlog-id-taken` | two reviews with one id: each side keeps its own (N6.1) | recorded |
| `sync/conflicts/sanity-bad` | AE64: a `bad` sanity answer, the round undone on both sides, `scm` set, the next status one-way (N8, N10) | recorded |
| `sync/conflicts/server-refuses` | a note whose note type the server lacks: HTTP 400, the round undone on both sides, `scm` unchanged, the next round fails the same way (N10, N13) | recorded |

What the official server cannot be made to send is tested in `meguri-sync` against the scripted stub (`transport.md` T13), from this file's rules: an incoming note type with other counts (N5.2 step 2), a note of an unknown note type or with a wrong field count (N6.3), the request order and the abort after each failure point (N3, N10), a lost `finish` answer (N10), progress (N11), and the split of 251 graves into 250 and 1 (N4.2).

U29's deferred regression belongs to the real normal job in `normal_sync.rs`.
After a successful baseline round, save a field option or rename a field through
the production note-type service. Run the real normal job again. Capture the
whole note type in `applyChanges`, including the edit, and verify it on a fresh
peer. Both status checks remain normal: matching `scm`, no one-way request.
A pending-row helper or a stand-in USN update does not satisfy this check.

## N15. Explicit transfer scope (KTD26)

This component serves explicitly authorized Settings Anki transfers. Routine
sync uses CloudKit. Launch, foreground, background, quit, review completion,
timers, Sync Now and integrations never start this component's server requests.
An already authorized operation may continue within existing lifecycle limits.

A normal round reconciles both sides. It must not obtain a directional preview
or silently make Import publish local changes or Export apply remote changes.
Any entry that needs this reconciliation must disclose its local and remote
effects and obtain the separate decision required by `sync/anki-transfers#M2`.
Current Import and Export still refuse active reviews under M2.
Core hold-back coverage does not change that admission rule.

The caller retains endpoint, account, direction, scope and collection-generation
admission. Replacement cancels and drains the old job through existing lifecycle
rules; it must not retarget its snapshot, stage or held-back run.
Media work requires the same operation's disclosed authority and selected scope.
There is no independent automatic media follow-up.

`sync/anki-transfers#M6` owns eligibility and durable receipts. Custom URL
servers have no successful-transfer wait, including retained earlier deadlines.
Historical records remain intact; active, partial and uncertain operations still
exclude overlap. The other server paths retain their existing cooldown rule.
Server USNs, stamps and round outcomes never become CloudKit revisions or tokens.
