# Synthetic collections

Spec: fixtures/synthetic-collections
Requirements: R1804, R1805; supports R1803, R1940, R1959, R1963
Anki: 26.09.3
Scenarios: none. Generated collections are checked with the oracle collection check (`specs/conformance/collection-check.md`), not with scenarios and goldens (F11).

Meguri's generator writes seeded collections whose shape follows a profile of the user's collection (R1804), the worst-case shapes (R1805), and small named UI fixtures (R1963). This file is the shape model (F2 to F4), the rows a generated collection holds and why each is what Anki itself would hold (F5, F6), the validity rules Anki's Check Database checks (F7), and the worst-case, recipe and output formats (F8 to F10). Day numbers, ids and stamps are those of `model/row-records`, `model/card-data-json` and `storage/ids-usn-mtime`.

## F1. Terms

- **Profile**: aggregate histograms of one collection (F2). `core/crates/meguri-testkit/profiles/user-shape.json` is the committed profile of the user's collection.
- **Request**: a profile, a number of cards, a seed, a pinned instant `now`, a media mode, and optionally a worst case (F8).
- **Pinned day**: the day that holds `now` (F4.3). "Today" below always means the pinned day, numbered `today`.
- **Answer**: one review of one card: a button (1 Again, 2 Hard, 3 Good, 4 Easy) at an instant. Each answer is one `revlog` row.

## F2. Profile format

### F2.1 File

- A profile is a JSON object, stored in the goldens' text layout (`specs/conformance/dsl.md` D11: sorted keys, two-space indents, arrays of scalars on one line, LF, final newline). `profile_sha256` is the lower-case hex SHA-256 of that text.
- `format` is 1. A reader refuses another value, unknown members and missing members.
- `provenance` is `provisional` (written from the plan's published aggregates), `extracted` (F2.6, from the user's collection) or `measured` (F2.5 on any other collection, such as a generated one).

### F2.2 Histograms and mixes

- A histogram is `{"edges": [e0, e1, ..., en], "counts": [c0, ..., c(n-1)]}`: integers, edges strictly increasing, at least two edges, one count fewer than edges. Bucket `i` holds the integers from `e(i)` up to but not including `e(i+1)`. A value below `e0` counts in the first bucket; a value at or past `en` counts in the last.
- Edge layouts used below:
  - `linear(a, b, s)`: `a`, `a+s`, `a+2s`, ... while the edge is at most `b`, then one more edge (the first one past `b`). `linear(1, 8, 1)` is 1 to 9; `linear(0, 500, 7)` is 0, 7, ..., 497, 504.
  - `log2(k)`: 0, 1, 2, 4, 8, ..., 2^k, then 2^k + 1. `log2(2)` is 0, 1, 2, 4, 5.
- Drawing from a histogram: a bucket with probability proportional to its count, then an integer uniformly in `[e(i), e(i+1))` (in `[e(i), e(i)+1)` when that range is empty). A histogram whose counts are all zero gives nothing. Drawing within `[lo, hi)`: draw until the value lies in the range, at most 64 times, then a uniform integer in the range.
- A mix is an object of category name to count. Drawing picks a category with probability proportional to its count.

### F2.3 Members

| Member | Meaning (as `measure` computes it, F2.5) |
|---|---|
| `format`, `provenance` | F2.1 |
| `utc_offset_secs` | the UTC offset, seconds east, that hours of day are read in; for an extraction, the zone's offset at the last note creation or review |
| `rollover_hour` | config `rollover`; 4 when missing or outside 0 to 23 |
| `totals.notes`, `.cards`, `.revlog` | row counts |
| `totals.notetypes`, `.presets` | row counts of `notetypes` and `deck_config` |
| `totals.decks` | normal decks, the default deck included |
| `totals.filtered_decks` | filtered decks |
| `totals.tags` | distinct tags on notes, compared with `text/unicode-primitives#U2` (`unicase`) |
| `totals.media_files`, `.media_bytes` | regular files in the media folder and their total size |
| `totals.span_days` | day number (F4.3, counted from `col.crt`) of the last note creation or review; 0 when there is neither |
| `notetypes` | one shape per note type, in id order (below) |
| `decks.depth` | `linear(1, 8, 1)`: normal decks by number of name components |
| `decks.cards_per_deck` | `log2(20)`: cards per normal deck, each card counted in its home deck (`odid` when not 0, else `did`), every normal deck counted once, empty ones included |
| `decks.decks_per_preset` | normal decks per preset, one entry per preset (0 for an unused one), largest first |
| `tags.depth` | `linear(1, 8, 1)`: distinct tags by number of `::` components |
| `tags.per_note` | `linear(0, 16, 1)`: tags per note |
| `media.file_bytes` | `log2(30)`: media file sizes |
| `media.kinds` | media files by extension, case-insensitive: `image` (png jpg jpeg gif webp svg bmp tif tiff avif heic), `audio` (mp3 ogg oga opus wav m4a aac flac spx), `video` (mp4 webm mov mkv ogv m4v avi mpg mpeg), `other` |
| `cards.states` | mix over all six of `new` (queue 0), `learning` and `relearning` (queues 1, 3, 4; `relearning` when type is 3), `review` (queue 2), `suspended` (queue -1), `buried` (queues -2, -3); zero counts included |
| `cards.interval_days` | `log2(15)`: `ivl` of type-2 cards |
| `revlog.per_card` | `linear(1, 64, 1)`: rows per card, over cards with at least one row |
| `revlog.kinds` | mix of rows by `type`: `learning`, `review`, `relearning`, `filtered`, `manual`, `rescheduled` (0 to 5) |
| `revlog.ratings` | per kind name, five counts: rows with `ease` 0, 1, 2, 3, 4 (an `ease` above 4 counts as 4) |
| `revlog.answer_ms` | `log2(20)`: `time` |
| `revlog.review_days` | `linear(0, span_days, 7)`: rows by day number |
| `revlog.hour_of_day` | `linear(0, 23, 1)` (24 buckets): rows by local hour, through the offset at each row's instant |
| `revlog.first_review_gap_secs` | `log2(32)`: per card with rows, seconds from its note's id to its first row's id (negative counts as 0) |
| `notes.unreviewed_creation_days` | `linear(0, span_days, 7)`: creation day numbers of notes none of whose cards has a row |

