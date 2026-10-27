# Collection check (oracle)

Spec: conformance/collection-check
Requirements: R1804, R1805, R1887; supports R1803, R1941, R1942
Anki: 26.09.3
Scenarios: none (a command, not a scenario); the `check_db` op of `dsl.md` D7.2 reports K5

A black-box check of a collection directory that someone else wrote, such as a generated one (`specs/fixtures/synthetic-collections.md`) or an anonymized clone (`specs/fixtures/anonymizer.md`): Anki opens a copy at a pinned instant and reports its row counts, the day it computes, deck-list counts, render results, optionally digests of every card's next states and every deck's queue, and what its Check Database finds and changes. Implementers run it like `tools/mg oracle record`, from any worktree, without reading `oracle/`.

## K1. Command

```
tools/mg oracle check <dir> [--now <instant>] [--tz <zone>] [--deck <name>]... [--render-note <note id>]... [--schedule]
```

- A host job. `<dir>` is a directory on the host holding `collection.anki2`, and optionally `collection.anki2-wal`, `collection.media/` and `manifest.json` (`fixtures/synthetic-collections` F10.2). The job gives one oracle worker read-only access to it and never writes it.
- `--now`: the instant the oracle's clock is frozen at, written as `clock.start` with an offset or `Z` (`dsl.md` D5), whole seconds. Default: the manifest's `now_ms` divided by 1000, rounded down. Without both, a usage error.
- `--tz`: the IANA zone the worker runs in. Default from the manifest's `utc_offset_secs`: `UTC` for 0; for a whole number of hours `h` from -12 to +14, `Etc/GMT-<h>` for east and `Etc/GMT+<h>` for west (IANA's inverted sign: +1 h is `Etc/GMT-1`). Any other offset, or no manifest, needs `--tz`. The zone must exist in the worker's time zone database.
- `--deck <name>`: a deck by its human name (`::` between components), any number of times. `--render-note <id>`: a note by its real id in decimal, any number of times.
- `--schedule`: also report K11.
- A usage error is the job status `error` with failure `usage`, before any worker starts.

## K2. Copy and clock

- The worker copies `collection.anki2` (and `collection.anki2-wal` when present) into its scratch directory and works only on the copy, which it deletes at the end. It needs no media folder.
- The worker's clock is frozen at `--now` in `--tz` for the whole check (as a scenario's frozen clock, `dsl.md` D5). Nothing advances it.

## K3. Steps, in order

1. Open the copy as Anki's desktop client opens a collection (with its Python layer, so the scheduler is loaded as for a user). A file Anki cannot open ends the check: K7 `open_failed`.
2. `counts`: row counts of `notes`, `cards`, `revlog`, `notetypes`, `decks`, `deck_config` (reported as `presets`), `tags` and `graves`.
3. `timing`: as the `timing` observation (`observations.md` O4): `days_elapsed`, `next_day_at`, `now`, `utc_offset_secs`.
4. `decks`: for each `--deck` (K6).
5. `render`: for each `--render-note` (K8).
6. Digest each of the twelve tables of `observations.md` O8: its rows in their O8 key order, every column with its storage class and value (any digest that is equal exactly when the rows are; only equality is used).
7. Run Check Database as Anki's Check Database command runs it (integrity check, optimize, then the repairs of K5). Its outcome is `check_db` (K5).
8. Digest the twelve tables again. `changed_tables` lists, in O8's table order, those whose digest differs from step 6.
9. Close the collection and delete the copy.

With `--schedule`, K11 runs between steps 5 and 6.

Steps 2 to 5 (and K11) run before Check Database, so they see the collection as written. Anki's own writes while they run (for example the config key `localOffset` that a client writes when it computes the day, or `curDeck` when K11 selects decks) happen on the copy and before step 6, so they never show in `changed_tables`.

## K4. Report

One JSON object, `conformance/schema/collection-check.schema.json`, in the goldens' text layout (`dsl.md` D11):

