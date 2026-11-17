# Migaku Recently received log, moves and legacy `/create`

Spec: servers/migaku-received-log
Requirements: R1721, R1722, R1723, R1724, R1725, R1726, R1727, R1728 (core side of each), R1731, R1732, R1733; R1702 (payload retention), R1719 (manual moves feed routing); supports R1709, R1859; AE73 (Retry), AE74
Anki: 26.09.3 (note adds, deletes, deck changes and undo, through the specs cited)
Scenarios: `conformance/recordings/servers/migaku/create/` (C5; declared, `servers/transcripts` T13) holds 11 of C5's 12 rows. `filtered-deck`, and every row of L17 (planned for `conformance/recordings/servers/migaku/moves/`), wait for transcript steps for filtered decks, Migaku profiles and the L16 calls. Until then they are listed as tables.

The core side of the Mac sidebar's "Received" page. It is a durable local log of every card the Migaku server received or failed to receive, with the operations behind Confirm, Move, Retry, Delete note, Open in Browse and Edit. The same file covers the legacy `POST /create` route. The Swift page, its toast and its menus are U145's.

Interfaces: the Migaku browser extension 1.30.16 and 1.31.0.17 for `/create`, which is legacy (R1731; the plan treats `/receive` as the live route), and the Migaku Anki add-on 0.6.6 as the reference for its form fields and replies. The log itself has no external interface.

`servers/migaku-receive` writes the rows (P20). Routing state, profiles and field maps are `servers/migaku-routing`. Bracket stripping is `servers/migaku-syntax`.

## L1. Storage

- The log lives in the integrations store (`meguri-integrations.db`, `servers/host-security` H10.1), beside the collection and never inside it (R1665). Its tables come with the store's numbered migrations (H10.1): U139 adds the received-log table, the receive index, the intents and the pending queue, and U140 adds the move-history table. Payload files live in the folder `meguri-integrations/`. Sync and `.apkg` export never carry them.
- Writes to the log are integrations-store transactions. A collection op and a log write are never one transaction. `servers/migaku-receive` P17 keeps them consistent for receives, and L8 rebuilds a row's status from the collection.
- The log survives restarts. A collection replacement keeps the log and runs L8's full reconcile.

## L2. Row contents

| Part | Content |
|---|---|
| row id | integer, increasing in insertion order; never reused |
| endpoint | `receive` or `create`: the route that wrote the row |
| receive time | the arrival instant of the first attempt, in milliseconds (`servers/migaku-receive` P1). A Retry never changes it (L14) |
| target word | `targetWordNoSyntax` (`servers/migaku-syntax` S5), then `text/strip-html` H4, white space trimmed. For `/create`, C2.10 says which field gives it |
| sentence snippet | the same applied to `sentence`, cut to its first 120 Unicode scalar values, with `…` appended when cut. For `/create`, C2.10 |
| media names | the stored names, by source: sentence audio, word audio, images (empty for a row that never reached Created) |
| profile | profile id and name as they were at the last receive, retry or move; empty when there was none |
| language, decision | the profile's language and the decision kind (L4) |
| routing route, detection | the routing route (`detected`, `recent`, `fallback_profile` or `override`) and the detection (the detected language or none, and `confident` or `uncertain`) of the last routing decision (`servers/migaku-routing` D13.1); empty when routing did not run (a `/create` row, a row that failed before routing). A move leaves them as they were |
| hidden | true while the row belongs to an open intent of a live request (`servers/migaku-receive` P17); such a row is invisible (L6) |
| original note id | the note the receive created; empty before Created |
| deck id, note type id | of the last receive or move; the deck is the one the cards were sent to |
| status | L3 |
| warnings | any of Possible duplicate, Front would be empty, Media missing (`servers/migaku-receive` P21) |
| error | the message of a Failed row (`servers/migaku-receive` P19) |
| confirmed | false when written; L9 |
| retry key | the retry key of `servers/migaku-receive` P1 (raw item ids, whether there were items, the `targetWord` and `sentence` pair); none for `/create` |
| failed instant | the instant the row last turned Failed; empty otherwise |
| payload | a reference to the stored raw body of a Failed or Pending row, and the instant it is pruned (L5) |
| move history | L13 |

## L3. Statuses

`Pending` and `Failed` are written by `servers/migaku-receive` and by Retry (L14). Once a row has a note, its status is derived from the collection and its move history:

1. The **live note** is the to note of the newest move entry in effect (L13). With no entry in effect, it is the original note.
2. The live note does not exist: **Deleted**.
3. At least one move entry is in effect: **Moved**.
4. Otherwise: **Created**.

