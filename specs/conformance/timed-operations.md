# Timed operations (Meguri versus Anki)

Spec: conformance/timed-operations
Requirements: R1806, R1865
Anki: 26.09.3
Scenarios: none (a command, not a scenario)

The same named operations, timed in real Anki (the oracle) and in Meguri, on the same machine, on copies of the same collection, under one measurement protocol, so that the ratio Meguri time / Anki time means something. This file fixes what both sides must do identically: the oracle command (T1), the inputs and clock (T2), the protocol (T3, T4), the shared inputs (T5), the operation catalogue (T6) and the report both sides write (T7). Meguri's side is `meguri-cli timing run` (T10); the job that compares the two reports and keeps the history is `tools/mg perf-ratio` (U18, Meguri tooling), which builds on T11.

Timings are not goldens. Nothing here is recorded under `conformance/goldens/` (T12).

## T1. Command

```
tools/mg oracle time <dir> [--platform apple] [--op <name>]... [--now <instant>] [--tz <zone>] [--repeat <n>]
```

- A host job. `<dir>` is a directory on the host holding `collection.anki2`, and optionally `collection.anki2-wal` and `manifest.json` (`fixtures/synthetic-collections` F10.2), for example a generated collection (`tools/mg fixtures 10k`) or a private-vault copy (U15). Nothing in `<dir>` is written. No operation reads media, so `collection.media/` is not used.
- `--platform apple` runs the oracle natively on the Mac (the macOS lane of `float-determinism.md` F6, KTD24): the primary series, where both sides run natively on one Mac. Without it the oracle runs in its Linux container (the Docker lane): the secondary series.
- `--op <name>`: an operation of T6 by its exact name, any number of times. Default: every operation. The operations run, and are reported, in catalogue order (T6), whatever the order of the options. Naming one twice changes nothing. An unknown name is a usage error.
- `--now`: the instant the clock is frozen at, written as `clock.start` with an offset or `Z` (`dsl.md` D5), whole seconds. Default: the manifest's `now_ms` divided by 1000, rounded down. Without both, a usage error.
- `--tz`: the IANA zone the run uses. Default from the manifest's `utc_offset_secs`, exactly as `collection-check.md` K1 derives it (`UTC`, `Etc/GMT-<h>`, `Etc/GMT+<h>`). Any other offset, or no manifest, needs `--tz`.
- `--repeat <n>`: timed repetitions per operation, an integer from 1 to 100. Default 5.
- A usage error is the job status `error` with failure `usage`, before any worker starts.

## T2. Inputs and clock

### T2.1 The image

At the start of a run, before anything is timed, the side copies `collection.anki2` (and `collection.anki2-wal` when present) into its scratch directory and folds the WAL into the copy with plain SQLite (a checkpoint, `PRAGMA wal_checkpoint(TRUNCATE)`, then close, then delete an empty `-wal` left behind), so the copy is one file. Nothing else changes it. This file is the **image**.

- Neither Anki nor Meguri ever opens the image: every operation works on a copy of it (T3.2), the import package and the download payload are built from its bytes (T5.4, T5.5).
- The image keeps the source's journal mode and schema. Fixtures and the user's collection are at schema 18; an older file would be upgraded by every open, and the open would time that.
- The scratch directory and everything in it are deleted when the run ends, whatever its outcome. When `<dir>` is under `build/private/` (the vault, U15), a side's scratch outside a container is under `build/private/` too.

### T2.2 Collection summary

Then the side opens a copy of the image as T6.1 does (untimed), counts the rows of `notes`, `cards`, `revlog`, `notetypes`, `decks`, `deck_config` (reported as `presets`), `tags` and `graves` (as `collection-check.md` K3 step 2), closes it and deletes it. These counts are the report's `collection` (T7). If this open fails, the run ends with `open_failed` (T8).

### T2.3 Clock