```json
{
  "changed_tables": ["col", "config"],
  "check_db": {"card_last_review_time": 0, "card_properties": 0, "cards_without_note": 0, "cards_without_template": 0, "decks_missing": 0, "duplicate_card_ords": 0, "field_counts": 0, "future_ids": 0, "new_positions_too_high": 0, "notes_invalid_utf8": 0, "notetypes_recovered": 0, "revlog_properties": 0},
  "counts": {"cards": 10012, "decks": 38, "graves": 0, "notes": 8701, "notetypes": 27, "presets": 9, "revlog": 80310, "tags": 167},
  "decks": {"Due": {"learning": 0, "new": 0, "review": 9999}},
  "dir": "build/fixtures/0123456789abcdef-1-10000",
  "now": 1894701600,
  "oracle": {"anki_version": "26.9.3", "driver_sha256": "<hex>", "lock_sha256": "<hex>"},
  "render": {"1883000000123": [{"answer_bytes": 1048790, "card": 1883000000123, "ord": 0, "question_bytes": 214, "status": "ok"}]},
  "report_version": 1,
  "timing": {"days_elapsed": 500, "next_day_at": 1894762800, "now": 1894701600, "utc_offset_secs": 3600},
  "tz": "Etc/GMT-1"
}
```

- `dir` is `<dir>` as given; `now` is the frozen instant in epoch seconds; `tz` the zone used.
- `decks` and `render` are `{}` when no `--deck` or `--render-note` was given.
- `schedule` (K11.3) is present only with `--schedule`; `report_version` stays 1.
- `check_db` is the K5 object, or `{"error": "corrupt"}` when Anki's integrity check found the file corrupt (then `changed_tables` is `[]`).

## K5. Check Database problems

Anki reports each problem as a localized message (R1887 keeps them out of reports). The report gives each kind below as a count, every kind present, 0 when Anki reported nothing of it. A kind's count is the number in Anki's message, except `notetypes_recovered`, whose message carries no number: 1 when reported, else 0.

| Kind | What Anki counts (and repairs) |
|---|---|
| `card_properties` | cards it rewrites because `due` is not an integer within `i32` (rounded, clamped), a review-queue card (queue 2) has `due` above 100,000 (set to today), `odue` is not an integer within `i32`, a card of type 1 or in queue 2 has `odue` above 0 and `odid` 0 (`odue` set to 0), `ivl` is not an integer from 0 to 2,147,483,647 (rounded, clamped), `mod` is not an integer (truncated), or `ord` is outside 0 to 30,000 (clamped); plus cards with `odid` above 0 whose `did` is a normal deck (`odid` and `odue` set to 0) |
| `new_positions_too_high` | type-0 cards outside queue 4 whose `due` is 2,000,000 or more (set to 1,000,000 plus `due` modulo 1,000,000; Anki's message says "1,000,000 or more", but a `due` from 1,000,000 to 1,999,999 is left alone) |
| `card_last_review_time` | cards of a type other than 0 with at least one review-log row whose `lrt` (card data) differs from the expected time, which it writes: the instant in seconds of the card's last row that has a button (`ease` above 0) and is not a cramming row (type 3 with `factor` 0), counting only rows after the card's last reset row (type 4 with `factor` 0); absent when there is none |
| `cards_without_note` | cards whose `nid` is no note (deleted) |
| `decks_missing` | deck ids that cards use but no deck has (a deck is created for each), plus parents missing from deck names (created) |
| `field_counts` | notes whose field count differs from their note type's (fixed, and tagged `db-check`) |
| `duplicate_card_ords` | a note's second and later cards with the same `ord` (deleted) |
| `cards_without_template` | cards of a normal note type whose `ord` is not below its template count (deleted) |
| `revlog_properties` | review-log rows whose `ivl` or `lastIvl` is not an integer (rounded), whose `time` is not an integer from 0 to 2,147,483,647 (rounded, clamped), or that have `type` 0, `time` 0 and `ease` 0 (type set to 5). Anki never counts an `ivl` or `lastIvl` whose truncation is outside `i32`: its Check Database stops first, changing nothing (K10 `failed`; `maintenance/check-database` C8.7). Meguri clamps such a value and counts the row (C10.1), deviation `check-db-lrt-reads-needed-columns` |
| `notes_invalid_utf8` | notes whose text is not valid UTF-8 (fixed) |
| `future_ids` | rows of `notes`, `cards` and `revlog` whose id is greater than the check's `now` in ms plus 86,400,000 (given new ids) |
| `notetypes_recovered` | note types that notes use but that do not exist (recreated) |

Check Database also changes rows without counting a problem, which only `changed_tables` shows: it adds the cards a note's templates or cloze numbers call for and that are missing; rewrites notes whose sort field, checksum, NFC form or tag string differ from what Anki computes; rebuilds the `tags` table from the notes' tags (keeping which tags were expanded); sets `nextPos` to the largest `due` of type-0 cards plus 1 (0 when there is none); sets the field tags of image-occlusion note types (stock kind 6 with at least five fields) that have notes, on their unprotected fields only: each such field at positions 0 to 4 gets its position as its tag, and those at positions 0 to 3 become protected (`maintenance/check-database-notes` N5); and adds a stock note type when there is none.

Check Database marks the schema changed (sets `scm`, forcing a one-way sync) only when at least one of these repairs happens: it writes a card's `lrt` (`card_last_review_time` above 0), deletes `cards_without_note`, clears `odid` and `odue` of cards whose `did` is a normal deck (the part of `card_properties` that does so), deletes `duplicate_card_ords` or `cards_without_template`, fixes `field_counts`, recovers a note type (`notetypes_recovered`), fixes `revlog_properties`, or gives `future_ids` new ids. Every other repair (the rest of `card_properties`, `new_positions_too_high`, `decks_missing`, `notes_invalid_utf8`) and every uncounted change above leaves `scm` as it was. Decided 2026-10-27: this replaces the earlier "after any repair", which was broader than Anki 26.09.3.

## K6. Deck counts

`decks` maps each `--deck` name, as given, to the three numbers Anki's deck list shows on that deck's row at `now` (`new`, `learning`, `review`: the deck with its subdecks, limits applied), or `null` when no deck has that name.

## K7. Errors

When the check cannot produce a report, the job status is `error` with one failure whose kind is:

| Kind | When |
|---|---|
| `usage` | K1 |
| `no_collection` | `<dir>/collection.anki2` does not exist |
| `open_failed` | Anki could not open the copy (too old, too new, not a collection) |
| `oracle_error` | any other failure in the worker (`detail` is driver text, never compared) |

## K8. Render

`render` maps each `--render-note` id, in decimal, to the note's cards in `ord` order, or `null` when no note has that id. Each card is rendered as for review (not the browser's templates), and reported as `{card, ord, status, question_bytes, answer_bytes}`:

- `status`: `ok`; `empty` when Anki reports the card's front empty (its empty-front or missing-cloze message); `template_error` when a template cannot be rendered (a syntax error, a field the note type lacks).
- `question_bytes`, `answer_bytes`: UTF-8 byte lengths of the question and answer text Anki renders for the card, without the note type's CSS; 0 for `template_error`.

## K9. Job result

- `result.json` (`tools/AGENTS.md`) lists the report as an artifact of kind `report`, `check.json` in the run directory, and repeats it under `details.check`.
- Status: `fail` when the report has a `check_db` count above 0, `check_db` is a rejection, or `changed_tables` names a table other than `col` and `config`; `pass` otherwise; `error` per K7. `--schedule` adds no condition: K11 observes, it does not judge.

## K10. The `check_db` op

In scenarios, `check_db` takes `{}` and runs step 7 on the scenario's collection. Its output is `{"check_db": <the K5 object>}`, or a rejection (nothing changed): `{"check_db": {"error": "corrupt"}}` when Anki's integrity check finds the file corrupt, or `{"check_db": {"error": "failed"}}` when Anki's Check Database stops with any other error, which it raises before changing anything (a column out of its stored range, C8.7 of `maintenance/check-database`; a new id that is taken, its C10.4; a row it cannot read or a statement that fails, its C13). It is stable (`dsl.md` D7.2). On Meguri's side the op runs `maintenance/check-database` (U128) and `maintenance/check-database-notes` (U129) and returns their report's `problems`, or `failed` when the job ends with `CheckFailed{stage}` whose `stage` is `cards`, `notes` or `history` (a repair stopped on a row it cannot read or a statement that failed, the whole repair op rolled back, C13). Meguri's `Corrupt` is `corrupt`; `CheckFailed{stage: integrity}`, `CheckFailed{stage: optimize}`, `SafetyBackupFailed`, `Cancelled` and its other errors are run errors.

- Decided 2026-10-27: `failed` is returned by both sides when the check stops at a row it cannot read (C13: an undecodable deck or note type record, a note whose `guid` is not valid UTF-8, `maintenance/check-database-notes` N6.1); such a scenario needs no deviation. Five scenarios record `failed` on Anki's side alone, and Meguri repairs each collection under a deviation: `revlog-ivl-overflow` (`check-db-lrt-reads-needed-columns`), `future-ids-taken` (`check-db-future-ids-skip-used`), `parents-under-filtered` and `parents-too-deep` (both `check-db-parents-past-deck-limits`) of `maintenance/check-database` C18.2, and `default-deck-missing` (`check-db-default-deck-restored`) of `maintenance/check-database-notes` N10.2. Those two sections list them.
- The `tools/mg oracle check` command keeps K4 and K7, so there such a failure is the job error `oracle_error`, never a `check_db` value.