A note type shape:

| Member | Meaning |
|---|---|
| `kind` | `normal` or `cloze` (notetype config `kind` 1) |
| `stock_kind` | the stored `original_stock_kind` (`model/protobuf-records` P6.1); informational, the generator ignores it (F6.5) |
| `fields`, `templates` | counts |
| `notes`, `cards` | notes of this type and their cards |
| `css_bytes` | bytes of the CSS |
| `template_bytes` | per template, `[front bytes, back bytes]` |
| `script_bytes` | bytes between `<script...>` and `</script>` (case-insensitive) over every template side |
| `cards_per_note` | `linear(1, 64, 1)` |
| `field_bytes` | per field, `log2(24)` of the field's UTF-8 bytes: the whole stored value, markup and media references included |
| `media_refs` | per field, `[image references, sound references]`: media tags and `[sound:...]` tags naming a local file (a remote URL or a `data:` URL does not count) |
| `refs_per_note` | `linear(0, 64, 1)`: local references per note; informational, the generator places references by field (F4.8) |

Notes and cards whose note type is missing count in the totals only.

### F2.4 Structural checks

A profile is refused unless: `format` is 1; `rollover_hour` is 0 to 23; `utc_offset_secs` is a whole number of minutes strictly between -24 h and +24 h; every histogram follows F2.2; `revlog.hour_of_day` has 24 buckets; there is at least one note type; each note type has kind `normal` or `cloze`, at least one field and one template, one `field_bytes` and one `media_refs` entry per field and one `template_bytes` entry per template.

### F2.5 Measuring

`measure` computes a profile from any collection file (opened through the storage open gate, so pass a copy) and optionally its media folder, given a function that returns the UTC offset at an instant. It reads only counts and lengths. Comparisons with a reference profile (F3) need equal edges: the day histograms (`review_days`, `unreviewed_creation_days`) of a generated collection are measured with the reference profile's `span_days`, never with a span recomputed from the generated rows, which may end a day earlier.

### F2.6 Extraction and privacy (R1940)

- `meguri-cli profile-extract <collection> --media-dir <dir> --tz <IANA zone> --out <file>` copies the collection file (and its `-wal`) to a temporary directory, measures the copy with the zone's offset at each instant, sets `provenance` to `extracted`, and writes the profile. It never opens the original. Only a role allowed to read `local-data/` runs it on the private collection.
- A profile holds counts, numbers and these strings only: its fixed member names, and the values `provisional`, `extracted`, `measured`, `normal`, `cloze`, `new`, `learning`, `review`, `relearning`, `suspended`, `buried`, `filtered`, `manual`, `rescheduled`, `image`, `audio`, `video`, `other`. No text, name or id of the source collection may appear in it.

## F3. Following a profile ("within 5%")

A generated collection follows its profile when, measured per F2.5:

- Histograms and mixes are within a total variation distance of 0.05: half the sum, over buckets (over the union of categories for a mix), of the absolute difference of the two shares. Shares of an empty histogram are all 0. Histograms with different edges have no distance and fail.
- Means (review-log rows per card: `totals.revlog / totals.cards`) are within a relative error of 5% of the profile's.
- The card count is within 5% of the request.
- Exact: the preset count, and FSRS, load balancer and sibling burying on (F6.2, F6.3).

The checks of the plan's test scenarios: at 10k cards, cards per note type (the mix of each note type shape's `cards`, keyed by its index), card states and reviews per card; at 100k cards, `review_days`, `hour_of_day` and `first_review_gap_secs`; at both sizes, field lengths (F3.1). Two seeds give the same aggregates within the same distances.

### F3.1 Field lengths

Field lengths are compared at 10k and at 100k cards. The generated collection's `field_bytes` are measured per F2.5; its note types in id order are the profile's shapes in order (F4.7, F6.5). The expected histogram of a field is its profile `field_bytes` with one change: for the first field of a normal note type, the count of the first bucket (0 bytes) is added to the bucket that holds 1 and the first bucket becomes 0, because F4.8 writes those fields as 1 byte. Each comparison is a total variation distance of at most 0.05:

- Pooled: the sum, over every note type and every field, of the field's expected histogram as shares times its note type's `notes`, compared with the same sum of the generated collection's measured histograms times its measured `notes`.
- Per field: every field of every note type that has at least 2,000 notes in the generated collection, compared with its expected histogram.

Smaller note types count only in the pooled comparison. Sampling alone puts about 0.03 of distance on a field of 1,000 notes, so among twenty such fields one passes 0.05 in about one seed in thirty; from 2,000 notes the worst field stays below about 0.035 in nineteen seeds of twenty. The cloze fields F4.8 lengthens (shorter than their cloze markup) stay in the expected histogram unchanged: they are few, and the distance absorbs them.