- The wall clock is frozen at `--now` in `--tz` for the whole run. Nothing advances it: every repetition of every operation sees the same instant, so their results are the same (T3.6).
- The frozen clock costs a side no more to read than a normal clock read. In particular, Anki must not re-read a clock file on every read during a timing run: the clock never moves, so the oracle keeps libfaketime's cache for the run instead of the per-read file access that scenario workers use. A per-read file access in the timed interval would add its cost to Anki's samples only.
- A worker that cannot freeze the clock (a native worker that answers `time_control: none`, `float-determinism.md` F6.3) cannot time: the run ends with `clock_unsupported` (T8). Every operation depends on the day.

## T3. Protocol

### T3.1 One warm process per side

Each side runs all its operations in one process that it starts and prepares before the first one: the interpreter and libraries loaded, Anki's backend created once (as the desktop client creates one per profile and opens collections in it), Meguri's engine runtime started. None of that is timed. Process start, interpreter start and library import are never inside an interval.

### T3.2 Runs

For each operation, in catalogue order: one warm-up run, then `--repeat` timed runs. The warm-up run is measured and checked like the others, and its sample is discarded. Every run, warm-up included, does:

1. **Copy.** A fresh copy of the image into a run directory in the side's scratch. Every run starts from the same bytes, read-only operations included, so no run sees another run's writes or caches.
2. **Setup.** The operation's setup (T5.1, T6): opening the copy, choosing its inputs, preparing what the operation acts on.
3. **Timed interval.** T4.1. Only this is measured.
4. **Check.** The operation's result check (T6), read from what the timed call returned and, where T6 says so, from the collection afterwards.
5. **Teardown.** Close the collection and delete the run directory and everything the run wrote there.

Copying, setup, the check, closing, deleting and garbage collection are outside the interval.

### T3.3 Garbage collection

Anki's desktop client turns off the Python layer's automatic garbage collection and collects at moments of its choosing. The oracle does the same during a timing run: automatic collection off, an explicit collection in each run's teardown. Meguri has no collector.

### T3.4 File cache

Warm. Neither side purges or bypasses the operating system's file cache: a run's copy was just written, so its pages are in memory, as a collection the user opens daily usually is. Within one lane both sides keep their copies on the same kind of file system: the Mac's local disk on the native lane (the run directory under `build/`), a tmpfs on the Docker lane (the oracle's `/scratch`).

### T3.5 Equal conditions