## K11. Schedule (`--schedule`)

What Anki's scheduler would do at `now`, as counts and digests. Two collections that hold the same cards with other text, such as a collection and its anonymized clone (`fixtures/anonymizer` A17 V3), can be compared without any card text, deck name or tag in the report. K11 runs after K3 step 5, on the collection as written, once step 3 has computed the day (which, as opening the deck list does, unburies cards buried on an earlier day).

### K11.1 Steps

1. Deck list: the deck tree Anki computes at `now`, read as K6 reads it: for every deck its `new`, `learning` and `review` numbers (with subdecks, limits applied), and the root's numbers.
2. Queues: for every deck, normal and filtered, in ascending id order: select it as the deck list does when the user clicks it, build its study queue, and read all of it in the order the reviewer would present the cards if none were answered: learning cards due now, then the main queue (reviews, interday learning and new cards as Anki mixes them), then learning cards due within the learn-ahead limit (`collapseTime`). Each entry is a card id and its kind, `new`, `learning` or `review`. Also read the three counts Anki gives that queue.
3. Next states: for every card, in ascending id order, its `next_states` value (`observations.md` O4, O5), as the reviewer computes it while a queue is built. When `loadBalancerEnabled` is on, the load balancer is in effect; it covers every deck's cards whichever deck's queue is built, so the last queue of step 2 serves.

Steps 2 and 3 write only `curDeck` and `activeDecks` on the copy (K3).

### K11.2 Digests

A digest is the SHA-256, in lower-case hex, of UTF-8 lines each ending in LF. An empty list gives the digest of no bytes, `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.

- Queue line: `<card id>`, TAB, `<kind>`; lines in queue order.
- Next-states line: `<card id>`, TAB, the O5 value written compactly: no whitespace, object members sorted by name (code point order), absent optional members (`memory_state` with FSRS off) left out, integers in decimal, 32-bit floats as O6 writes them; lines in card id order.

Example lines (TAB written as `→`): `1894701600123→review` and `1894701600123→{"again":{"kind":"relearning",...},"current":{...},"easy":{...},"good":{...},"hard":{...}}`.

### K11.3 Report

The report gains one member:

```json
"schedule": {
  "cards": 10012,
  "decks": {
    "1": {"cards": 0, "next_states_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "queue": {"entries": 0, "learning": 0, "new": 0, "order_sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855", "review": 0}, "tree": null},
    "1894701600001": {"cards": 412, "next_states_sha256": "<hex>", "queue": {"entries": 220, "learning": 3, "new": 20, "order_sha256": "<hex>", "review": 197}, "tree": {"learning": 3, "new": 20, "review": 197}}
  },
  "root": {"learning": 3, "new": 40, "review": 412},
  "sha256": "<hex>"
}
```

- `cards`: the number of cards.
- `decks`: one member per deck, keyed by its id in decimal:
  - `cards`: the cards whose `did` is this deck (subdecks not included; a filtered deck has the cards it holds);
  - `next_states_sha256`: the next-states digest of those cards;
  - `queue`: `entries`, the number of entries step 2 read; `new`, `learning`, `review`, the counts Anki gave; `order_sha256`, the queue digest;
  - `tree`: the deck's step 1 numbers, or `null` when the deck tree leaves the deck out (an empty default deck without subdecks).
- `root`: the deck tree root's numbers. The root has numbers but no queue: Anki builds a queue only for a selected deck, and the root is not one (every top-level deck's queue is reported under its id).
- `sha256`: the SHA-256 of `{"cards":…,"decks":…,"root":…}` written compactly (no whitespace, members sorted by name).

Ids are the collection's own; no deck name, tag or field text appears. A report from the private collection stays in its run directory; only pass counts are committed (R1940).

### K11.4 Comparing

Two reports have the same schedule exactly when their `schedule.sha256` are equal; the per-deck digests then locate a difference. Digests compare oracle reports with each other, under one oracle pin. They do not compare Meguri with Anki: O7's tolerance on memory states needs the values, which the private suite (R1942) compares itself.
