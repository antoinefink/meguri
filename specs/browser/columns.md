# Browser columns and rows

Spec: browser/columns
Requirements: R1118, R1119, R1120, R1121, R1122, R1123, R1124, R1125, R1126, R1127, R1128, R1129, R1130, R1131, R1268; supports R96, R935, R976, R1132, R1146, R1147, R1148, R1149, R1150, R1242, R1859
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/columns/
Status: Under review (R1232; round 8, decided 2026-11-20) for the lines of C5, C7.4 that begin "Changed 2026-11-20 (round 8)": do not implement them until approved. Every other section is in force and unchanged.

What Browse shows for one card or one note: the catalog of the 19 columns, the row record the core gives the UI (row state, tint, font, cells), the value of every column in cards mode and in notes mode, and the card excerpts call (C16) that gives the review summary a card's Question and Deck cells before Browse exists. Values are typed; the UI formats them with the app's formatters (U210, DesignSystem). C12 gives Anki's English display of each value, which the conformance observation compares and which the UI's formatters follow where a requirement quotes it. How rows sort is `browser/sorting`. Column keys, the active-column lists and the stored sort are `browser/config-keys` (K3 to K5). The observations are `specs/conformance/browser-ops.md` (Browse rows) and `specs/conformance/card-excerpt-ops.md` (card excerpts).

U85a specified C3.1, C7.2, C7.4, C10 and C16 and part of C1; U84 specified the rest under the ids U85a left free.

## C1. Terms

- **Mode.** Cards mode has one row per card, notes mode one row per note (R1115).
- **Cards of a note.** Every card row of the collection whose `nid` is the note's id, whatever the search matched. Notes-mode values are computed over all of them.
- **First card.** The note's card with the lowest `id`. Notes-mode values that come from a single card come from the first card. (A note whose cards were created together has its lowest id on ordinal 0 for a standard note type; Anki creates the cards of a cloze note in no set order.)
- **Card columns.** As stored and read by `model/row-records`: `nid`, `did` (the deck the card is in), `ord` (the template ordinal; for a cloze card, the cloze number minus one), `type` (0 new, 1 learning, 2 review, 3 relearning), `queue` (0 new, 1 learning with `due` a timestamp, 2 review, 3 learning with `due` a day number, 4 preview, −1 suspended, −2 buried with a sibling, −3 buried by the user), `due`, `ivl` (days), `factor` (permille), `reps`, `lapses`, `odid` (the home deck of a card in a filtered deck, 0 otherwise), `odue`, `flags`, `mod`, `data`. A card is **filtered** when `odid` is not 0. The cells read these values with the read tolerances of `model/row-records` R4: `due` and `odue` read as 0 when the stored value is not an INTEGER in the `i32` range, `mod` and `ivl` are read as integers, and a card whose columns do not decode makes the row `unreadable` (C3.1). The sort keys read the stored values instead (`browser/sorting` S2), so a cell and its sort key can disagree on such a card.
- **Home due.** `odue` for a filtered card, `due` otherwise.
- **Full deck name.** A deck's stored name in its human form, the components joined by `::` (`model/schema11-json` S11), as Browse and the deck list show it: `Japanese::Mining`.
- **The template** of a card: for a standard note type, the template whose ordinal is the card's `ord`; for a cloze note type, its first template, whatever the card's ordinal (`render/card-render-pipeline` K2).
- **Card data.** The keys `pos`, `s`, `d`, `decay` and `lrt` of `data`, read as `search/card-extractors` X3 says, its added rules included (absent when missing or unreadable), the reading the sort keys also use. So a `cd` member that is not a JSON string makes the whole text read as no keys: on `{"pos":5,"s":10,"d":5,"cd":5}` a new card's Position is its `due` and its Stability, Difficulty and Retrievability are empty, and a review card shows all four empty. Decided 2026-11-10: this is how Anki reads a card row's `data` for its cells, so the cells follow X3 and not `model/card-data-json` C3, which reads only `cd` as absent there (the open point X3 names).
- **Memory state.** Present when the card data holds both `s` (stability, days) and `d` (difficulty), as `model/card-data-json` C3 rule 4 says; both are `f32`, possibly infinite (`1e40` reads as +∞) or −0.0.
- **Counts in days.** A card counts in days when any of these holds: its `type` is not 0 and its home due is at most 365,000; its `queue` is 2 or 3; its `type` is 2 and its `queue` is negative.
- **Now, today.** `now` is the current instant in whole epoch seconds. `today` is the scheduler's day number (`sched/day-clock`: days elapsed since the collection's creation day). One row request reads each once and uses it for every row.
- **Local time.** The time zone that also gives the scheduler's day boundaries: the device's zone in the app, the scenario's zone in conformance runs.

## C2. Catalog