- One run at a time within a side, and the two sides never run at the same time on one machine.
- Each side runs the build it ships: Anki the pinned wheel (`conformance/oracle.json`), Meguri an optimized build (Cargo's `release` profile), without debug assertions, sanitizers or coverage. The report names Meguri's build (T7).
- Each side uses its normal settings for everything this file leaves open (SQLite pragmas, threads, compression levels, durability): the comparison is between the two products as users run them.

### T3.6 Stable results

Every run of an operation starts from the same bytes at the same instant, so all its runs must return the same check. A run whose check differs from the operation's first run makes the operation `error` with kind `unstable` (T7.2): the work was not the same each time.

## T4. Measurement

### T4.1 The interval

The side reads a monotonic clock immediately before the first call of the operation's timed part (T6) and immediately after its last call returns. The calls go through the entry point the product itself uses: for Anki, its Python layer as the desktop client calls it; for Meguri, the engine's public entry point that the app's bridge calls (not the Swift layer).

- The interval ends when the side holds the complete result the operation produces (every id, every count, every byte written and the file closed where T6 says so), never a handle or iterator to compute it later.
- Work that the call starts and that continues after it returns (a background task, a deferred write) is part of the interval: the interval ends only when that work has finished. Anki does each operation synchronously.

### T4.2 Samples

A sample is the interval in milliseconds: nanoseconds divided by 1,000,000, rounded to 3 decimal places (the nearest microsecond, halves up). Samples are reported in the order they were taken.

### T4.3 Statistic

- `median_ms`: the sample at position ⌈n/2⌉ (1-based) in ascending order, n the number of timed repetitions. For an even n this is the lower of the two middle samples, never an average, so the statistic is always a sample that was measured. Ratios use it (T11).
- `min_ms`: the smallest sample.

## T5. Shared inputs

Every input is chosen by a rule over ids, counts and fixed text, never by a deck name, tag, field text or other private content, so that a run on the user's collection leaks nothing (R1940). Queries are fixed text (T6.5).

### T5.1 Standard setup and the study deck

The **standard setup** of an operation is: open the run's copy (as T6.1), compute the deck list at `now` (as T6.2), then select the study deck as the current deck, as clicking it in the deck list does.

The **study deck** is the top-level deck whose deck-list row has the largest total of new, learning and review cards (the three numbers the row shows, with subdecks, limits applied), ties broken by the smallest deck id. Only top-level rows of the deck list take part (so an empty default deck that the list leaves out does not). The deck list always has at least one top-level row, so there is always a study deck, even when every total is 0.

Computing the deck list in the setup also does what the first deck list of a day does (unburying cards buried on an earlier day), so that work stays outside every interval but T6.2's.

### T5.2 Answer

The card answered is the first card of the study deck's queue, the one the reviewer shows first. It is answered **Good** (the third of the four buttons), at `now`, with 10,000 ms as the time taken (the review-log `time`).

### T5.3 New collection

The import target (T6.11) is a new, empty collection that the side creates in the run directory at `now`, as it creates one for a new profile (each side with its own new-collection contents: Anki's stock note types and defaults, Meguri's own, R1959). Creating and opening it is setup.

### T5.4 The import package

Built once per run of the command from the image's bytes, before the first operation that needs it:

- A ZIP archive with two entries, in this order, both stored without compression: `collection.anki2`, holding the image's bytes exactly, then `media`, holding the two bytes `{}` (an empty JSON object: no media files).
- There is no `meta` entry, so it is a package of the oldest layout, version 1 (`model-ops.md` L7: without `meta` and without `collection.anki21`, the collection entry is `collection.anki2`). Its collection is at the image's schema (18 for every fixture), not the schema 11 such packages usually hold: an importer opens the entry as it opens any collection file.
- Entry times and other archive metadata are free. Only the two entries' names and contents matter.

### T5.5 Sync stub

For T6.12 and T6.13, each side runs its own HTTP server on `127.0.0.1` at a free port, plain HTTP/1.1, which stands in for a sync server and does no server-side work. The side's sync client uses the endpoint `http://127.0.0.1:<port>/` and any non-empty host key (for example `timing`); requests must not go through a proxy. The stub runs where it never waits on the timed call (its own thread or process) and is started in setup.

- It accepts `POST /sync/upload` and `POST /sync/download` and ignores the request headers (`anki-sync` and the others).
- It reads each request body to its end (a client may send it with chunked transfer encoding) and discards it, then answers at once.
- Its answer: status 200, header `anki-original-size` with the decimal byte length of the content, header `Content-Length`, and as body the content in the raw zstd frame of T5.5.1.
  - `/sync/upload`: the content is the two bytes `OK`, which a client reads as an accepted upload.
  - `/sync/download`: the content is the image's bytes (the collection a server would send).
- Any other request: status 404, no body.

#### T5.5.1 Raw zstd frame

A zstd frame (RFC 8878) that stores the content without compressing it, so both sides receive the same bytes and decoding costs no more on one side than on the other:

1. The magic number, bytes `28 B5 2F FD`.
2. The frame header: descriptor byte `00` (no content size, no checksum, no dictionary, not single-segment), then window descriptor byte `38` (a 128 KiB window).
3. The content cut into pieces of 131,072 bytes, the last one the rest (an empty content is one empty piece; a content of an exact multiple has no extra empty piece). Each piece is one raw block: a 3-byte little-endian block header whose value is `last + 8 × size` (`last` 1 for the final piece, 0 otherwise; block type 0, raw), then the piece's bytes.
4. Nothing after the last block (no checksum).

Example: `OK` is `28 B5 2F FD 00 38 11 00 00 4F 4B`.

## T6. Operation catalogue

Names are stable: the history keys on them. `collection.open` is the name U12's benchmarks use for the same interval. In catalogue order:

| Section | Name | Setup | Timed | Check |
|---|---|---|---|---|
| T6.1 | `collection.open` | none | open the copy | `cards`, `notes`, `revlog` |
| T6.2 | `decks.counts` | open | first deck list | `decks`, `new`, `learning`, `review` |
| T6.3 | `queue.build` | standard | build the study deck's queue, first card | `new`, `learning`, `review` |
| T6.4 | `review.answer` | standard, first card | answer Good, next card | `new`, `learning`, `review` |
| T6.5 | `search.deck`, `search.state`, `search.tag`, `search.text`, `search.regex`, `search.prop`, `search.rated`, `search.combined` | standard | one search, no order | `cards` |
| T6.6 | `browser.sort_field`, `browser.sort_due` | standard | every card, sorted by a column | `cards` |
| T6.7 | `stats.graphs` | standard | every graph, whole collection, one year | `cards`, `reviews` |
| T6.8 | `maintenance.check_db` | standard | Check Database | the K5 counts |
| T6.9 | `package.apkg_export` | standard | export the study deck | `entries`, `notes` |
| T6.10 | `package.colpkg_export` | standard | export the collection | `entries` |
| T6.11 | `package.apkg_import` | new collection | import the T5.4 package | `notes_added`, `cards`, `revlog` |
| T6.12 | `sync.full_upload` | standard, stub | one-way upload, reopen | `cards`, `graves`, `pending` |
| T6.13 | `sync.full_download` | standard, stub | one-way download, reopen | `cards`, `revlog` |
| T6.14 | `fsrs.recompute_memory_states` | standard, FSRS off | turn FSRS on | `memory_states` |

The catalogue order is the order of this table, and within T6.5 and T6.6 the order of their own tables. In the checks, every count is a non-negative integer; the check is a JSON object with exactly the members listed.

### T6.1 `collection.open`

- **Setup:** none beyond the copy.
- **Timed:** opening the copy as the desktop client opens a profile's collection, until the collection can serve requests: the file opened, its schema checked (and upgraded when needed), its configuration read, the scheduler set up (for Anki, its Python layer's collection object complete). Per T4.1, open work a side defers past the call's return counts too.
- **Check** (read after the interval): the rows of `cards`, `notes` and `revlog`.