| Event | Status after |
|---|---|
| receive commits | Created |
| receive waits in the pending queue (switch on) | Pending |
| receive or Retry fails | Failed |
| a move commits | Moved |
| that move is undone (no other entry in effect) | Created |
| it is redone | Moved |
| the live note is deleted anywhere (Browse, review, AnkiConnect, a sync) | Deleted |
| that delete is undone | Created or Moved, by rules 1 to 4 |

## L4. Decision kinds

| Kind | Written when |
|---|---|
| Auto, Fallback, Override | routing's decision for a receive or a Retry (`servers/migaku-routing` D11). Every card that D11 step 4 routes is `Fallback`: detection was uncertain, or it was confident for a language that no profile has. Under an active override the decision is `Override`, whatever the detection |
| Manual | a move, or its undo or redo, changed the row (L13); and every `/create` row, whose destination the sender chose (C2, Q2) |

A row that failed before routing ran has no decision.

**Labels.** R1722 shows how the language was decided with one of five labels. U145 takes the label from the row's decision and the detection's language (L2). A `Fallback` row always has the confidence `uncertain` (`servers/migaku-routing` D11 step 4, D13.1), so the language tells the two kinds of `Fallback` apart:

| Decision | Detected language | Label |
|---|---|---|
| Auto | any | Auto |
| Fallback | none (detection itself was uncertain) | Uncertain |
| Fallback | a language (one that no profile has) | Fallback |
| Override | any | Override |
| Manual | any | Manual |
| none | — | no label |

This reading of R1722's Uncertain and Fallback is Meguri's (L18 Q7).

## L5. Retention and pruning

- A row is kept when it is among the newest 500 rows by receive time, or when its receive time is within the last 30 days (R1721). Pending rows are always kept. A row that neither rule keeps is removed with its move history.
- A Failed row's payload is deleted 7 days (604,800 s) after its failed instant (L2), the instant the row turned Failed (R1702). For a live request that is its arrival plus the time it took to fail. For a pending entry that failed when it drained, it is the drain, so the payload is kept for Retry for 7 days from then. A Retry that fails again does not move the instant (L14). The row stays after the deletion, and Retry is no longer available for it.
- Pending payloads are never deleted by pruning. They go when their entry commits; when it fails, the raw body becomes the Failed row's payload (`servers/migaku-receive` P15).
- Hidden rows (L2) are never pruned; P17 recovery resolves them.
- The prune pass runs at launch, after the pending queue has drained (`servers/migaku-receive` P15), and after a row is added, at most once per hour.

Examples: 600 rows received within the last 10 days are all kept. 520 rows all older than 30 days are trimmed to the newest 500.

## L6. Reading the log (R1859)

- Rows are read in pages, newest first: by receive time descending, then row id descending. A page holds at most 200 rows. The cursor is the last row's (receive time, row id).
- A read of one row by id, and the badge count (L9), are single calls.
- Hidden rows (L2) are left out of pages, single-row reads, the badge and every call of L16 until they stop being hidden.
- A host event "received log changed" (`servers/host-security` H11.1, no payload) follows every committed log write that a read can see, so the page refreshes without polling.

## L7. Superseded Failed rows

When a receive C reaches Created, every other Failed row F that still holds its payload, and whose retry key C's key matches by `servers/migaku-receive` P12's key match (C in the place of the request, F in the place of the earlier receive), is removed from the log with its payload. P12's other conditions, the 600 s window and an existing note, do not apply here. The card exists, so the failure no longer needs attention. A client retry after a network error therefore leaves one Created row and no Failed row.

## L8. Reconciliation with the collection

- While the Migaku server runs, the log follows the change stream (`engine/change-stream` S3), attached when the server starts (`servers/host-security`). Notes removed or restored and the undo and redo of move steps update the rows they concern, by L3 and L13.
- At server start and after every `CollectionReplaced`, a full reconcile checks the live note of every row against the collection. Deletes made while Migaku was off, or by a sync or a restore, are caught then.
- Reconciliation never writes to the collection.

## L9. Badge and Confirm (R1723)

- The badge counts the kept, visible rows that are not confirmed and whose decision is Fallback (labels Uncertain and Fallback, L4), or whose status is Failed. A Fallback row whose detection found a language counts too: routing sent it to a profile that does not have its language, which needs the same attention as an uncertain one.
- Confirm on one or more rows sets their confirmed flag. Confirming twice changes nothing. Confirm never changes status or decision.
- A move sets the decision to Manual, so a moved row leaves the badge whether or not it was confirmed. A successful Retry of a Failed row leaves the badge unless its new decision is Fallback.