| Key | Cards-mode label | Notes-mode label | Alignment | Elision | Cell font |
|---|---|---|---|---|---|
| `noteFld` | Sort Field | Sort Field | start | end | yes |
| `question` | Question | Question | start | end | yes |
| `answer` | Answer | Answer | start | end | yes |
| `template` | Card Type | Cards | start | end | no |
| `deck` | Deck | Deck | start | middle | no |
| `cardDue` | Due | Due | center | end | no |
| `cardEase` | Ease | Avg. Ease | center | end | no |
| `cardIvl` | Interval | Avg. Interval | center | end | no |
| `cardReps` | Reviews | Reviews | center | end | no |
| `cardLapses` | Lapses | Lapses | center | end | no |
| `noteCrt` | Created | Created | center | end | no |
| `cardMod` | Card Modified | Card Modified | center | end | no |
| `noteMod` | Note Modified | Note Modified | center | end | no |
| `note` | Note Type | Note Type | start | end | no |
| `noteTags` | Tags | Tags | start | end | no |
| `originalPosition` | Position | Position | center | end | no |
| `stability` | Stability | Stability | center | end | no |
| `difficulty` | Difficulty | Difficulty | center | end | no |
| `retrievability` | Retrievability | Retrievability | center | end | no |

- Sortability per mode and the first sort direction of each key are `browser/config-keys` K3; `browser/sorting` S1 repeats them. The catalog holds them so that the UI needs one source (R1139, R1140, R1146).
- **Alignment** (R1131): start-aligned text columns, centred values. **Elision**: where a value too long for its cell is shortened: deck names in the middle (`Parent::…::Child`), everything else at the end. **Cell font**: the column uses the row's font (C6, R1150).
- **Labels** are the exact English strings above (R1118–R1130). Header menus list columns sorted by label (R1133); that order is the UI's.
- **Tooltips** (R1132) are Meguri's own wording, keyed `browse.column.<key>.help.cards` and `browse.column.<key>.help.notes` in the String Catalog; each says what the cell shows in that mode (C7, C8). Anki has tooltips for a few columns only; none of its text is used.
- Keys other than these 19 are never shown (R1137).

## C3. Row records

A row request names a mode, a list of ids (card ids in cards mode, note ids in notes mode) and a list of columns. For each id the core returns a row record or a row error (C3.1). The card excerpts of C16 report the same errors.

| Member | Cards mode | Notes mode |
|---|---|---|
| id | the card id | the note id |
| note id | the card's `nid` | the note id |
| state | C4 | C4 |
| tint | C5 | C5 |
| font | C6, of the card's template | C6, of the first card's template |
| sort field direction | C6 | C6 |
| cells | one per requested column, in request order: C7 | one per requested column, in request order: C8 |

### C3.1 Row errors

| Error | When |
|---|---|
| `deleted` | the row's own record no longer exists: in cards mode the card, in notes mode the note |
| `damaged` | in cards mode the card exists but its note, its note type, its deck (`did`), its home deck (`odid`, when not 0) or its template (C1) does not (whatever columns were requested); in notes mode the note exists but has no card, or one of those records is missing for its first card |
| `unreadable` | a record the row reads exists but does not decode (`model/row-records` R1 and the tolerances of its tables): in cards mode the card, its note, note type, deck or home deck; in notes mode the note, any of its cards, or the note type, deck or home deck of its first card. Examples: a card whose `ivl` is −1, whose `flags` is 300, whose `factor` is outside 0 to 65,535 or not an INTEGER, whose `reps` or `lapses` is negative, or whose `type` or `queue` is not a known value after the low-8-bit reduction; a note whose `tags` or `flds` is not TEXT. It carries the table, the column and the row id of the value that failed |

- The records are read in this order, and the first that fails gives the error: the row's own record (missing: `deleted`), then in cards mode the card's note, in notes mode all of the note's cards (none: `damaged`), then the note type, the deck, the home deck and the template. So a card whose note does not decode and whose deck is missing is `unreadable`.
- A row error never stops the other rows of the request. The UI shows a damaged or unreadable row disabled (U210). Check Database repairs damaged rows and the values that make a row unreadable (U128).
- Anki's table shows the same rows: a record that does not decode fails that row only, shown disabled with the backend's message. The card excerpts call (C16) has no `unreadable` record: such a card fails the whole call (C16.3).

## C4. Row state (R1149)

| Flag | Cards mode | Notes mode |
|---|---|---|
| flag | `flags` & 7, a number 0–7 (0: none) | 0 |
| marked | the note has a tag equal to `marked` ignoring ASCII letter case | same |
| leech | the note has a tag equal to `leech` ignoring ASCII letter case | same |
| suspended | `queue` is −1 | false |
| buried | `queue` is −2 or −3 | false |
| filtered | `odid` is not 0 | false |

- `Marked`, `MARKED` and `marked` count; `marked::x`, `unmarked` and `märked` do not. Tags are compared one by one, as the note stores them (`tags/tags`).
- The leech flag is Meguri's (the leech glyph of R1149); Anki's Browse has no leech state. It follows the same rule as marked: `Leech` counts, `leech::old` does not, although `tag:leech` would find both.

## C5. Tint (R1147)

Changed 2026-11-20 (round 8): Meguri tints no row; a state column shows a symbol (`browser/workspace` V2.5).

The row background the UI derives from DesignSystem tokens (R1148).

- Cards mode: the first that applies:
  1. the stored `flags` value is exactly 1, 2, 3, 4, 5, 6 or 7: flag tint 1 to 7. The whole value counts, so a card with `flags` 9 (user flag 1 plus a higher bit another client set) gets no flag tint, although its C4 flag is 1;
  2. the note is marked (C4);
  3. `queue` is −1: suspended;
  4. `queue` is −2 or −3: buried;
  5. none.
- Notes mode: marked when the note is marked, otherwise none.