### T6.2 `decks.counts`

- **Setup:** open the copy. Nothing else: the timed computation is the first deck list after the open.
- **Timed:** computing the deck list at `now`: every deck's new, learning and review numbers, with subdecks, limits applied (`collection-check.md` K6, K11.1 step 1). On the first deck list of a new day this includes unburying cards buried on an earlier day; on generated collections it unburies nothing (`fixtures/synthetic-collections` F6.2 `lastUnburied`).
- **Check:** `decks`, the number of decks in the tree (the root not counted, an empty default deck that the tree leaves out not counted); `new`, `learning`, `review`, the root's numbers.

### T6.3 `queue.build`

- **Setup:** standard (T5.1).
- **Timed:** the reviewer's first fetch after the deck was selected: building the study deck's queue (the deck and its subdecks, limits applied, siblings buried as the queue is gathered, the load balancer when it is on) and returning its first card with that card's next states and the queue's three counts. An empty queue is a valid result.
- **Check:** `new`, `learning`, `review`, the three counts the fetch returned.

### T6.4 `review.answer`

- **Setup:** standard, then the reviewer's first fetch as in T6.3, which gives the card of T5.2 and its next states. When the queue is empty, the operation is `error` with kind `no_input` (stage `setup`).
- **Timed:** answering that card Good at `now` with 10,000 ms (T5.2), using the next states the fetch returned, as the reviewer does: the card's new state written, a review-log row added, the change committed as the side commits every answer, the queue updated; then the reviewer's next fetch: the next card with its next states and the three counts.
- **Check:** `new`, `learning`, `review`, the counts of that second fetch.

### T6.5 Searches

- **Setup:** standard (the study deck is the current deck, for `deck:current`).
- **Timed:** one search of cards over the whole collection with the class's query, in Anki's search syntax, without any ordering, until the complete list of matching card ids is returned.
- **Check:** `cards`, the number of ids.