Example: one unconfirmed Uncertain row, one unconfirmed Fallback row and one Failed row give 3. Confirming the Fallback row gives 2.

## L10. Moves: eligibility (R1724, R1726)

A move takes one or more rows and a destination profile P (note type Tp, deck Dp). For each row, the first matching line of this table gives its eligibility:

| Condition | Eligibility | Reason |
|---|---|---|
| status Pending or Failed | cannot move | `This card was not added.` |
| status Deleted | cannot move | `The note no longer exists.` |
| P's note type is missing, or its deck is missing or not usable (a filtered deck, `notes/deck-placement-and-position`) | cannot move | the R1715 message of `servers/migaku-receive` P19 |
| the live note's note type is Tp and its cards are all in Dp | cannot move | `The card is already in this note type and deck.` |
| the live note's note type is Tp | deck move | none |
| some card of the live note has a review-log row | deck move only | `This card has review history, so only its deck can change.` |
| otherwise | full move | none |

Any review-log row counts, a manual one included. The eligibility call returns this table's result per row without changing anything. U145 offers "Move deck only to <Dp>" with the reason when the result is "deck move only".

## L11. Moves: deck change

A deck move, and a "deck move only" the caller accepts, change the deck of every card of the live note to Dp. They run as the Change Deck operation (`browser/bulk-actions` B7.2) inside the move op: a card in a filtered deck goes home first (B7.2 step 3.2), and remaining steps and FSRS memory states follow the target's preset (B7.2 steps 3.1 and 4, R645). The note keeps its id, note type, fields and tags. The cards are recorded in the move op's one step (L13), not in a "Change Deck" step of their own. A filtered Dp never gets here (L10).

## L12. Moves: another note type (R1725)

A full move creates a note N' of note type Tp in deck Dp from the live note N of note type Ts, then deletes N. It never changes a note type in place, so it changes no schema and needs no one-way sync.

Let Ps be the row's current profile, which gives Ts's field plan and defaults. A default of the empty string means "no default".

**Field values.** For each field F of Tp, in field order:

1. **By name**: when Ts has a field with exactly F's name, take its current value from N.
2. **By source key**: otherwise, when P's plan gives F a source key k (not `none` and not `default_only`), and Ps's plan gives some field of Ts the same key, take the value of the first such field in Ts's field order.
3. Otherwise F has no source, and its value is the empty string.

**Defaults.** Then, for each F of Tp:

- When F got its value from a field G of Ts, and that value equals G's default in Ps (both empty counts as equal), the value becomes F's default in P, or the empty string when P has none.
- When F had no source, the value becomes F's default in P.
- Otherwise the value stays as copied. A field the user edited away from its default keeps the edit.

When the row has no current profile (it was deleted), only rule 1 applies, and the default rule applies only to fields with no source.

**The rest:**

- Tags: N's tags, unchanged. P's tags to add are not applied.
- Media: the field values keep their media references. No file is copied, renamed or removed.
- N' gets a new id and a new GUID. Its cards are generated and placed as for any add (`notes/card-generation`, `notes/deck-placement-and-position`), as new cards. Card flags, suspension and burying of N's cards are not carried over: N has no review history (L10), so its scheduling state is new anyway.
- N is deleted as `notes/note-records` D1 deletes a note: graves for its cards and itself, and a line in the deleted-notes log.
- `update_add_defaults` is false for N' (`notes/note-records` A4).
- The result names every field of Ts whose non-empty value did not reach N', so U145 can say what the move dropped.

Example (AE74). Ts is the Spanish Custom type with defaults Is Vocabulary Card `x` and Is Audio Card `x`. Tp is the German Custom type with the same field names and defaults. A received, unreviewed note is moved. Every field is copied by name. Both flags equal the Spanish defaults, so they take the German ones (`x`, `x`). From a Japanese Custom profile (Is Vocabulary Card no default, Is Audio Card `x`), the same move gives Is Vocabulary Card `x` (empty equals "no default", so it takes the German default) and Is Audio Card `x`. If the user had cleared Is Audio Card on the Spanish note, it stays empty, since the empty string differs from the Spanish default `x`.

## L13. Moves: one op, undo and redo (R1727)