So a card flagged red (1) whose note is marked and which is suspended has the red tint, with state {flag 1, marked, suspended}; in notes mode the same note has the marked tint and state {marked} (AE44).

## C6. Font and direction

- **Font**: the browser font name and size of the card's template (C1): note type template config `browser_font_name` and `browser_font_size`, schema-11 `bfont` and `bsize`. The two members are independent: the name is absent when empty, the size is absent when 0. A name with size 0 sets the family at the default size; a size with an empty name sets the size in the default family (Anki's table also applies each on its own). The UI applies them to the cell-font columns (R1150).
- **Sort field direction**: right to left when the note type's sort field (index `sortf`) has the field option `rtl` set; the Sort Field cell is then laid out right to left (R1118). Left to right when the note type has no field at index `sortf` (Decided 2026-10-27, as C7.1). No other cell carries a direction.

## C7. Cells in cards mode

The row's card is "the card". Each cell has one of the typed values of C9; "empty" is the value with nothing to show.

### C7.1 Sort Field (`noteFld`, R1118)

Text: `text/strip-html` H6 with media names kept, applied to the note's field at the note type's sort field index, as the note stores it (not the `sfld` column). Decided 2026-10-27: when the note has no field at that index (a `sortf` past the note's fields, which only a damaged collection holds), the cell is the empty text and the row stays a row record, not a row error. Anki's lookup fails there and its table shows an error row and stops updating, which no golden records; Meguri keeps the row so that the user can open and repair the note. Examples: `<b>Dog</b>` → `Dog`; `front [sound:a.mp3] <b>bold</b>` → `front a.mp3 bold`; `one<br>two` → `one two`; `<img src="cat.png">` → `cat.png`.

### C7.2 Question and Answer (`question`, `answer`, R1119, R1120)

C10.

### C7.3 Card Type (`template`, R1121)

Text: the name of the card's template (C1). For a cloze note type: that name, one U+0020, and the card's ordinal + 1 in decimal: `Cloze 1`, `Cloze 2`, `Cloze 10`. The core composes this text; the UI does not (C14).

### C7.4 Deck (`deck`, R1122, R1268)

Changed 2026-11-20 (round 8): Meguri shows the deck as its name with the parent path in secondary text, cut from the path's end (`browser/workspace` V5.1); dates in every cell read as `browser/workspace` V5.2 says. The cell values here stay the core's.

Text: the full name of the deck `did` names. When the card is filtered: that name, ` (` (U+0020, U+0028), the full name of the deck `odid` names, `)`: `Filtered 1 (Japanese::Mining)`, `Cram (Spanish::Verbs)`. The deck shown first is the filtered deck the card is in; the one in parentheses is its home deck.

- The core composes this text; the UI does not (C14).
- Only the two names count. Whether the deck `did` names is itself a filtered deck plays no part, nor do the card's other columns (`odue`, `queue`, `due`).
- Nothing is shortened: elision is the UI's (C2 shortens deck names in the middle).
- Examples: a card in `Japanese::Mining` → `Japanese::Mining`; a card in `Default` → `Default`; a card with `did` naming `Filtered 1` and `odid` naming `Japanese::Mining` → `Filtered 1 (Japanese::Mining)`.

### C7.5 Due (`cardDue`, R1123, R1268)

The first that applies:

1. the card is filtered: **filtered**;
2. `type` is 0 or `queue` is 0: **new position** `due` (the stored value, sign included);
3. `queue` is 1: the **instant** given by the home due, read as epoch seconds;
4. the card counts in days (C1): the **instant** now + (home due − today) × 86,400 seconds;
5. otherwise: empty.

Then, when `queue` is negative and the value is not empty, the value is **wrapped**. An empty value is never wrapped.

The UI shows an instant as its local date (C12). This is the cell's own rule: the Due sort key (`browser/sorting` S6) and the `prop:due` search (`search/properties` P4) compute their own due values, and the three differ by design (a suspended learning card has an empty cell, an S6 key and never matches `prop:due`). Rule 4 adds whole multiples of 86,400 seconds to now: it does not step calendar days, so when a daylight-saving change lies between now and the due day, the date shown can differ by one from "today's date plus the days" (scenario `browser/columns/due-dst`).

| Card (today 0, now 2030-01-15 10:00 UTC) | Value |
|---|---|
| new, `due` 5 | new position 5 |
| new, suspended, `due` 5 | new position 5, wrapped |
| learning, `queue` 1, `due` 2030-01-15 10:20 UTC | instant 2030-01-15 10:20 |
| relearning, `queue` 3, `due` 1 | instant 2030-01-16 10:00 |
| review, `due` −3 | instant 2030-01-12 10:00 |
| review, suspended, `due` 2 | instant 2030-01-17 10:00, wrapped |
| learning (`type` 1), suspended (`queue` −1), `due` a timestamp | empty |
| review in a filtered deck | filtered |
| new in a filtered deck, buried | filtered, wrapped |
| review, `due` 1,894,701,600 (epoch seconds stored as a day number, which Check Database repairs) | an instant about 1.6 × 10^14 seconds after the epoch, shown `invalid date` (C12) |
| review, `due` 3,000,000 | an instant in the year 10243, shown `+10243-…` (C12) |

### C7.6 Ease (`cardEase`, R1124)

`type` 0: the **new** marker. Otherwise the **percent** `factor` ÷ 10, rounded down: 2,500 → 250, 2,505 → 250, 1,300 → 130. A learning card whose `factor` is still 0 shows 0, not the new marker.

### C7.7 Interval (`cardIvl`, R1125)

`type` 0: the **new** marker. `type` 1: the **learning** marker. `type` 2 or 3: the **time span** of ((`ivl` × 86,400) reduced modulo 2^32) seconds, as an `f32`. A relearning card with `ivl` 0 shows a span of 0 seconds.

### C7.8 Reviews, Lapses (`cardReps`, `cardLapses`, R1126)

The **count** `reps`, the **count** `lapses`.

### C7.9 Created, Card Modified, Note Modified (`noteCrt`, `cardMod`, `noteMod`, R1127)

**Date-times**: the note id divided by 1,000 and truncated toward zero (epoch seconds; a note id of −60,500 gives −60, shown `1969-12-31 @ 23:59` in UTC); the card's `mod`; the note's `mod`. The UI shows date and time of day to the minute in local time. A `mod` written in milliseconds gives a date-time near the year 62000 (C12).

### C7.10 Note Type, Tags (`note`, `noteTags`, R1128)

Text: the note type's name. Text: the note's tags in stored order (the stored text split at U+0020 and U+3000, empty parts dropped), joined by one U+0020; empty text when the note has none.

### C7.11 Position (`originalPosition`, R1129)

The card data `pos` when present (C1): the **position** `pos`. Otherwise, when `type` is 0: the **position** `due` (the stored value: a new card in a filtered deck shows its filtered-deck position, such as −100000). Otherwise empty.

### C7.12 Stability, Difficulty, Retrievability (`stability`, `difficulty`, `retrievability`, R1130)

Without a memory state all three are empty (a card whose data holds `s` but no `d` shows nothing here, although `browser/sorting` S4 sorts it by `s`).

- Stability: the **time span** of `s` × 86,400 seconds, the product in `f32`. A negative `s` keeps its sign (`s` −1 → −1 day); `s` +∞ gives a span of +∞.
- Difficulty: the **percent** ((`d` − 1) ÷ 9) × 100, each step in `f32`, an `f32` shown rounded (C12): `d` 5.0 → 44, `d` 3.0 → 22, `d` 9.0 → 89, `d` 0.99 → `-0`, `d` +∞ → `inf`, `d` −∞ → `-inf`, `d` 1e30 → a 32-digit number.
- Retrievability: C11.

## C8. Cells in notes mode

| Column | Value |
|---|---|
| Sort Field, Created, Note Modified, Note Type, Tags | as C7 for the note |
| Question, Answer | C10 for the first card |
| Cards (`template`) | the **count** of the note's cards |
| Deck | when the note's cards have more than one distinct `did`: the **deck count**, that number; otherwise C7.4 for the first card. Every distinct `did` counts, also one that names no deck: a note whose first card is in `Lang` and whose second card's deck is missing shows the deck count 2, not `Lang` and not an error (only the first card's records make a row `damaged`, C3.1). The sort key counts only existing decks (`browser/sorting` S5) |
| Due | among the note's cards that are not filtered, have `type` and `queue` both not 0, and have a non-negative `queue`: the earliest of their C7.5 instants (rules 3 and 4; a card for which neither applies is skipped); empty when there is none. Never wrapped (R1144, AE50) |
| Avg. Ease | the cards with `type` not 0: none → the **new** marker; otherwise the **percent** ⌊⌊S ÷ n⌋ ÷ 10⌋, where n is their number and S the sum of their `factor` values reduced modulo 65,536 (C13) |
| Avg. Interval | the cards with `type` 2 or 3: none → empty; otherwise the **time span** of ⌊((sum of their `ivl`) × 86,400 reduced modulo 2^32) ÷ n⌋ seconds as an `f32`, n their number (C13). Learning cards are not counted |
| Reviews, Lapses | the **count** of the sum over the note's cards, reduced modulo 2^32 (C13) |
| Card Modified | the **date-time** of the largest `mod` of the note's cards |
| Position, Stability, Difficulty, Retrievability | C7.11 and C7.12 for the first card |

Example: a note with a review card (`factor` 2,500, `ivl` 10) and a learning card (`factor` 0) shows Avg. Ease 125 and Avg. Interval 10 days; a note with only new cards shows the new marker, an empty Avg. Interval and an empty Due.

## C9. Value types

| Type | Carries | Used by |
|---|---|---|
| text | a string (C6 direction for Sort Field) | Sort Field, Question, Answer, Card Type, Deck, Note Type, Tags |
| count | a non-negative integer | Cards, Reviews, Lapses |
| deck count | an integer ≥ 2 | notes-mode Deck |
| percent | an integer (Ease); an `f32`, finite or not, shown rounded (Difficulty, Retrievability) | Ease, Difficulty, Retrievability |
| position | an integer | Position |
| instant | epoch seconds (a 64-bit integer, possibly outside the range a date can show), shown as a local date | Due |
| date-time | epoch seconds (as for instant), shown as local date and time to the minute | Created, Card Modified, Note Modified |
| new position | an integer, shown as `New #` and the number | Due |
| filtered | nothing | Due |
| new marker, learning marker | nothing | Ease, Interval |
| time span | seconds as an `f32`, negative or infinite for Stability | Interval, Stability |
| empty | nothing | any column |
| render outcome | C10 | Question, Answer |
| deferred | nothing: the cell is not computed yet; `qa_cells` gives it (`browser/result-sets` H7). Distinct from empty and from a row error | Question, Answer, only in a row page |

The Due values instant, new position and filtered carry the wrapped flag of C7.5.

## C10. Question and Answer

Steps 1 to 3, 5, 6 and 7 make the Question cell, and step 4 the Answer cell; step 8 is the row cache. The card excerpts of C16 use the Question cell only.

1. **Render** the card (notes mode: the first card) in browser mode, `render/card-render-pipeline` K5: the template's browser question format when it is not the empty string, otherwise its question format (R935); FrontSide empty; no audio extraction. The outcome is rendered (question text Q, answer text A), blank (`missing_cloze`, question text Q), or a template error (kind and side). A card of a standard note type is never blank in browser mode, even when its question renders nothing (R976): its cell is then the empty text. A card that cannot be rendered at all makes the row `damaged` (C3.1).
2. **Audio display form.** For a text x, P(x) is x read as items (`render/av-tags` A2 to A4) and written back with: each sound tag as U+1F509, the tag's name exactly as written (no entity decoding), U+1F509; each TTS directive (named exactly `tts`, whatever its language, an empty one included) as U+1F4AC, its content, U+1F4AC; every other directive in its normal form (`render/av-tags` A7); plain text unchanged. So `B:front [sound:a.mp3] <b>bold</b>` becomes `B:front 🔉a.mp3🔉 <b>bold</b>`, and `[anki:tts lang=en_US]hi[/anki:tts]` becomes `💬hi💬`.
3. **Question cell**: the text line (`text/strip-html` H6, media names kept) of P(Q).
4. **Answer cell**: let q = P(Q) and a = P(A). When a begins with q (code point by code point), remove that beginning. The cell is the H6 line (media names kept) of what remains. Since the browser answer's FrontSide is empty (K5), the beginning only matches when the answer template repeats the question by other means, such as `{{Front}}<br>{{Back}}` for a question `{{Front}}`, which shows `b1` for the fields `10`, `b1`.
5. **Blank**: the Question cell is the blank outcome with the H6 line of P(Q); the Answer cell is the blank outcome with no text. Keep the missing-cloze metadata. Browse adds no separate blank-card notice (`render/card-render-pipeline` K12, R976).
6. **Template error**: both cells are the error (kind and side); the UI shows Meguri's wording (`render/template-syntax` T9). A browser format with an error the review templates do not have shows the error here only.
7. Rendering runs only for rows whose request includes Question or Answer (R1119, R1242), never on the main thread, and never writes anything (`render/card-render-pipeline` K7).
8. **Cache** (Meguri, R1242, U85): rendered cells may be kept in an in-memory LRU keyed by card id, each entry with its card's note id. Decided 2026-10-27: entries are dropped by change sets, not keyed on mtimes, since mtimes are whole seconds and two edits in one second would keep an equal key. An in-core observer (`engine/change-stream` S2), which runs before the change set is published, drops the entries of every card the change set lists and of every card whose note it lists, and every entry when it lists any note type or deck (as `browser/result-sets` H10 refetches); a generation change drops them all. A cell rendered in a read transaction is stored only when the cache has applied no change set newer than that transaction's change seq (`browser/result-sets` H1). So Change Deck, a flag change, a deck rename, a note edit and a note type edit are never served from the cache (R960), however close together.

Examples of Question cells (each from a stored card, browser question format empty unless said):

| Question template and fields | Question cell |
|---|---|
| `{{Front}}`, Front `<b>猫</b><br>[sound:neko.mp3]` | `猫 🔉neko.mp3🔉` |
| `{{Front}}`, Front `<div>犬</div><img src="inu.jpg">` | `犬 inu.jpg` |
| `{{Front}}`, Front `ねこ [anki:tts lang=ja_JP]猫[/anki:tts] &amp; 犬` | `ねこ 💬猫💬 & 犬` |
| `<div>{{Front}}</div>{{Extra}}`, Front `Baum`, Extra empty | `Baum` |
| question format `{{Front}}`, browser question format `{{Back}} ({{Front}})`, Front `Baum`, Back `tree` | `tree (Baum)` |
| question format `{{Front}}?`, browser question format `{{Extra}}`, Extra empty | the empty text, not blank |
| question format `{{Front}}!`, browser question format of two spaces | the empty text (the format is used, its line is trimmed) |
| cloze `{{cloze:Text}}`, Text `{{c1::犬}} と {{c2::猫::ねこ}}<br>[sound:neko.mp3]`, card of cloze 2 | `犬 と [ねこ] 🔉neko.mp3🔉` |
| the same note, card of cloze 1 | `[...] と 猫 🔉neko.mp3🔉` |

The goldens of C16.6 are the reference for these values.

## C11. Retrievability

The cell's elapsed time is its own; the retrievability formula is `search/card-extractors` X5 (notation w as there). E, an unsigned 32-bit number of seconds since the last review:

1. card data `lrt` present: w(now − `lrt`) (an `lrt` after now gives a very large E);
2. otherwise, when the card counts in days (C1): with T the home due read as epoch seconds when `queue` is 1, otherwise now + (home due − today) × 86,400, and L = T − `ivl` × 86,400, E = w(now − L), or 0 when now − L is negative. A review card with `ivl` 60,000 that is due today has now − L = 5,184,000,000 s, so E = 5,184,000,000 − 2^32 = 889,032,704 s (about 10,290 days);
3. otherwise: w(now − home due).

The value is the **percent** R × 100, an `f32` shown rounded (C12), where R is `search/card-extractors` X5 steps 3 to 5 with E in place of e: the card's memory state (C1) and its data's `decay`, or 0.5 when the data has none. Examples (scenarios `browser/columns/cards-mode` and `notes-mode`): `s` 10, `d` 3, no `lrt`, a review due in 3 days with `ivl` 10 → E = 7 days → 93; due in 4 days with `ivl` 4 → E = 0 → 100. R is NaN, shown `NaN`, when `s` is 0 and E is 0, and when a negative `s` makes the base negative (X5); `browser/sorting` S7 turns the same NaN into a NULL key.

- When `lrt` is present and lies in 0..now, E is X5's e (an `lrt` that is negative or at least 2^32 can give a different E: X5 wraps each operand before a subtraction floored at 0, rule 1 wraps the difference), so the cell shows the value the `prop:r` search compares and `browser/sorting` S7 sorts by. Without `lrt`, rules 2 and 3 differ from X5 step 2: a relearning card with no `lrt` whose timestamp due is later today shows a very low retrievability here (rule 3 wraps around) while the sort counts 0 elapsed seconds. Both reproduce Anki.

## C12. Anki's English display

What Anki 26.09.3 shows in English for each value. The conformance observation (`specs/conformance/browser-ops.md` B4) compares these forms in structured shape. Where a requirement quotes them (R1123–R1125), the UI uses the strings of C14.

| Value | Display |
|---|---|
| text | the text |
| count, position, deck count | the decimal number; a deck count in parentheses: `(2)` |
| percent | the number and `%`: `250%`, `7%`, `-0%`, `inf%`, `NaN%` (below) |
| instant | the local date `YYYY-MM-DD`; years and invalid instants below |
| date-time | the local date and time `YYYY-MM-DD @ HH:MM`; `invalid date @ invalid date` for an invalid instant |
| new position | `New #5`, `New #-100000` |
| filtered | `(filtered)` |
| new marker, learning marker | `(new)`, `(learning)` |
| wrapped | the display in parentheses: `(2030-01-17)`, `(New #5)`, `((filtered))` |
| empty | nothing |
| time span | amount and unit, as below |

**Percent** of an `f32` p (Difficulty, Retrievability): the exact decimal value of p rounded to an integer, an exact half to the even neighbour, written in full with no exponent and a `-` when p is negative, then `%`. A p from −0.5 to −0.0 is written `-0`. +∞ is `inf`, −∞ `-inf`, NaN `NaN`.

**Years.** The year of a local date is written with four digits, zero-padded, from 0000 to 9999; outside that range with a sign and at least four digits: `+10243-06-02`, `-0001-12-31`. An **invalid instant** is one whose local date-time lies outside −262143-01-01 to +262142-12-31; its date and its time of day each show `invalid date`. Rule 4 of C7.5 reaches it for a review `due` more than about 95,000,000 days from today.

**Time span** of t seconds (an `f32`):

1. The unit: the first row whose condition holds, where a is the absolute value of t, compared in `f32`.

   | Condition | Unit | Unit length (s) |
   |---|---|---|
   | a < 60 | seconds | 1 |
   | a < 3,600 | minutes | 60 |
   | a < 86,400 | hours | 3,600 |
   | a < 2,628,000 | days | 86,400 |
   | a < 31,536,000 | months | 2,628,000 |
   | otherwise | years | 31,536,000 |

   The same thresholds as the answer buttons (`sched/answer-labels` A2): a year is 365 days, a month a twelfth of it.
2. The amount: q = t ÷ unit length in `f32`. Seconds and days: q rounded to an integer, halves away from zero. Minutes, hours, months and years: round(q × 10) ÷ 10, all in `f32`, halves away from zero. (Minutes keep a decimal here, unlike the answer buttons: 90 s is 1.5 minutes.)
3. The amount's text: the amount written with two decimals (the `f32` value widened exactly to 64 bits, rounded to two places, an exact half to the even digit), then trailing zeros removed, then a trailing `.` removed: 1.3 → `1.3`, 13 → `13`, 82.2 → `82.2`. The sign is kept (−0.0 → `-0`); +∞ is `inf`, −∞ `-inf`.
4. Anki writes `<amount> <unit>`, the unit singular when the absolute amount is exactly 1 and plural otherwise, infinite amounts included: `1 day`, `-1 day`, `10 days`, `1.3 months`, `82.2 years`, `0 seconds`, `-0 seconds`, `inf years`.

Examples: `ivl` 10 → 10 days; 40 → 1.3 months; 45 → 1.5 months; 364 → 12 months; 365 → 1 year; 30,000 → 82.2 years; stability 12.5 → 13 days (12.5 days rounds half away from zero).

Anki also wraps each number it inserts into these texts in the isolation marks U+2068 and U+2069; they are not part of any value.

## C13. Arithmetic kept from Anki

Meguri computes these exactly as Anki 26.09.3 does, overflow included, so that every cell matches the oracle (R1886):

- Avg. Ease sums `factor` values modulo 65,536 before dividing: a note with 27 review cards at 2,500 shows 7, not 250 (scenario `browser/columns/aggregates`).
- Avg. Interval multiplies the summed days by 86,400 modulo 2^32 before dividing: two review cards with `ivl` 30,000 show 14.1 years, not 82.2.
- Retrievability takes now − `lrt`, now − L and now − due modulo 2^32 (C11).
- Reviews and Lapses in notes mode sum `reps` and `lapses` modulo 2^32: two cards with `reps` 3,000,000,000 show 1,705,032,704. The notes-mode sort keys use the exact sums (`browser/sorting` S5).
- Due adds days as multiples of 86,400 seconds (C7.5).

Decided 2026-10-27: Meguri keeps these results and shows no "true" average. Changing them would be a deliberate deviation, which needs a requirement amendment and a `conformance/deviations.yaml` entry against the scenario's golden.

## C14. Strings

The UI takes these strings only from this table (role: `docs/process/roles/spec-writer.md`). The two rows marked "core" are composed by the core into the cell's text value (C7.3, C7.4), never by the UI; they are listed so that the wording has one source:

| Use | String |
|---|---|
| Due of a new card | `New #` followed by the position |
| Due of a filtered card | `(filtered)` |
| Ease or Interval of a new card | `(new)` |
| Interval of a learning card | `(learning)` |
| a wrapped Due | `(` value `)` |
| Deck of a filtered card (core) | deck name, ` (`, home deck name, `)` |
| notes-mode Deck spread over decks | `(` count `)` |
| Card Type of a cloze card (core) | template name, U+0020, number |
| column labels | C2 |

## C15. Scenarios

Observation: `browser_rows` (`specs/conformance/browser-ops.md` B3, B4). Every scenario has its golden (browser-ops B6).

| Scenario | Covers |
|---|---|
| `browser/columns/cards-mode` | C4, C5, C7 for every card state: new, suspended new, buried new, learning, relearning, interday learning, review (overdue, today, future), suspended review, suspended learning (empty Due), filtered new and review; flags 1 and 9, marked (AE44), ease truncation, interval spans from 0 s to years, FSRS cells with and without `lrt`, Position from `pos` and from `due` |
| `browser/columns/notes-mode` | C8: first-card values (a note whose first card is ordinal 1), Cards count, deck count, earliest Due over eligible cards only, AE50's note with only new cards, averages with learning cards, sums, latest Card Modified, marked tint |
| `browser/columns/question-answer` | C10: answer prefix removal, FrontSide empty, `<br>` and `<div>` to spaces, image and sound names, TTS and other directives, cloze question and answer, notes mode on the first card |
| `browser/columns/due-dst` | C7.5 rule 4 across a daylight-saving change; local dates and date-times in a zone with an offset |
| `browser/columns/aggregates` | C13: the 16-bit ease sum and the 32-bit interval product |
| `browser/columns/row-errors` | C3.1 in both modes: a deleted card, a deleted note, a note without cards (notes mode `damaged`), a card whose deck is missing. `unreadable` has no case yet: the observation records Anki's undecodable row as a run error (browser-ops B4) |
| `browser/columns/switched-columns` | C3: each row request returns exactly its own columns. Four reads of the same two cards: Sort Field and Deck, then a disjoint set (Note Type, Tags, Reviews), then Deck alone with the rows reversed, then the first set again. No read keeps a cell from an earlier selection |
| `browser/columns/requested-order` | C3: cells in request order. The Note Type, Deck and Sort Field cells (C7.10, C7.4, C7.1) requested in three orders, none of them the catalog's, with every cell text distinct; the last read in notes mode (C8) |
| `browser/columns/time-span-units` | C12's time span through Interval (C7.7) and Stability (C7.12): each unit threshold (59.6 s stays in seconds, 0.9999 days in hours, 364 days in months, 365 days is a year), whole amounts for seconds and days, one decimal for minutes, hours, months and years |

- The oracle driver implements `browser_rows`. The testkit's scenario model reads it, and Meguri's executor reports it as pending until U84's adapter implements it (browser-ops B6). `question-answer` names the cards that `edit_note` adds with `adopt` (browser-ops B5).
- `browser/modes/rows-current` and `rows-excerpts` (`browser/result-sets` H16, goldens recorded, inactive until U85) read `browser_rows` after edits, deletions, a deck rename and a template rename, and compare the cards-mode Question and Deck cells with `card_excerpts` (C16).
- The cards-mode Question and Deck cells (C7.2, C7.4, C10 steps 1 to 3) and the cards-mode row errors also have the `card_excerpts` scenarios of C16.6, which are active now: `question-deck/browser-format`, `media-line`, `cloze` and `deck` for the cells; `question-deck/missing` for `deleted` (the excerpt's `missing`); `question-deck/damaged` and `damaged-notetype` for `damaged` (a missing deck, home deck, note, note type or template). `row-errors` adds what only Browse rows show: the notes-mode errors (a deleted note, a note without cards).

Not covered by scenarios yet: card data with a non-string `cd` in the cells (C1), `unreadable` rows (C3.1), invalid and five-digit-year instants (C12), non-finite and `-0` percents and time spans (C7.12, C11, C12), the notes-mode Reviews and Lapses wrap (C13), a deck count over a missing deck (C8), browser answer formats (`bafmt`) and fonts (`bfont`, `bsize`), which `update_notetype` template edits can set (`notetype-ops.md` T5) but no active observation reads; RTL sort fields (field config); template errors and blank cloze cards in Question and Answer (Anki's cells hold localized messages, R1887). U84's unit tests cover them from this text.

## C16. Card excerpts

The review session summary lists each missed card with a plain-text excerpt of its question and its deck (R96, U157). Those are the card's Question and Deck cells (C10, C7.4), computed by the same functions Browse uses (U84 reuses them unchanged), without any search, sort or Browse state. One call returns them for a list of card ids (R1859).

### C16.1 The call

- **Input**: a list of card ids (64-bit integers) in the order the caller wants the records, made through a collection handle (`engine/lifecycle` L3). The list may be empty, may repeat an id, and may hold ids that name no card.
- **Output**: a list with one record per input id, in input order. A repeated id gets a record at each of its places. An empty input gives an empty output.
- **Records**:

  | Record | When | Members |
  |---|---|---|
  | found | the card exists and none of its records is missing (C3.1) | card id; question: the Question cell (C16.2); deck: the Deck cell (C7.4), a string |
  | missing | no card has the id (C3.1 `deleted`) | card id |
  | damaged | C3.1 `damaged` in cards mode | card id |

- Cards mode always: the config key `browserTableShowNotesMode` plays no part, nor do Browse's active columns, sort or search.

### C16.2 The Question cell value

The outcome of C10 steps 1 to 3, 5 and 6, as one of:

| Value | When | Carries |
|---|---|---|
| text | rendered (C10 step 1) | the step 3 line: a string, possibly empty |
| blank | a cloze card whose cloze number is in no field (`missing_cloze`, C10 step 5) | the H6 line of P(Q), a string; the kind `missing_cloze` |
| template error | C10 step 6 | the error kind (`render/template-syntax` T9) and the side (`question` or `answer`) |

No localized or English message is part of the value (R1887). The UI writes Meguri's own wording for blank and template-error values and labels a missing card as deleted (U157). Lines are whole: the core never shortens them, and the UI truncates them for display.

### C16.3 Reading

- The call reads in one read transaction on the query reader (`engine/lifecycle` L11), so every record sees the same state of the collection, and it holds no statement or transaction once it returns (KTD9).
- It checks the handle's generation while it holds the reader (`engine/lifecycle` L3). A call through a handle of an older generation fails as a whole with `StaleGeneration{planned, current}` and returns no records, whatever the list holds (an empty one included).
- It never writes: no row of any table, no `col.mod`, `usn` or `mtime` change, no config key, no undo step, no change event, no media file (C10 step 7, `render/card-render-pipeline` K7).
- It needs no day or time: nothing in C7.4 or C10 depends on them, so it runs no day query (`sched/day-clock` D8) and reads no clock.
- It derives only the Question cell from the complete browser outcome (`render/card-render-pipeline` K5), including answer-side template errors. It does not compute the Answer cell text (C10 step 4) and keeps no cache (C10 step 8).
- A failure that is not a record outcome (a storage error, a record that exists but cannot be decoded) fails the whole call with that error; no partial list is returned.

### C16.4 Meguri contract

- A short blocking export on the query lane (KTD9). CoreBridge wraps it in one async call per list, never on the main thread; the summary passes all its missed card ids, in first-miss order, in one call (R1859). There is no paging: the lists are one session's missed cards.
- The records cross the boundary with the card id in every record, so the caller can match them to its own list.
- Errors: `StaleGeneration` (C16.3), the engine's other refusals of reader requests (`engine/lifecycle`, such as `CollectionUnavailable` when no collection is open) and storage errors. A missing or damaged card is a record, never an error.

### C16.5 Cross-check

U85 verifies C16 against C3.1, C7.4 and C10.

### C16.6 Scenarios

Observation: `card_excerpts` (`specs/conformance/card-excerpt-ops.md`). Each scenario needs its golden (`card-excerpt-ops` E6).

| Scenario | Covers |
|---|---|
| `browser/columns/question-deck/browser-format` | C10 step 1: a browser question format used when set, the question format when not, a browser format that renders nothing (the empty text, not blank, R976), a format of only spaces used (R935) |
| `browser/columns/question-deck/media-line` | C10 steps 2 and 3: bold text, `<br>` and `<div>` to spaces, a sound tag and an image kept by name, a TTS directive, an entity |
| `browser/columns/question-deck/cloze` | C10 on a cloze note: each card's question with its own cloze hidden, the other shown, stripped to one line |
| `browser/columns/question-deck/deck` | C7.4: a nested deck, the default deck, a filtered card with its home deck in parentheses (R1268) |
| `browser/columns/question-deck/missing` | C16.1: request order kept, a card of a deleted note reported missing in its place |
| `browser/columns/question-deck/damaged` | C3.1 and C16.1: a card whose deck, home deck or note is missing reported damaged, the other records unaffected |
| `browser/columns/question-deck/damaged-notetype` | C3.1 and C16.1: a card whose ordinal names no template of its standard note type, and a card whose note names no note type, reported damaged |
| `browser/columns/question-deck/read-only` | C16.3: no row stamped by the call |

Not covered by scenarios: blank cloze cards and template errors (Anki's cells hold localized messages, R1887), the generation check and the bridge lane. Meguri's engine and CoreBridge tests cover them from this text.