| Name | Query | What it exercises |
|---|---|---|
| `search.deck` | `deck:current` | deck membership, subdecks included |
| `search.state` | `is:due` | card state at the day of `now` |
| `search.tag` | `tag:*::*` | the tag pattern match (notes with a tag that has a `::` level) |
| `search.text` | `kamo` | an unqualified term: a substring match over every field |
| `search.regex` | `re:k[aeiou]m` | a regular expression over every field |
| `search.prop` | `prop:ivl>=21` | a numeric card property |
| `search.rated` | `rated:7` | answers in the review log over the last 7 days |
| `search.combined` | `deck:current (is:new OR is:due) -is:suspended` | a boolean combination |

The queries are the exact text above (the side passes it as the user would type it). What each one matches is the search area's specification; this file only names them.

### T6.6 Browser sorts

- **Setup:** standard.
- **Timed:** the browser's card search `deck:*` (every card) sorted by one column, ascending, until the complete ordered list of card ids is returned. The order is the browser's for that column (the search area's specification); building the rows shown on screen is not part of it.
- **Check:** `cards`, the number of ids.

| Name | Column (its `sortType` config value) |
|---|---|
| `browser.sort_field` | Sort Field (`noteFld`) |
| `browser.sort_due` | Due (`cardDue`) |

### T6.7 `stats.graphs`

- **Setup:** standard.
- **Timed:** computing the data of every graph of the Stats screen in one request, for the search `deck:*` and a range of one year (365 days): card counts, future due, intervals, ease or difficulty, stability, retrievability, reviews per day with their times, hours, answer buttons, cards added, today, and true retention. Drawing is not part of it.
- **Check:** `cards`, the number of cards the card-count graph covers (all its kinds summed, suspended and buried cards included); `reviews`, the total of the review graph's daily counts over the range (every kind summed).

### T6.8 `maintenance.check_db`

- **Setup:** standard.
- **Timed:** Check Database as the command runs it (`collection-check.md` K3 step 7): the integrity check, the optimize step (the file rebuilt and its statistics refreshed), then the repairs and rebuilds of K5 (in one transaction), until it returns. The backup the desktop client makes before it is not part of it.
- **Check:** the K5 object (`collection-check.md` K5, all twelve kinds), or `{"error": "corrupt"}` when the integrity check found the file corrupt.

### T6.9 `package.apkg_export`

- **Setup:** standard.
- **Timed:** exporting the study deck with its subdecks to a new file in the run directory, in the current package format (not the option for older clients), with scheduling information (learning progress) and deck presets, without media, until the file is completely written and closed: the notes, cards, review history, decks, presets and note types gathered, written into a temporary collection, which is compressed into the package with the package's other entries.
- **Check:** `entries`, the package's entry names sorted by code point (for Anki 26.9.3: `collection.anki2`, `collection.anki21b`, `media`, `meta`); `notes`, the number of notes exported.

### T6.10 `package.colpkg_export`

- **Setup:** standard.
- **Timed:** exporting the whole collection as a collection package to a new file in the run directory, in the current format, without media, until the file is completely written and closed. The collection is closed to be exported (its WAL folded in); opening it again afterwards is not part of the interval.
- **Check:** `entries`, as T6.9 (for Anki 26.9.3 the same four names).

### T6.11 `package.apkg_import`

- **Setup:** the T5.4 package (built once per command run); a new collection (T5.3), open.
- **Timed:** importing the package into the new collection with: learning progress (scheduling) included, deck presets included, note types not merged, notes and note types updated only when the package's are newer; until the import returns, committed. It reads the package's collection, gathers every note, card, deck, preset, note type and review-log row, and adds them to the target.
- **Check:** `notes_added`, the number of notes the import reports as new; `cards` and `revlog`, the target's row counts afterwards.

### T6.12 `sync.full_upload`

- **Setup:** standard; the stub (T5.5) running.
- **Timed:** a one-way upload, as after the user chose to upload in the full-sync choice, from its start until the collection is open again and can serve requests: the client prepares the collection for upload (every pending object marked as synced, deletions forgotten, its update sequence number raised and its schema and last-sync times set, `sync-ops.md` S7.3), optimizes the file, closes the collection, reads the file, compresses it, sends it to `/sync/upload`, reads the stub's `OK`, and reopens the collection. The network is loopback and the stub does nothing, so almost all of the interval is the client's local work.
- **Check** (read after the interval): `cards`, the rows of `cards`; `graves`, the rows of `graves`; `pending`, the rows whose `usn` is -1 in `notes`, `cards`, `revlog`, `decks`, `deck_config`, `notetypes` and `tags` together.

### T6.13 `sync.full_download`

- **Setup:** standard; the stub (T5.5) running, its download content prepared.
- **Timed:** a one-way download, as after the user chose to download, from its start until the collection is open again: the client closes its collection, requests `/sync/download`, receives and decompresses the image's bytes, writes them to a temporary file, checks that file's integrity, records it as just synced (its last-sync time set to its modification time), replaces the collection file with it and reopens the collection.
- **Check** (read after the interval): `cards` and `revlog`, the rows of the reopened collection.

### T6.14 `fsrs.recompute_memory_states`

- **Setup:** standard; then FSRS is switched off by the save of T6.14.1 with FSRS off (which clears the memory states of the reviewed cards, or changes nothing when FSRS was already off).
- **Timed:** the save of T6.14.1 with FSRS on. Switching FSRS on computes, for every reviewed card in a normal deck, its memory state from its review history with its preset's FSRS parameters, and writes it to the card (`model/card-data-json` C2 `s`, `d`, `dr`, `decay`), without rescheduling; the interval ends when the save returns, committed.
- **Check** (read after the interval): `memory_states`, the number of cards whose `data` holds a memory state (`s` and `d` both present, `model/card-data-json` C3 rule 4).

#### T6.14.1 The deck options save

The save the deck options screen makes for the default deck (id 1) when the user changes only the FSRS switch: every preset as stored, the default deck keeping its preset, the default deck's own limits as stored, every other collection-wide option as stored, rescheduling of cards off, and the FSRS switch (config key `fsrs`) set to the value the step names.

## T7. Report

### T7.1 `timing.json`

One JSON object, `conformance/schema/timing.schema.json`, in the goldens' text layout (`dsl.md` D11). Both sides write it; `side` says which.

```json
{
  "collection": {"cards": 10012, "decks": 38, "graves": 0, "notes": 8701, "notetypes": 27, "presets": 9, "revlog": 80310, "tags": 167},
  "now": 1894701600,
  "operations": [
    {
      "check": {"cards": 10012, "notes": 8701, "revlog": 80310},
      "name": "collection.open",
      "samples_ms": [41.207, 40.883, 41.95, 40.512, 41.031],
      "statistic": {"median_ms": 41.031, "min_ms": 40.512},
      "status": "ok"
    },
    {
      "error": {"kind": "no_input", "stage": "setup"},
      "name": "review.answer",
      "status": "error"
    }
  ],
  "oracle": {"anki_version": "26.9.3", "driver_sha256": "<hex>", "lock_sha256": "<hex>"},
  "platform": "macos-arm64",
  "protocol": {"repeat": 5, "warmup": 1},
  "report_version": 1,
  "side": "anki",
  "tz": "Etc/GMT-1"
}
```

- `report_version`: 1.
- `side`: `anki` or `meguri`.
- `platform`: where the side ran, as an oracle worker's hello names it: `macos-arm64` (the native lane), `linux-arm64` or `linux-x86_64` (the Docker lane).
- `oracle` (side `anki` only): the pin of the lane that ran: `anki_version`, `driver_sha256`, and `lock_sha256`, the SHA-256 of that lane's lock file (`oracle/requirements.lock` in Docker, `requirements-macos.lock` natively; `conformance/oracle.json` `lock_sha256` and `macos_lock_sha256`).
- `meguri` (side `meguri` only): `commit`, the 40-hex git commit Meguri was built from, or `null` when the build has none; `profile`, the Cargo profile (`release`).
- `protocol`: `warmup`, always 1; `repeat`, the timed repetitions (T1).
- `now`: the frozen instant in epoch seconds; `tz`: the zone used.
- `collection`: T2.2, counts only.
- `operations`: one entry per requested operation, in catalogue order (T6), each T7.2.

Nothing in the report names a deck, tag, field, note, file or directory: a report from the user's collection holds only counts, times, operation names and build identifiers (R1940). It still stays in its run directory (T12).

### T7.2 An operation entry

| Member | When | Value |
|---|---|---|
| `name` | always | the T6 name |
| `status` | always | `ok`, `unsupported` or `error` |
| `samples_ms` | `ok` | the `repeat` timed samples (T4.2), in the order taken; the warm-up's is not included |
| `statistic` | `ok` | `{median_ms, min_ms}` (T4.3) |
| `check` | `ok` | the T6 check of the runs (all equal, T3.6) |
| `error` | `error` | `{kind, stage}`, plus `exception` when a call raised: the class name of what was raised, never its message |

- `unsupported`: the side has no implementation of the operation. Meguri reports it for every operation its engine cannot run yet (it becomes `pending` in the comparison, T11); the oracle supports the whole catalogue, so an `unsupported` from it is a defect (T9). No setup runs for it.
- `error` kinds:
  - `no_input`: the collection lacks what the operation needs (T6.4: the study deck's queue is empty);
  - `failed`: a call of the run raised or returned an error;
  - `unstable`: a run's check differed from the first run's (T3.6).
- `stage`: `setup`, `timed` or `check`, where it happened (`unstable` is always `check`). The first failing run ends the operation; the next operation still runs.
- Messages, which may quote collection content and are localized (R1887), never appear.

## T8. Errors

When the run cannot produce a report, the job status is `error` with one failure whose kind is:

| Kind | When |
|---|---|
| `usage` | T1 |
| `no_collection` | `<dir>/collection.anki2` does not exist |
| `open_failed` | the summary open of T2.2 failed (too old, too new, not a collection) |
| `clock_unsupported` | the worker cannot freeze the clock (T2.3) |
| `oracle_error` | any other failure of the worker or the harness (`detail` is driver text, never compared) |

An operation that fails is not a run error: it is reported as T7.2 `error` and the run goes on.

## T9. Job result

- `result.json` (`tools/AGENTS.md`) lists the report as an artifact of kind `report`, `timing.json` in the run directory, and repeats it under `details.timing`.
- Status: `pass` when every reported operation is `ok`; `fail` when one is `error` or `unsupported`, with one failure per such operation (its id is the operation name, its message names the status and the error kind); `error` per T8.
- `--platform apple` first makes sure the native lane is installed, as the other native commands do (`oracle/macos/README.md` is the oracle maintainer's).

## T10. The Meguri side

`meguri-cli timing run` takes the same collection directory and the same `--op`, `--now`, `--tz` and `--repeat` with the same defaults (T1), follows T2 to T6 through Meguri's engine, and writes the same report (T7) with `side` `meguri` and the `meguri` member instead of `oracle`. It needs no Anki and reads nothing of the oracle's.

- An operation its engine cannot run yet is `unsupported`, without setup, samples or check.
- For a comparison it runs on the same machine as the oracle run and in the same lane: natively on the Mac for `--platform apple`, in the Linux dev container for the Docker lane, with its copies on the file system kind of T3.4.
- Its stub (T5.5) and package (T5.4) are its own, built by the same rules.

## T11. Comparing reports

- An `anki` report and a `meguri` report are comparable when they have the same `platform`, `now`, `tz`, `protocol` and `collection`.
- For one operation the ratio is Meguri's `median_ms` divided by Anki's, computed only when both are `ok` and their `check` objects are equal (member by member).
- Meguri `unsupported` (Anki `ok`): no ratio; the operation is pending.
- Checks that differ: no ratio. The two sides did different work (often a conformance difference in the operation itself), which is a finding to investigate, never a timing.
- Either side `error`: no ratio.

What the job does with ratios (the failure rule that a ratio of 1 or more fails, R1806, the history file and the private mode) is `tools/mg perf-ratio`'s, U18.

## T12. No goldens

- Timings depend on the machine and are never recorded as goldens: nothing from this command goes under `conformance/goldens/` or `conformance/goldens-apple/`.
- A report from the user's collection stays in its run directory (`build/private/` through the vault, U15). Only the aggregate rows that `tools/mg perf-ratio` writes reach the repository.