- One move call on any number of rows is one op with source `UserUndoable` and one undo step (`engine/op-model` O1, `engine/undo` M1). Within it, rows are handled in the order given: deck moves through L11 and full moves through L12's add and delete. Rows that cannot move, and "deck move only" rows when the caller did not accept them, are skipped and reported. When no row moves, the op writes nothing and records no step.
- **Step name and catalog.** The step is named `Move Received Card`, whatever the number of rows, as Anki's step names do not change with the count ("Delete Note", "Change Deck"). It is a Meguri step with no Anki counterpart; `conformance/undo-ops` V4 names it as the pending Meguri-only step `move_received_card`, which U140 adds to V4's table together with the engine's step name, as `engine/undo` M1 allows only V4 names (L18 Q4). The op's catalog entry (`engine/op-model` O9, added by U140 with the op) is `migaku_move`, undo class undoable, no long write. Delete note (L15) uses the existing "Delete Note" step.
- The call returns, per row, one of: full move (with the new note id and the dropped fields of L12), deck move, or skipped with the L10 reason.
- Each moved row gets a move-history entry: the move op's seq, from note and to note (equal for a deck move), from profile and to profile, to deck, and for a deck move each card's deck before the move (card id and deck id). A new entry is in effect.

**In effect.** No event names the step an undo or redo restored (`engine/change-stream` S3 publishes only change sets; `engine/undo` M4 names the undone step to the caller alone, and each undo and redo records a step with a new id). So whether an entry is in effect is a stored flag that the subscriber of L8 changes only when the collection shows the pattern an undo or a redo leaves:

| Entry | Undo pattern: the flag turns off | Redo pattern: the flag turns on |
|---|---|---|
| full move | the to note does not exist and the from note exists | the to note exists and the from note does not exist |
| deck move | every card listed in the entry exists and is in the deck it was in before the move | every card listed in the entry exists and is in the to deck |

- After every `Changes` event that lists a note or a card of a row with move history, the subscriber checks that row's entries, newest first, and applies the table. A change that shows neither pattern leaves the flag as it is: a later deck change in Browse, or deleting the to note in Browse, is not an undo. Deleting the to note makes the row Deleted by L3 instead.
- An undo restores N under its id and removes N'; a redo restores N' under its id (`engine/undo` M4, M5). So after the flag changes, the row's live note, deck, note type and status follow (L3).
- The row's profile, note type and deck follow the newest entry in effect, or the receive's values with none.
- Every committed move, and every flag change of this table, sets the decision of each row concerned to Manual. It also calls routing's `record_manual_move` with the profile the card now belongs to (P after a move or a redo pattern, the from profile after an undo pattern) and the instant: the move's commit, or the instant the subscriber handles the event (from the injectable clock) for an undo or redo (`servers/migaku-routing` D12, R1719). An uncertain card received within 60 minutes then follows the user's correction.
- The full reconcile (L8) applies the same table to every row with move history.

## L14. Retry (R1728, AE73)

- **Failed row, payload kept**: the stored payload goes through `servers/migaku-receive` from stage 1, as a new arrival at the Retry instant: its timers, its place in the server's FIFO, its 5 s busy wait and, when it creates a note, its receive-index entry use that instant. It gets both retry checks and a new routing decision. No HTTP reply is sent anywhere. The same row is updated and no row is added. Its receive time stays the first arrival (L2), so its place in the pages and its retention do not change:

  | Outcome | Row | Call result |
  |---|---|---|
  | created | Created, with note id, decision, route, detection, warnings and media names; payload deleted | `created{note_id}` |
  | matched an earlier receive (P12) | removed, with its payload | `matched{note_id}`, the earlier receive's live note |
  | failed | stays Failed with the new error; payload and failed instant kept (L5) | `failed{message}` |
  | busy, switch off | stays Failed with `Meguri is busy` | `failed{message}` |
  | busy, switch on | Pending, with a pending entry | `pending` |

- **Pending row**: Retry drains that entry now if the collection is available, as `servers/migaku-receive` P15 commits it, and returns `created{note_id}` or `failed{message}`. Otherwise it does nothing and returns `pending`, and the entry still commits on the next `Available` event. It never submits the payload a second time, so it makes one note.
- **Failed row without payload** (pruned): Retry is unavailable and returns `retry_unavailable`.
- **Created, Moved or Deleted row**: Retry is not offered; the call returns `retry_unavailable`.

## L15. Delete note, Open in Browse, Edit (R1728)

- **Delete note**: deletes the row's live note as `notes/note-records` D1 does, as a `UserUndoable` op with the step "Delete Note". Reconciliation then marks the row Deleted, and undo returns it to Created or Moved (L3).
- **Open in Browse**: the core returns the live note id. U145 and U146 show Browse with the search `nid:<id>`.
- **Edit**: the core returns the live note id for the editor panel (U190).

## L16. Calls