## F4. Generation model

### F4.1 Randomness

Every draw comes from a ChaCha stream seeded through the randomness seam (R1875) from the run's seed, a stream family and an index: one structure stream (presets, decks, note types, tags, note type assignment), one stream per note (by generation index), one per media file, one per recipe note. A note's rows depend only on the seed and its index. The same request always writes the same rows (F10.3).

### F4.2 Scale

`scale = requested cards / totals.cards`. Counts drawn from totals (notes per note type, distinct tags) are multiplied by `scale` and rounded, at least 1 when the profile's count is not 0.

### F4.3 Timeline

- The offset is the profile's `utc_offset_secs`, fixed for the whole collection; the rollover hour is the profile's.
- The pinned day starts at the last local rollover at or before `now`. `today` is the profile's `span_days` (at least 1, at most 36,500). `col.crt` is the pinned day's start minus `today` × 86,400 s, so day `d` covers `[crt + 86,400 d, crt + 86,400 (d + 1))` and `now` lies in day `today`.
- An instant on day `d` at local hour `h`: `crt + 86,400 d + 3,600 × ((h - rollover) mod 24)` seconds plus a uniform number of seconds below 3,600 and milliseconds below 1,000. An instant after `now` is refused and drawn again.
- `now` must be before 2038-01-19T03:14:07Z minus one day, so every `due` in epoch seconds fits `i32`.

### F4.4 No answer on the pinned day

Every answer falls before the pinned day's start (the last answer day is `today - 1`). So a generated collection is one whose user has not studied yet today, and the decks' "studied today" counters in `decks.common` (P6.4 `last_day_studied`, `new_studied`, `review_studied`, `learning_studied`, `milliseconds_studied`) correctly stay at their defaults. Notes may still be created on the pinned day.

### F4.5 Presets, decks, note types, tags

- Presets: `totals.presets` of them (at least 1): `Default` (id 1), then `Preset 2`, `Preset 3`, ... (F6.3).
- Decks: the default deck (id 1, `Default`), then `totals.decks - 1` more (the worst case's `decks` when given). Each draws a depth from `decks.depth`, lowered until a deck one level up exists, and takes a parent uniformly among the decks one level up. Names are `Deck <n>` at the top level and `<parent>::Part <n>` below (`n` the deck's number from 1). Each takes a preset drawn by the weights `decks_per_preset[i]` (1 when missing or 0) and a weight drawn from `cards_per_deck`; the default deck's weight is drawn too. A note's home deck is drawn by these weights (all 1 when every weight is 0).
- Note types: one per profile shape (F6.5).
- Tags: a pool of `round(totals.tags × scale)` distinct names. Each draws a depth from `tags.depth` (clamped to 1 to 6) and builds `topic<i>::area<j>::unit<k>::part<l>::item<m>::leaf<n>` truncated to that depth, each number uniform below `max(2, ceil(sqrt(pool size)))`; a name already in the pool (case-insensitively) gets the suffix `_<pool index>`. A note's tags: a count drawn from `tags.per_note`, then that many uniform picks from the pool, duplicates dropped.

### F4.6 Notes