The FFI exports U145 uses (U140, `core/crates/meguri-ffi/src/servers.rs`). The routing override (R1720) is `servers/migaku-routing`'s. Each call does one bounded piece of work, never one call per visible row. Calls marked "waits" are async for Swift and return when their work ends; the others return at once. A row id that names no visible row gives `not_found`.

| Call | Input | Result | Waits |
|---|---|---|---|
| page | cursor (none for the first page), limit up to 200 | rows of L2 (without payload bytes and retry key) and the next cursor, or none | no |
| row | row id | one row of L2, or `not_found` | no |
| badge count | none | integer (L9) | no |
| confirm | row ids | none; unknown ids are ignored | no |
| move eligibility | row ids, destination profile id | per row: `full_move`, `deck_move`, `deck_move_only{reason}` or `cannot_move{reason}` (L10) | no |
| move | row ids, destination profile id, deck-only accepted (bool) | per row: `full_move{new_note_id, dropped_fields}`, `deck_move` or `skipped{reason}` (L13); or the op's error | yes |
| retry | row id | `created{note_id}`, `matched{note_id}`, `failed{message}`, `pending` or `retry_unavailable` (L14) | yes: up to the 5 s wait plus the downloads |
| live note | row id | note id, or `deleted` (L15) | no |
| delete note | row id | none, or the op's error (L15) | yes |

## L17. Scenarios

Planned for `conformance/recordings/servers/migaku/moves/` (`servers/transcripts` T2). They wait for transcript steps for Migaku profiles, the L16 calls, the prune pass and the fault hooks. They use `servers/migaku-receive` P24's note type `T-Mining` and its profiles, plus `ES` (Spanish, defaults Is Vocabulary Card `x` and Is Audio Card `x`, deck `ES::Mining`) on a second note type `T-Mining-ES` with the same 11 field names. Each starts by receiving cards through `/receive`.

| Id | Setup | Action | Expected |
|---|---|---|---|
| `ae74-cross` | an unreviewed card received into `ES` | move to `DE` | a `T-Mining` note in `DE::Mining` with the same Sentence, Translation, Target Word, Definitions and media names, Is Vocabulary Card `x`, Is Audio Card `x`; the `ES` note deleted with note and card graves; `col.scm` unchanged; status Moved, decision Manual; one undo restores the `ES` note and its id and the status Created; redo makes the `T-Mining` note live again under the same id, status Moved |
| `ae74-reviewed` | as above, then one review of its card | eligibility for `DE`; then move with deck-only accepted | eligibility "deck move only" with the L10 reason; after the move every card of the note is in `DE::Mining`, the note keeps its id and note type |
| `edited-default` | a card received into `ES`; its Is Audio Card edited to empty | move to `DE` | Is Audio Card stays empty, Is Vocabulary Card `x` |
| `ja-to-de` | a card received into `JA` | move to `DE` | Is Vocabulary Card `x`, Is Audio Card `x` |
| `three-one-reviewed` | three cards received into `ES`, one of them reviewed | move all three to `DE`, deck-only not accepted | one undo step; two full moves; the reviewed row skipped with the L10 reason; one undo restores both |
| `deleted-row` | a received card | delete its note in Browse; then undo | row Deleted; then Created |
| `deleted-while-off` | a received card; Migaku off; delete the note; Migaku on | server start | row Deleted |
| `retention` | 600 rows within 10 days; separately 520 rows older than 30 days | prune pass | all 600 kept; newest 500 of the 520 kept |
| `badge` | one Uncertain row (Fallback, no detected language: a D10 tie), one Fallback row (Fallback, detected `ko` with no Korean profile) and one Failed row, unconfirmed | count; confirm the Fallback row; Retry the Failed one successfully with decision Auto | labels Uncertain and Fallback; 3; 2; 1 |
| `deck-move-undo` | a card received into `DE`; a second deck `DE::Other`; a profile `DE2` with `T-Mining` and `DE::Other` | move to `DE2`; undo; redo | deck move; status Moved, then Created, then Moved; the decision Manual after each; three `record_manual_move` calls with `DE2`, `DE`, `DE2` |
| `browse-deck-change` | the row of `deck-move-undo` after the move | Change Deck in Browse to `DE::Mining` | entry stays in effect, status Moved; no `record_manual_move` call (neither pattern of L13) |
| `crash-before-row` | fault hook of `servers/migaku-receive` P24 `crash-between`, live request | restart; read page | one Created row with the note id, media names from the note's fields, and warnings; no hidden row |
| `retry-ae73` | a Failed row from `ae73-remote-404`; the stub then serves `/b` | Retry | note created; row Created; payload deleted |
| `retry-pruned` | a Failed row 8 days old | Retry | `retry_unavailable` |
| `pending-old` | switch on; a pending entry 8 days old; collection made available | launch | the entry commits before the prune pass; row Created |
| `pending-old-fails` | switch on; a pending entry 8 days old whose deck was deleted | launch | row Failed with the R1715 deck message; payload kept, pruned 7 days after the drain; Retry available |
| `pending-retry` | switch on; a Pending row; collection available | Retry | one note |
| `supersede` | a Failed row for item ids `{a, b}` | a receive with ids `{a, b}` that succeeds | one Created row; the Failed row and its payload gone |
| `manual-route` | profiles `DE`, `ES`; a move to `DE` at 10:00 | at 10:05, a card whose detection is uncertain | routed to `DE` with decision Fallback |

## C1. `/create`: the request (R1731)

- The route is `POST /create`, on the Migaku server, with that server's origin, CORS, `Host` and body-limit rules (`servers/migaku-protocol`, `servers/host-security`).
- The body is a form. `multipart/form-data` with a `boundary` parameter is read as multipart. `application/x-www-form-urlencoded`, or no `Content-Type`, is read as URL-encoded: UTF-8, `+` as a space, then percent-decoding. Any other `Content-Type` fails with 400 `Unsupported form encoding.`
- In multipart, a part whose `Content-Disposition` has a `filename` parameter is a file part. Every other part is a text field. The `filename` value is taken literally between its double quotes: a backslash is an ordinary character, not an escape. Browsers' `FormData` sends Windows paths this way, and C2.6 splits at `\`. In URL-encoded forms every pair is a text field. When a text field occurs twice, its last occurrence counts, as for a repeated JSON key in `servers/migaku-receive` P3 (Meguri rule; the add-on's choice is not recorded).
- Only file parts named `files` are files of the card (C2.6). A file part with another name, and a text field named `files`, are ignored.
- The form's kind is decided in this order:
  1. a text field named `Migaku-Deliver-Audio`, `Migaku-No-Audio-Found`, `Migaku-Accept-Data`, `Migaku-Deliver-Definitions` or `searchNote` (the first of these names, in this order, that is present): that sub-command (C3);
  2. otherwise a text field `card`: card creation (C2);
  3. otherwise 400 `Nothing to create.`

## C2. `/create`: card creation (R1731, R1732)

1. **Version.** The text field `version` must be exactly `2`. When it is missing or holds anything else, the reply is HTTP 200, `Content-Type: text/plain; charset=UTF-8`, body `Card could not be created: Version mismatch`, and nothing else happens (Q1).
2. **Card.** `card` is JSON text holding one object:

   | Key | Value |
   |---|---|
   | `noteTypeId` | a JSON integer, or a string of ASCII digits |
   | `deckId` | the same |
   | `fields` | an array of `{"name": <string>, "content": <string>}`; missing or `null` reads as empty |
   | `tags` | an array of strings, or one string split at white space; missing or `null` reads as no tags |

   Other keys are ignored. Any other shape fails with 400 `Invalid card data.`