- Note type assignment: each note type gets `round(notes × scale)` notes (at least 1 when its `notes` is not 0), concatenated in note type order, then shuffled (Fisher-Yates from the end with the structure stream). The note's generation index is its place in that order.
- Card count: a normal note type's template count; a cloze note type draws from its `cards_per_note` within 1 to 64.
- Each card draws a target state from `cards.states` (F5.2).
- Review instants: the note's first card whose target is not `new` (the lead) draws a count from `revlog.per_card` (at least 1) and that many instants, each a day from `review_days` within `[0, today - 1]` and an hour from `hour_of_day`; duplicates are dropped and the rest sorted. The note is created a gap before the lead's first answer: a gap from `first_review_gap_secs` within `[1, room)` seconds, `room` being the seconds from `crt + 1 s` to that answer, minus a uniform number of milliseconds below 1,000. Each other non-new card starts a gap (drawn the same way, within the seconds left before the pinned day's start) after the note's creation, then draws its other instants on days from that first day on, keeping only instants after its first. A card for which no first answer fits becomes `new`; when the lead gets no instant at all, every card of the note becomes `new`.
- A note none of whose cards has a target other than `new` is created at an instant on a day drawn from `notes.unreviewed_creation_days` within `[0, today]` and an hour from `hour_of_day`; it may fall on the pinned day, before `now`.

### F4.7 Ids

- Note id: the creation instant in ms. Card id: the creation instant plus the card's ordinal. Review-log id: the answer instant, raised to the previous row id of the same card plus 1 when not above it.
- A candidate already taken in its table moves up by 1 until free (this is the generator's own rule; `storage/ids-usn-mtime` I2 does not apply to written rows, I4).
- So a note id is at most its first card's id, a card id is below its first review-log id, and no id of `notes`, `cards` or `revlog` exceeds `now` in ms (F7).
- Presets other than `Default`, decks other than the default deck, and note types take `crt` in ms plus a small index (their place in their table's generation order), moved up when taken. They are not compared with anything; only uniqueness matters.

### F4.8 Fields and media references

This section builds the fields of profile and worst-case notes; recipes build theirs per F9. Fields are ASCII.

- Length: each field draws a length `L` from its `field_bytes`. `L` is the byte length of the whole stored field, markup included, as `measure` reads it (F2.3). A normal note type's first field drawn at 0 is 1 byte instead, so every normal front shows a non-empty field (F7); other fields drawn at 0 are empty. (Version 2 raised every first field to at least 4 bytes; that moved every profile's empty and short first fields out of their buckets.)
- Words: 1 to 3 syllables from a fixed list of 24 two-letter syllables, separated by spaces, the whole cut to the number of bytes asked for (it may end with a space).
- Markup: a cloze note's first field holds its cloze deletions `{{c1::<answer>}}` to `{{cN::<answer>}}` (N its card count), each answer words of a length drawn from 3 to 14 bytes. When the deletions with these answers, joined by single spaces, are longer than `L`, every answer is 1 byte instead. After the deletions come the field's media references (below), in the order they are written.
- Text: the cloze deletions, then the words, then the references, joined by single spaces. The words take the bytes left: `L` minus the markup (its pieces and the single spaces between them) minus one separating space. When fewer than 2 bytes are left (no room for the space and one letter), the field is its markup alone, one byte short of `L` when exactly 1 was left. When the markup alone is longer than `L`, the field is still the markup, longer than drawn: this happens only to a cloze field shorter than its deletions with 1-byte answers and to the `media_files` worst case. A field without markup is `L` bytes of words.
- References by field, in every profile and worst-case collection but the `media_files` worst case: a field whose `media_refs` is `[images, sounds]` with `images + sounds > 0` has the rate `r = (images + sounds) / (notes × q)`, `notes` being its shape's and `q` the share of its `field_bytes` outside the first bucket (lengths of 1 byte or more); a field with `q` = 0, or of a shape whose `notes` is 0, takes no reference. A note's field whose `L` is not 0 draws `floor(r)` references, and one more with probability `r - floor(r)`; each is an image with probability `images / (images + sounds)`, otherwise a sound. A reference is written only when it fits: the field's markup so far, one separating space when the field already holds markup, and the reference take at most `L` bytes together. A reference that does not fit is not written: no name, no file. So a non-empty media field holds references at the profile's rate and is never lengthened by them; the cloze deletions are placed before the references. The profile's `refs_per_note` is not drawn from: a per-note count drawn apart from the field lengths puts references into fields drawn empty or short, which either lengthens them or loses the references.
- References in the `media_files` worst case (F8): each note holds the count F8 gives it. Each reference goes to a field drawn by the weights `images + sounds` of `media_refs` (the last field when every weight is 0) and is an image with probability `images / (images + sounds)` of that field (an image and a sound alternately, an image first, when both are 0). All are written, fitting or not.
- A reference: fields in order and references in the order drawn, numbered `n` from 0 across the note, counting written references only. An image is the file `mg<note index in lower-case hex>-<n>.png` written `<img src="<file>">`; a sound is `mg<note index in lower-case hex>-<n>.mp3` written `[sound:<file>]`. Its size is drawn from `media.file_bytes` (from the worst case's `bytes` for `media_files`), at least 128 bytes.
- Worst cases (F8): a large field is exactly `bytes` bytes of words and takes none of the references it would otherwise draw; inline images are appended to the first field after its text, beyond its drawn length.
- Media mode `references` writes no file (sizes at 100k cards and above would reach tens of gigabytes); `files` writes the file of every written reference (F4.9).

Examples, for note index 7,978 (`1f2a`), so that reference 0 as an image, `<img src="mg1f2a-0.png">`, is 24 bytes:

| Field | `L` | Markup | Stored field | Bytes |
|---|---|---|---|---|
| a normal note type's second field | 0 | none | empty | 0 |
| a normal note type's first field | 0 | none | one letter | 1 |
| a field with the image reference | 40 | the reference | 15 bytes of words, a space, the reference | 40 |
| the same | 25 | the reference | the reference alone (1 byte left) | 24 |
| the same | 20 | none: the reference does not fit and is not written | 20 bytes of words | 20 |
| a cloze first field, N = 2, answers drawn 5 and 8 bytes | 60 | `{{c1::<5>}} {{c2::<8>}}`, 13 + 1 + 16 bytes | the deletions, a space, 29 bytes of words | 60 |
| the same, answers drawn 5 and 8 bytes | 20 | 30 bytes is longer than 20, so `{{c1::<1>}} {{c2::<1>}}`, 9 + 1 + 9 bytes | the deletions alone (1 byte left) | 19 |
| a cloze first field, N = 3 | 10 | `{{c1::<1>}} {{c2::<1>}} {{c3::<1>}}`, 29 bytes | the deletions alone | 29 |

### F4.9 Media files

A file's bytes depend only on the seed and its index. An image is a valid PNG of exactly its size when that is at least the smallest one (a 1 × 1 image, otherwise that smallest image): RGB, 8 bits, 32 pixels wide, zlib stream of stored (uncompressed) deflate blocks, padded to its size with a private ancillary chunk named `mgPd` before `IEND`. A sound is random bytes from its stream (not decodable audio).

## F5. Review histories and card state

Histories come first; card columns, card data and review-log columns follow from them as Anki's scheduler with the generated presets (F6.3: FSRS on, learning steps 1 min and 10 min, relearning step 10 min, desired retention 0.9) would have written them.

### F5.1 Answer table

Before each answer a card is new, at learning step 1 or 2, in review, or relearning (its one step). An answer's review-log kind is the phase it starts in: 0 for new and learning, 1 for review, 2 for relearning.

| Before | Again (1) | Hard (2) | Good (3) | Easy (4) |
|---|---|---|---|---|
| new, learning step 1 | step 1, 60 s | step 1, 330 s | step 2, 600 s | review, F days |
| learning step 2 | step 1, 60 s | step 2, 600 s | review, F days | review, F days |
| review | relearning, 600 s (a lapse) | review, F days | review, F days | review, F days |
| relearning | relearning, 600 s | relearning, 900 s | review, F days | review, F days |

- A delay in seconds that reaches the next day boundary (the answer instant plus the delay is at or past the start of the next day) becomes 1 day.
- F days: the FSRS interval of the memory state after that answer (F5.3).

### F5.2 Target states and buttons

- A card's target is one of `new`, `learning` (the history never leaves learning), `relearning` (it ends relearning), `review`, `suspended` and `buried` (all three end in review).
- Buttons are drawn for each answer from `revlog.ratings` of the answer's kind (buttons 1 to 4; Good alone when the kind has no counts), restricted to the buttons from which the target can still be reached with the answers left. The history obeys F5.1 and ends in the target's phase.
- A `relearning` target with fewer than two answers ends `learning` instead; one answer can end `review` only by Easy.

### F5.3 FSRS memory states and intervals

- Memory states come from fsrs 6.6.2 with its default parameters: its historical memory-state computation over the card's answers in order, each with its button and the whole days since the previous answer (the difference of day numbers, 0 on the same day, 0 for the first). The state after answer `i` belongs to row `i` and the last one to the card. fsrs never refuses a history this generator makes; a refusal is a generation error, never patched over.
- The FSRS interval of a memory state: fsrs's next interval for its stability at desired retention 0.9, rounded to the nearest day, at least 1 and at most 36,500.
- `decay` is fsrs-6's default decay, 0.1542 (`model/card-data-json` C4 writes 0.154).

### F5.4 Ease (permille)

New and learning cards (never graduated) keep 0. The answer that first takes a card to review sets 2500 (the preset's starting ease 2.5). Each answer of kind review then adds -200 (Again), -150 (Hard), 0 (Good) or +150 (Easy); after Again or Hard the result is at least 1300. Learning and relearning answers leave it.

### F5.5 Card columns

| Final state | `type` | `queue` | `due` | `ivl` | `left` |
|---|---|---|---|---|---|
| new | 0 | 0 | position (F6.6) | 0 | 0 |
| learning step 1 or 2 | 1 | 1 | last answer instant (s) + its delay | 0 | 2 at step 1, 1 at step 2 |
| relearning | 3 | 1 | last answer instant (s) + its delay | F days of the last memory state | 1 |
| review | 2 | 2 | last answer's day + `ivl` | last answer's F days (adjusted below) | 0 |
| suspended | 2 | -1 | as review | as review | 0 |
| buried | 2 | -3 | as review | as review | 0 |

- A learning or relearning card whose last delay became 1 day (F5.1) has queue 3 and `due` = last answer's day + 1.
- Buried cards are buried by the user (queue -3) on the pinned day, which F4.4 allows and config `lastUnburied` (F6.2) keeps: queue -2 (buried by the scheduler) would need a sibling answered today.
- Due targets (worst case `due_deck`, recipe `due` and `later`): a card that must be due on or before today takes `ivl = max(1, min(F, today - last day))`; one that must be due after today takes `ivl = max(F, today - last day + 1)`; both at most 36,500.
- `factor` per F5.4. `reps` the number of answers; `lapses` the number of review-kind Again answers. `odue`, `odid`, `flags` 0. `usn` -1. `mod`: the last answer's instant in seconds (new cards: the note's creation second).
- `data` (`model/card-data-json`): new cards `{}`; others `pos` (the card's position), `s` and `d` (the last memory state), `dr` 0.9, `decay` (F5.3), `lrt` (the last answer's instant in seconds, rounded down), written per C4.

### F5.6 Review-log columns

| Column | Value |
|---|---|
| `id` | the answer instant in ms (F4.7) |
| `cid` | the card |
| `usn` | -1 |
| `ease` | the button |
| `ivl` | the answer's delay per F5.1: minus the seconds (`-60`, `-330`, `-600`, `-900`), or the days (1 for a delay that became a day, F days otherwise); for the card's last answer, equal to the card's `ivl` when it ends in review |
| `lastIvl` | the previous row's `ivl` for this card; 0 for its first answer |
| `factor` | the FSRS difficulty `D` (1 to 10) after the answer, mapped in 32-bit float arithmetic, each step rounded to `f32`: `n = (D - 1) / 9`, `s = n + 0.1`, `factor = round(s × 1000)` with halves away from zero; so 100 to 1100 (`model/row-records` R5) |
| `time` | drawn from `revlog.answer_ms`, at most 60,000 (the presets' `cap_answer_time_to_secs` 60: Anki never records more) and at least 0 |
| `type` | the answer's kind (F5.1) |

Review instants are drawn from the profile, not from the intervals, so an answer may come before the previous row's interval has passed. This keeps the temporal shape and breaks no rule Anki checks.

## F6. Collection rows

### F6.1 `col`

`id` 1, `crt` (F4.3), `mod` = `now` in ms, `scm` = `crt` in ms, `ver` 18, `dty` 0, `usn` 0, `ls` 0, and the empty string in `conf`, `models`, `decks`, `dconf`, `tags`. A never-synced collection: `scm` above `ls` makes its first sync a full one.

### F6.2 `config`

Every row: `usn` -1, `mtime_secs` = `crt`, `val` the compact JSON text (`storage/config-table` C5). The keys are those a new Anki 26.09.3 collection holds, plus the scheduling keys Anki writes as it is used:

| Key | Value | Why |
|---|---|---|
| `activeDecks` | `[1]` | new collection |
| `addToCur` | `true` | new collection |
| `collapseTime` | `1200` | new collection (learn-ahead seconds) |
| `creationOffset` | minutes west of UTC: `-utc_offset_secs / 60` | new collection; without it Anki's desktop client writes it on open and Anki computes day numbers in its legacy way |
| `curDeck` | `1` | new collection |
| `curModel` | id of the first generated note type | new collection (its first note type) |
| `dayLearnFirst` | `false` | new collection |
| `dueCounts` | `true` | new collection |
| `estTimes` | `true` | new collection |
| `fsrs` | `true` | R1804 |
| `lastUnburied` | `today` | Anki writes the day it last unburied cards; `today` keeps the buried cards (F5.5) buried at `now` |
| `loadBalancerEnabled` | `true` | R1804 |
| `newSpread` | `0` | new collection |
| `nextPos` | number of notes + 1 | the next new-card position (F6.6) |
| `rollover` | the profile's (recipe's) rollover hour | Anki writes 4 on first use when missing |
| `sched2021` | `true` | new collection; without it Anki's desktop client asks for a scheduler update before it lets the user study |
| `schedVer` | `2` | new collection; the open gate refuses the v1 scheduler (R1482) |
| `sortBackwards` | `false` | new collection |
| `sortType` | `"noteFld"` | new collection |
| `timeLim` | `0` | new collection |

### F6.3 `deck_config`

The new-collection default preset of `model/protobuf-records` P6.6 (the example message) with `bury_new`, `bury_reviews` and `bury_interday_learning` true (R1804) and `desired_retention` 0.9; `name` `Default` (id 1) or `Preset <n>`; `mtime_secs` = `crt`; `usn` -1. The due worst case sets `reviews_per_day` of `Preset 2` to its review limit; recipes set `new_per_day` and `reviews_per_day` of their own presets.

### F6.4 `decks`

- Normal decks only. `name` in native form (`model/row-records` R7), `mtime_secs` = `crt`, `usn` -1, kind normal with `config_id` the deck's preset.
- `common`: `study_collapsed` and `browser_collapsed` true in profile and worst-case collections, as Anki creates every deck; false in recipes, so UI fixtures show their whole tree. Every other member default (no studied-today counters, F4.4).
- A deck's parent always exists (Check Database creates missing parents). Names are unique case-insensitively.

### F6.5 `notetypes`, `fields`, `templates`

Meguri's own note types, CSS and templates (R1959), never Anki's stock ones:

- `name` `Meguri Synthetic <n>` (recipes: F9.2), id per F4.7, `mtime_secs` = `crt`, `usn` -1.
- Config: `kind` 0 normal or 1 cloze; `sort_field_idx` 0; `css` Meguri's base rules grown with filler rules to the shape's `css_bytes`; `latex_pre` and `latex_post` empty (no generated text uses LaTeX); `original_stock_kind` 0 for every note type, whatever the profile's `stock_kind`: a stock kind would claim Anki's stock templates (Restore to Default) and kind 6 would make Meguri (U63) and Anki's Check Database treat it as an image-occlusion note type; `reqs` one requirement per template, `card_ord` its ordinal, `kind` 1 (any), `field_ords` `[0]`, which is what Anki computes for these templates on every note type save.
- Fields: `Front`, `Back` (normal) or `Text`, `Extra` (cloze), then `Field 3`, `Field 4`, ...; config `font_name` `Arial`, `font_size` 20, `id` a random 64-bit integer from the structure stream, everything else default.
- Templates: a cloze note type has one; a normal one the shape's count, named `Side <n>`, `mtime_secs` 0 and `usn` 0 (as Anki keeps them). Config `id` random from the structure stream; `q_format` shows `{{<first field>}}` (cloze: `{{cloze:<first field>}}`) and the template's number, so fronts differ (Anki refuses identical fronts); `a_format` shows `{{FrontSide}}` and the second field (cloze: the cloze and `{{<second field>}}`). Each side is grown to the shape's bytes: a `<script>` element carrying the side's share of `script_bytes` (`script_bytes / (2 × templates)`), then an HTML comment of filler letters. The template worst case grows the first note type's first front instead (F8).
- Image-occlusion-shaped note types of a profile (cloze, many fields) are generated as plain cloze note types whose notes reference PNG images through `<img>`; real image-occlusion notes (stock kind 6, Occlusion field shapes) wait for U63's format spec.

### F6.6 `notes` and positions

- `guid` the base91 text of a random 64-bit integer (`text/guid`), `mid`, `mod` the creation second, `usn` -1, `flds` the values joined by U+001F, `sfld` and `csum` per `text/checksum-and-sort-field` with sort field 0, `flags` 0, `data` the empty string.
- `tags`: the empty string, or the tags joined by single spaces with one space before and after, ordered by `text/unicode-primitives#U2` (Anki's order after its tag clean-up). Field text and tags are NFC.
- Positions: note `i` in generation order (from 0) has position `i + 1`; all its cards share it (Anki gives one position per added note). Positions stay below 1,000,000: a request needing more than 999,999 notes is refused. `nextPos` is the number of notes + 1.

### F6.7 `tags`

One row per distinct tag on notes, spelled as on the notes, with no row for a parent that no note carries (Anki registers only the tags notes use; the tag tree adds parents when shown). `usn` -1; `collapsed` 1 in profile and worst-case collections (Anki registers a new tag collapsed) and 0 in recipes; `config` NULL.

### F6.8 `graves`

Empty.

## F7. Validity

A generated collection, opened by the oracle at its manifest's `now` in a zone whose offset at `now` is the manifest's `utc_offset_secs`, must give Anki's Check Database zero problems of every kind of `conformance/collection-check` K5, and Check Database must leave every table but `col` and `config` unchanged (K3 step 8; Check Database rewrites `nextPos` and may stamp `col`). With the rules above:

| K5 kind | Rule that keeps it at 0 |
|---|---|
| `card_properties` | integer columns within range; no review-queue `due` above 100,000; `odue` and `odid` 0; `ivl` ≥ 0; `ord` 0 to 30,000 |
| `new_positions_too_high` | positions below 1,000,000 (F6.6) |
| `card_last_review_time` | every non-new card's `lrt` is its last row's instant in seconds |
| `cards_without_note`, `duplicate_card_ords`, `cards_without_template`, `field_counts`, `notetypes_recovered` | cards reference their note, one card per ordinal, ordinals below the template count (normal), field counts equal the note type's |
| `decks_missing` | every `did` and every parent deck exists |
| `revlog_properties` | `ivl` and `lastIvl` in `i32`, `time` 0 to 2,147,483,647, no row of type 0 with `time` 0 and `ease` 0 |
| `notes_invalid_utf8` | text is UTF-8 |
| `future_ids` | no id after `now` |

Unchanged tables additionally need: every card a template or cloze number calls for exists (normal fronts show the non-empty first field; cloze cards for exactly c1 to cN); `sfld`, `csum`, NFC text with no ASCII control character other than tab and newline, and the tag string as Anki computes them; tag rows as F6.7.

Learning cards whose last answer is days old are valid: Anki shows them as due learning cards.

## F8. Worst cases

`core/crates/meguri-testkit/profiles/worst-cases/<name>.json`: the user's shape at a card count, with one stress applied. Members (unknown members refused):

| Member | Meaning |
|---|---|
| `name`, `description` | the file stem; one line |
| `cards` | cards to generate (overridable) |
| `decks` | normal decks, the default deck included |
| `tag_nodes` | a tag tree of this many nodes: node `i` (from 0) is `node<i>` for `i < 10`, else `<node (i - 10) / 10>::node<i>` (ten children per node); note `j` of `N` carries nodes `j`, `j + N`, `j + 2N`, ... besides its drawn tags, so every node is on a note |
| `template_bytes` | the first note type's first front is grown to this many bytes |
| `media_files` | `{count, bytes}`: exactly `count` files (`bytes` a histogram of sizes), always written; note `i` of `N` holds `count / N` references, one more when `i < count mod N` |
| `large_fields` | `{notes, bytes}`: the first `notes` notes (generation order) hold `bytes` bytes of words in their second field (the first when there is one field) |
| `inline_images` | `{notes, per_note, image_bytes}`: the first `notes` notes get `per_note` PNGs of `image_bytes` bytes appended to their first field as `<img src="data:image/png;base64,...">` (standard base64 with padding) |
| `due_deck` | `{review_limit}`: every card is a review card due on or before today (F5.5 due targets), in one top-level deck `Due` besides the default deck, whose preset (`Preset 2`) allows `review_limit` reviews a day |

Committed (R1805): `decks-2000` (20,000 cards, 2,000 decks), `due-500k` (500,000 cards, `review_limit` 9,999), `field-1mb` (10,000 cards, 20 notes of 1,048,576 bytes), `inline-images` (10,000 cards, 200 notes, 2 images of 20,000 bytes), `media-30k` (10,000 cards, 30,000 files of 512 to 8,191 bytes), `tags-10k` (20,000 cards, 10,000 nodes), `template-5mb` (10,000 cards, 5,242,880 bytes).

## F9. Recipes

### F9.1 Format

`core/crates/meguri-testkit/fixtures/recipes/<name>.yaml`, YAML in the scenarios' subset (`specs/conformance/dsl.md` D2); unknown members refused:

| Member | Meaning |
|---|---|
| `recipe`, `description` | the name; one line |
| `now` | the pinned instant, RFC 3339 with `Z` or `±HH:MM`; the offset is the collection's |
| `rollover_hour` | optional, default 4 |
| `presets` | optional list of `{name, new_per_day?, reviews_per_day?}` added after `Default` |
| `decks` | list of decks; each `name` (`::` nests; missing parents are created with `Default`; a deck listed twice or after its child is an error), `preset` (default `Default`), `notetype` (`basic`, `reversed`, `cloze`, `broken`; default `basic`), card counts by state `new`, `learning`, `due` (review due on or before today), `later` (review due after today), `suspended`, `buried` (each default 0), `tags`, `fields` (texts with `{n}`, the note's number in its deck from 1; drawn words when missing), `media` `{images, sounds, missing}` references per note (`missing` names files that are not written) |

### F9.2 Rules

- History: 120 days before the pinned day. Note types are created in the order basic, reversed, cloze, broken, only those used (`Meguri Basic` when none is): `Meguri Basic`, `Meguri Basic and Reverse` (two templates; both cards of a note share its state, so counts are even), `Meguri Cloze`, `Meguri Broken Template` (its back names a field the note type lacks: Anki refuses to save such a note type but reads and renders it, reporting the unknown field; it stands for note types from older releases or imports).
- Buttons are drawn from fixed mixes (Again, Hard, Good, Easy): learning 10, 5, 80, 5; review 8, 4, 83, 5; relearning 10, 5, 83, 2. Answer times from fixed buckets: 1–3 s 20, 3–6 s 40, 6–12 s 30, 12–30 s 10.
- Answers follow F4.4 and F5: a `learning` card has one answer in the 24 hours before the pinned day (so it is due now); a `due` card's last answer is 1 to 20 days before today; a `later` card's last answer is 1 to 3 days before today with its interval lengthened past today. A note is created one to two days before its first answer, or 1 to 30 days before `now` when it has none.
- Media files are always written (F4.9).

## F10. Output

### F10.1 Directory

`collection.anki2` (with `collection.anki2-wal` if the file was not checkpointed), `collection.media/` and `manifest.json`. `tools/mg fixtures <size|worst case|recipe|worst-cases|recipes>... [--seed N] [--media files] [--cards N]` writes `build/fixtures/<first 16 hex digits of the profile's sha256>-<seed>-<size or worst case>/` and `build/fixtures/recipes/<name>-<seed>/`. Sizes: `10k`, `100k`, `150k`, `500k`, `1m` or a card count.

- Reuse: a profile or worst-case directory is reused when its manifest (F10.2) has the request's `generator_version`, `kind`, `name`, `seed`, `cards_requested`, `now_ms`, `utc_offset_secs`, `profile_sha256`, `worst_case_sha256` and `media`, and its `collection.anki2` exists; otherwise it is generated again. Comparing `worst_case_sha256` makes an edited worst case regenerate under its old name. A recipe is generated again on every run: its manifest does not identify the recipe's text.
- Concurrent runs: a run holds an exclusive lock on the file `.<name>.lock` beside the output directory `<name>` from its reuse check to its end, so runs into one directory take turns; the lock file stays. A run builds its output in a directory `.<name>.staging-*` beside it and publishes it by one rename; a previous output first moves to `.<name>.old-*` beside it, returns if the rename fails, and is deleted after it succeeds. A reader finds one run's whole output, or briefly none, never a mix; a failed run leaves the previous output.

### F10.2 Manifest

`manifest.json`, in the goldens' text layout:

| Member | Meaning |
|---|---|
| `generator_version` | F10.4 |
| `kind`, `name` | `profile` (name: the card count), `worst-case` or `recipe` (name: its name) |
| `seed`, `cards_requested` | the request |
| `now_ms` | the pinned instant |
| `utc_offset_secs` | the offset the collection was written in (F4.3; recipes: `now`'s offset) |
| `profile_sha256` | F2.1; `null` for recipes |
| `worst_case_sha256` | lower-case hex SHA-256 of the worst case's definition as read (F8), written again in the goldens' text layout, so a change of layout alone keeps it and a changed value changes it; `null` for profiles and recipes |
| `media` | `references` or `files` |
| `counts` | rows written: `notes`, `cards`, `revlog`, `notetypes`, `decks`, `presets`, `tags`; `media_refs`, `media_files`, `media_bytes` |
| `dump_sha256` | F10.3 |

### F10.3 Logical dump hash

SHA-256 over the twelve tables in the order `col`, `config`, `deck_config`, `decks`, `notetypes`, `fields`, `templates`, `tags`, `graves`, `notes`, `cards`, `revlog`. For each: the table name's bytes and a 0 byte, then each row in key order (`config` by `KEY`, `fields` and `templates` by `(ntid, ord)`, `tags` by `tag` in its `unicase` order, `graves` by `(oid, type)`, the others by `id`), each row's columns in declaration order, each value as a tag byte and its bytes: 0 NULL; 1 INTEGER and its 8 little-endian bytes; 2 REAL and the 8 little-endian bytes of its bits; 3 TEXT, its length as 8 little-endian bytes, its UTF-8 bytes; 4 BLOB, length, bytes; then a 0xFF byte after each row. It does not depend on page layout.

### F10.4 Generator version

`generator_version` changes whenever the same request would write other rows. The rules of this file are version 3. Version 3 made the drawn field length the length of the whole stored field and placed media references by field (F4.8, F3.1).

## F11. Checks

- T0 (`core/crates/meguri-testkit/tests/generate_validity.rs`, in the dev container): same seed twice gives the same `dump_sha256`, another seed another one with the same aggregates (F3); F3 at 10k and 100k cards, field lengths (F3.1) included; the F4.8 examples; ids per F4.7; every worst case's stress present (counts, sizes, valid PNGs); recipes give their decks and counts; F2.6 privacy on an extraction of a hand-built collection with known private strings; 1M cards within 10 minutes and 2 GB peak memory in Docker on the M3 Max (run on demand).
- T1 (oracle, `tools/mg oracle check`): the 10k collection, `tools/mg fixtures 100k`, every worst case and every recipe: Check Database reports 0 for every kind, `changed_tables` lists nothing but `col` and `config`, and `counts` equal the manifest's; `timing.days_elapsed` is `today`. `due-500k` with `--deck Due`: `review` = min(review cards in `Due`, 9,999). `field-1mb` with `--render-note` on a large-field note: every card `ok`, its answer longer than 1,048,576 bytes.
- No scenario files: the generated collections are too large and too numerous to commit as fixtures, and their checks need no golden.