3. **Destination.** The note type with id `noteTypeId` and the deck with id `deckId` are used exactly, with no routing (R1732). A missing one fails with 400 `Note type <id> does not exist.` or `Deck <id> does not exist.`, the note type first. A deck that is a filtered deck fails with 400 `Deck <id> is a filtered deck.`, since the note could not go to exactly that deck (`notes/deck-placement-and-position` D2 would place it elsewhere; Meguri rule, L18 Q5). `<id>` is the id as parsed, written in decimal without leading zeros. Both are checked again at commit.
4. **Matching profile.** The profile `servers/migaku-routing` D13.2 names: the one with the same note type and deck, else the lowest id among those with the same note type, else none.
5. **Field values.** For each field of the note type: the `content` of the first `fields` entry whose `name` equals the field's name exactly, else the empty string. Then, when the value is empty, the matching profile's default for that field (as `servers/migaku-receive` P7 step 2). Entries with other names are ignored.
6. **Files.** Every `files` file part (C1), in form order, is one file. Its desired name is the last path component of its `filename` parameter, splitting at both `/` and `\`. A part whose filename, or whose last component, is empty fails with 400 `Invalid card data.`, and a part with no bytes with 400 `File <name> is empty.` Files are stored by `media/media-files` M3, staged inside the commit op (M13). When the stored name differs from the given name, every `[sound:<given>]`, `src="<given>"` and `src='<given>'` in the field values is rewritten with the stored name (the given name matched exactly). An M3 refusal fails with 400 `Could not store file <name>.`
7. **Busy.** Card creation takes its place in the server's FIFO and waits up to 5 s for the collection, as `servers/migaku-receive` P13 and P14 say. Then it fails with 400 `Meguri is busy`. `/create` has no pending queue, whatever the switch.
8. **Commit.** One op `migaku_create` with source `LocalApi` adds the note by `notes/note-records` A1: the note type, deck, field values, the tags (`tags/tags` T4), a new GUID and `update_add_defaults` false. Its op-catalog entry (`engine/op-model` O9, added by U140) is: undo class not destructive, no long write. A full disk fails with 400 `No space left on device`. Any other failure of the add fails with 400 `Meguri could not add the card.` Any failure aborts the op and leaves no file. There is no retry detection.
9. **Reply.** HTTP 200 `{"id": <note id>}`, a JSON reply (`servers/migaku-protocol` P1).
10. **Log.** A Created row with endpoint `create`, decision Manual, no routing route, detection or retry key, the matching profile (or none), the stored file names (under images when the extension is one of the image extensions of `servers/migaku-receive` P9, under sentence audio otherwise), and the warnings of `servers/migaku-receive` P21. Its target word and sentence snippet come from the stored note: the first field, in field order, that the matching profile's plan gives `targetWord` or `targetWordNoSyntax` (for the snippet, `sentence` or `sentenceNoSyntax`), stripped and cut as L2 says. With no matching profile, or no such field, they are empty. The duplicate check's target field is the same field, so with no profile there is none. A failed `/create` writes no row. A crash between the commit and the row write may lose the row but never duplicates the note.

Errors other than the version mismatch use the Migaku error shape (`servers/migaku-protocol` P1): HTTP 400 `{"success": false, "error": "<message>"}`.

## C3. `/create`: sub-commands (R1733)

| Sub-command | Effect | Reply |
|---|---|---|
| `Migaku-Deliver-Audio`, `Migaku-No-Audio-Found`, `Migaku-Accept-Data`, `Migaku-Deliver-Definitions` | none | 200, `text/plain; charset=UTF-8`, body `OK` (Q3) |
| `searchNote` with a value of 1 to 19 ASCII digits | the detached host interaction `BrowseNote` of `servers/ankiconnect-gui` G8 and G10, payload `{"nid": "<value>"}` (timeout 10 s, `servers/host-security` H11.2), which shows Browse with the search `nid:<value>` (U146); the reply does not wait for it | 200, `text/plain; charset=UTF-8`, body `OK` |
| `searchNote` with any other value | none | 400 `{"success": false, "error": "Invalid note id."}` |

Sub-commands never check `version`, never wait for the collection, never read or write it and write no log row.

## C4. Differences from the add-on

- `/create` writes a log row and applies the matching profile's defaults (R1732). Both are Meguri additions: the received log is a Meguri feature (R1721), and per-profile defaults are Meguri's own setting (R1713).
- Busy handling, atomic media and `update_add_defaults` follow `servers/migaku-receive`.

## C5. Scenarios

Each row is the declared scenario `conformance/recordings/servers/migaku/create/<id>.yaml` (`servers/transcripts` T2, T13), except `filtered-deck`, which is planned. They use P24's note type and profiles. `<T>` and `<D>` are the ids of `T-Mining` and `DE::Mining`.

The written files check every reply. The `{"id": <nid>}` replies need `servers/transcripts` T10.5 id positions for Migaku replies (`/id` of `/create` and `/receive`). For the After column, they check today only what T13 and T14 can check: `no_effect` (collection and media unchanged) for the refusals and the sub-commands, and the T14 equivalence of the created note (note type, deck, fields, tags and the files the fields reference) for `multipart-two-files`, `file-path-name`, `repeated-field` and `create-row-text`. The other After checks wait: log rows (no row, Created, Manual, media names, target word and snippet), the count of notes, the rewrite to `[sound:clip.m4a]` (T14 accepts either name), files no field references, interactions, and profile `DE`'s defaults. They need T13 output expectations (on `logs`, `events` and `observe`) and a transcript key for Migaku profiles. Until then the transcripts store no profile, so the created notes have no defaults and their rows no target word or snippet. `filtered-deck` waits for a filtered-deck setup or step.

| Id | Request | Reply | After |
|---|---|---|---|
| `version-mismatch` | URL-encoded `version=1&card=<valid>` | 200 text `Card could not be created: Version mismatch` | nothing changes |
| `multipart-two-files` | multipart: `version=2`; `card={"noteTypeId": <T>, "deckId": <D>, "fields": [{"name": "Sentence", "content": "Ein Text."}, {"name": "Sentence Audio", "content": "[sound:Clip.m4a]"}], "tags": ["legacy"]}`; files `Clip.m4a` (M4A) and `pic.png` (a PNG stub) | 200 `{"id": <nid>}` | note in `<D>`; Sentence Audio `[sound:clip.m4a]` (rewritten to the stored name); Is Vocabulary Card `x`, Is Audio Card `x` from profile `DE`; tags ` legacy `; files `clip.m4a`, `pic.png`; row Created, decision Manual |
| `unknown-deck` | `version=2`, `card` with `deckId` 999 | 400 `Deck 999 does not exist.` | nothing changes; no row |
| `unknown-notetype` | `version=2`, `card` with `noteTypeId` 998 and `deckId` 999 | 400 `Note type 998 does not exist.` | nothing changes |
| `accept-data` | URL-encoded `Migaku-Accept-Data=1` | 200 text `OK` | no collection read or write; no row |
| `search-note` | URL-encoded `searchNote=1234` | 200 text `OK` | one Browse interaction with `nid:1234` |
| `search-note-bad` | URL-encoded `searchNote=12a` | 400 `Invalid note id.` | no interaction |
| `form-encoding` | `Content-Type: application/json` | 400 `Unsupported form encoding.` | nothing changes |
| `filtered-deck` (planned) | `version=2`, `card` with `noteTypeId` `<T>` and the id `<F>` of a filtered deck | 400 `Deck <F> is a filtered deck.` | nothing changes |
| `file-path-name` | multipart: `version=2`; valid `card`; one `files` part with `filename="C:\clips\a1.m4a"` (M4A), one part `other` with `filename="x.png"` | 200 `{"id": <nid>}` | file `a1.m4a` only; row media names `a1.m4a` |
| `repeated-field` | URL-encoded `version=1&version=2&card=<valid>` | 200 `{"id": <nid>}` (the last `version` counts) | one note |
| `create-row-text` | multipart: `version=2`; `card` for `<T>`, `<D>` with Target Word `<b>Hund[Hund,nn,m]</b>` and Sentence `Der Hund.` | 200 | row target word `Hund`, snippet `Der Hund.` (profile `DE` maps them) |

## L18. Open questions

- **Q1. Version mismatch status.** The text of the version-mismatch reply is recorded (R1731), but its HTTP status and content type are not. C2 sends 200 with `text/plain`.
- **Q2. Decision of `/create` rows.** R1722's decision kinds describe routing, which `/create` skips. C2 records Manual, which keeps such rows out of the badge and out of routing state (no `record_manual_move`, and no received count, `servers/migaku-routing` D12).
- **Q3. Sub-command shape and replies.** The sources name the five sub-commands and say they get plain-text acknowledgements, but not how the extension sends them (C1 assumes a form field of that name) or the exact texts. C3 replies `OK`. `/create` is legacy (R1731), and the plan treats `/receive` as the live route, so the risk is low. A recording of the extension's `/create` traffic would settle both.
- **Q4. The move step's name.** Settled for the spec: `conformance/undo-ops` V4 names L13's `Move Received Card` (id `move_received_card`) as its pending Meguri-only row. U140 adds that row to V4's table in the same change as the engine's step name (the engine test keeps the two equal), so `engine/undo` M1 accepts it and `undo_status` reports it. Two more entries outside the specs come with the op: the `undo_step` enum of `conformance/schema/observation.schema.json` (which mirrors V4) gets `move_received_card`, and `engine/op-model` O9 gets the catalog entry `migaku_move` (L13). Reusing "Change Deck" or "Change Note Type" was rejected: it would mislabel a move that mixes both kinds of rows.
- **Q5. `/create` into a filtered deck.** The sources do not say what the add-on does with a `deckId` that names a filtered deck. Anki's add would place the cards elsewhere (`notes/deck-placement-and-position` D2), which contradicts R1732's "exactly that deck", so C2.3 refuses it.
- **Q6. Undo detection.** L13 recognizes an undo or a redo of a move from the state it leaves, since no event names the restored step. A user who restores exactly the same state by other means (for example moving every card of a deck-moved note back to their old decks in Browse) is treated as an undo: the decision stays Manual and routing gets one more `record_manual_move`. A change-stream event naming the restored step (a cross-area change to `engine/change-stream` and `engine/undo`) would remove this case.
- **Q7. Uncertain and Fallback labels.** R1722 names both labels, R1719 flags every card it routes as "Uncertain", and routing gives all of them the decision `Fallback` (`servers/migaku-routing` D11). L4 reads Uncertain as "the language could not be detected" and Fallback as "the detected language has no profile". Another reading would key the label on the route (`recent` against `fallback_profile`). Both count for the badge alike, so only the label text depends on the choice.
