# Browser columns and rows

Spec: browser/columns
Requirements: R1118, R1119, R1120, R1121, R1122, R1123, R1124, R1125, R1126, R1127, R1128, R1129, R1130, R1131, R1268; supports R96, R935, R976, R1132, R1146, R1147, R1148, R1149, R1150, R1242, R1859
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/columns/

What Browse shows for one card or one note: the catalog of the 19 columns, the row record the core gives the UI (row state, tint, font, cells), the value of every column in cards mode and in notes mode, and the card excerpts call (C16) that gives the review summary a card's Question and Deck cells before Browse exists. Values are typed; the UI formats them with the app's formatters (U210, DesignSystem). C12 gives Anki's English display of each value, which the conformance observation compares and which the UI's formatters follow where a requirement quotes it. How rows sort is `browser/sorting`. Column keys, the active-column lists and the stored sort are `browser/config-keys` (K3 to K5). The observations are `specs/conformance/browser-ops.md` (Browse rows) and `specs/conformance/card-excerpt-ops.md` (card excerpts).

U85a specified C3.1, C7.2, C7.4, C10 and C16 and part of C1; U84 specified the rest under the ids U85a left free.

## C1. Terms

- **Mode.** Cards mode has one row per card, notes mode one row per note (R1115).
- **Cards of a note.** Every card row of the collection whose `nid` is the note's id, whatever the search matched. Notes-mode values are computed over all of them.
- **First card.** The note's card with the lowest `id`. Notes-mode values that come from a single card come from the first card. (A note whose cards were created together has its lowest id on ordinal 0 for a standard note type; Anki creates the cards of a cloze note in no set order.)
- **Card columns.** As stored and read by `model/row-records`: `nid`, `did` (the deck the card is in), `ord` (the template ordinal; for a cloze card, the cloze number minus one), `type` (0 new, 1 learning, 2 review, 3 relearning), `queue` (0 new, 1 learning with `due` a timestamp, 2 review, 3 learning with `due` a day number, 4 preview, −1 suspended, −2 buried with a sibling, −3 buried by the user), `due`, `ivl` (days), `factor` (permille), `reps`, `lapses`, `odid` (the home deck of a card in a filtered deck, 0 otherwise), `odue`, `flags`, `mod`, `data`. A card is **filtered** when `odid` is not 0.
- **Home due.** `odue` for a filtered card, `due` otherwise.
- **Full deck name.** A deck's stored name in its human form, the components joined by `::` (`model/schema11-json` S11), as Browse and the deck list show it: `Japanese::Mining`.
- **The template** of a card: for a standard note type, the template whose ordinal is the card's `ord`; for a cloze note type, its first template, whatever the card's ordinal (`render/card-render-pipeline` K2).
- **Card data.** The keys `pos`, `s`, `d`, `decay` and `lrt` of `data`, read as `model/card-data-json` C3 says (absent when missing or unreadable). The sort keys read the same keys through `search/card-extractors` X3.
- **Memory state.** Present when the card data holds both `s` (stability, days) and `d` (difficulty); both are `f32`.
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

Check Database repairs damaged rows (U128). A row error never stops the other rows of the request. The UI shows a damaged row disabled (U210).

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

- **Font**: the browser font name and size of the card's template (C1): note type template config `browser_font_name` and `browser_font_size`, schema-11 `bfont` and `bsize`. An empty name or a size of 0 means "no font set". The UI applies it to the cell-font columns (R1150).
- **Sort field direction**: right to left when the note type's sort field (index `sortf`) has the field option `rtl` set; the Sort Field cell is then laid out right to left (R1118). No other cell carries a direction.

## C7. Cells in cards mode

The row's card is "the card". Each cell has one of the typed values of C9; "empty" is the value with nothing to show.

### C7.1 Sort Field (`noteFld`, R1118)

Text: `text/strip-html` H6 with media names kept, applied to the note's field at the note type's sort field index, as the note stores it (not the `sfld` column). An index past the last field gives empty text. Examples: `<b>Dog</b>` → `Dog`; `front [sound:a.mp3] <b>bold</b>` → `front a.mp3 bold`; `one<br>two` → `one two`; `<img src="cat.png">` → `cat.png`.

### C7.2 Question and Answer (`question`, `answer`, R1119, R1120)

C10.

### C7.3 Card Type (`template`, R1121)

Text: the name of the card's template (C1). For a cloze note type: that name, one U+0020, and the card's ordinal + 1 in decimal: `Cloze 1`, `Cloze 2`, `Cloze 10`.

### C7.4 Deck (`deck`, R1122, R1268)

Text: the full name of the deck `did` names. When the card is filtered: that name, ` (` (U+0020, U+0028), the full name of the deck `odid` names, `)`: `Filtered 1 (Japanese::Mining)`, `Cram (Spanish::Verbs)`. The deck shown first is the filtered deck the card is in; the one in parentheses is its home deck.

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

The UI shows an instant as its local date (C12). Rule 4 adds whole multiples of 86,400 seconds to now: it does not step calendar days, so when a daylight-saving change lies between now and the due day, the date shown can differ by one from "today's date plus the days" (scenario `browser/columns/due-dst`).

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

### C7.6 Ease (`cardEase`, R1124)

`type` 0: the **new** marker. Otherwise the **percent** `factor` ÷ 10, rounded down: 2,500 → 250, 2,505 → 250, 1,300 → 130. A learning card whose `factor` is still 0 shows 0, not the new marker.

### C7.7 Interval (`cardIvl`, R1125)

`type` 0: the **new** marker. `type` 1: the **learning** marker. `type` 2 or 3: the **time span** of ((`ivl` × 86,400) reduced modulo 2^32) seconds, as an `f32`. A relearning card with `ivl` 0 shows a span of 0 seconds.

### C7.8 Reviews, Lapses (`cardReps`, `cardLapses`, R1126)

The **count** `reps`, the **count** `lapses`.

### C7.9 Created, Card Modified, Note Modified (`noteCrt`, `cardMod`, `noteMod`, R1127)

**Date-times**: the note id divided by 1,000 and rounded down (epoch seconds); the card's `mod`; the note's `mod`. The UI shows date and time of day to the minute in local time.

### C7.10 Note Type, Tags (`note`, `noteTags`, R1128)

Text: the note type's name. Text: the note's tags in stored order (the stored text split at U+0020 and U+3000, empty parts dropped), joined by one U+0020; empty text when the note has none.

### C7.11 Position (`originalPosition`, R1129)

The card data `pos` when present: the **position** `pos`. Otherwise, when `type` is 0: the **position** `due` (the stored value: a new card in a filtered deck shows its filtered-deck position, such as −100000). Otherwise empty.

### C7.12 Stability, Difficulty, Retrievability (`stability`, `difficulty`, `retrievability`, R1130)

Without a memory state all three are empty (a card whose data holds `s` but no `d` shows nothing here, although `browser/sorting` S4 sorts it by `s`).

- Stability: the **time span** of `s` × 86,400 seconds, the product in `f32`.
- Difficulty: the **percent** ((`d` − 1) ÷ 9) × 100, each step in `f32`, rounded to the nearest integer, an exact half to the even neighbour: `d` 5.0 → 44, `d` 3.0 → 22, `d` 9.0 → 89.
- Retrievability: C11.

## C8. Cells in notes mode

| Column | Value |
|---|---|
| Sort Field, Created, Note Modified, Note Type, Tags | as C7 for the note |
| Question, Answer | C10 for the first card |
| Cards (`template`) | the **count** of the note's cards |
| Deck | when the note's cards have more than one distinct `did`: the **deck count**, that number; otherwise C7.4 for the first card |
| Due | among the note's cards that are not filtered, have `type` and `queue` both not 0, and have a non-negative `queue`: the earliest of their C7.5 instants (rules 3 and 4; a card for which neither applies is skipped); empty when there is none. Never wrapped (R1144, AE50) |
| Avg. Ease | the cards with `type` not 0: none → the **new** marker; otherwise the **percent** ⌊⌊S ÷ n⌋ ÷ 10⌋, where n is their number and S the sum of their `factor` values reduced modulo 65,536 (C13) |
| Avg. Interval | the cards with `type` 2 or 3: none → empty; otherwise the **time span** of ⌊((sum of their `ivl`) × 86,400 reduced modulo 2^32) ÷ n⌋ seconds as an `f32`, n their number (C13). Learning cards are not counted |
| Reviews, Lapses | the **count** of the sum over the note's cards |
| Card Modified | the **date-time** of the largest `mod` of the note's cards |
| Position, Stability, Difficulty, Retrievability | C7.11 and C7.12 for the first card |

Example: a note with a review card (`factor` 2,500, `ivl` 10) and a learning card (`factor` 0) shows Avg. Ease 125 and Avg. Interval 10 days; a note with only new cards shows the new marker, an empty Avg. Interval and an empty Due.

## C9. Value types

| Type | Carries | Used by |
|---|---|---|
| text | a string (C6 direction for Sort Field) | Sort Field, Question, Answer, Card Type, Deck, Note Type, Tags |
| count | a non-negative integer | Cards, Reviews, Lapses |
| deck count | an integer ≥ 2 | notes-mode Deck |
| percent | an integer | Ease, Difficulty, Retrievability |
| position | an integer | Position |
| instant | epoch seconds, shown as a local date | Due |
| date-time | epoch seconds, shown as local date and time to the minute | Created, Card Modified, Note Modified |
| new position | an integer, shown as `New #` and the number | Due |
| filtered | nothing | Due |
| new marker, learning marker | nothing | Ease, Interval |
| time span | seconds as an `f32` | Interval, Stability |
| empty | nothing | any column |
| render outcome | C10 | Question, Answer |

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
8. **Cache** (Meguri, R1242, U85): rendered cells may be kept in an in-memory LRU keyed by everything that can change them: card id, ordinal, `did` and that deck's mtime, `odid` and that deck's mtime, `flags`, note id and note mtime, note type id and mtime. So Change Deck, a flag change, a deck rename (a parent's rename changes each child's mtime), a note edit and a note type edit all miss the cache (R960).

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
2. otherwise, when the card counts in days (C1): with T the home due read as epoch seconds when `queue` is 1, otherwise now + (home due − today) × 86,400, and L = T − `ivl` × 86,400, E = now − L, or 0 when that is negative;
3. otherwise: w(now − home due).

The value is the **percent** R × 100 in `f32`, rounded to the nearest integer, an exact half to the even neighbour, where R is `search/card-extractors` X5 steps 3 to 5 with E in place of e: the card's memory state (C1) and its data's `decay`, or 0.5 when the data has none. Examples (scenarios `browser/columns/cards-mode` and `notes-mode`): `s` 10, `d` 3, no `lrt`, a review due in 3 days with `ivl` 10 → E = 7 days → 93; due in 4 days with `ivl` 4 → E = 0 → 100.

- When `lrt` is present and lies in 0..now, E is X5's e (an `lrt` that is negative or at least 2^32 can give a different E: X5 wraps each operand before a subtraction floored at 0, rule 1 wraps the difference), so the cell shows the value the `prop:r` search compares and `browser/sorting` S7 sorts by. Without `lrt`, rules 2 and 3 differ from X5 step 2: a relearning card with no `lrt` whose timestamp due is later today shows a very low retrievability here (rule 3 wraps around) while the sort counts 0 elapsed seconds. Both reproduce Anki.

## C12. Anki's English display

What Anki 26.09.3 shows in English for each value. The conformance observation (`specs/conformance/browser-ops.md` B4) compares these forms in structured shape. Where a requirement quotes them (R1123–R1125), the UI uses the strings of C14.

| Value | Display |
|---|---|
| text | the text |
| count, position, deck count | the decimal number; a deck count in parentheses: `(2)` |
| percent | the decimal number and `%`: `250%`, `7%` |
| instant | the local date `YYYY-MM-DD` |
| date-time | the local date and time `YYYY-MM-DD @ HH:MM` |
| new position | `New #5`, `New #-100000` |
| filtered | `(filtered)` |
| new marker, learning marker | `(new)`, `(learning)` |
| wrapped | the display in parentheses: `(2030-01-17)`, `(New #5)`, `((filtered))` |
| empty | nothing |
| time span | amount and unit, as below |

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
3. The amount's text: the amount written with two decimals (the `f32` value widened exactly to 64 bits, rounded to two places, an exact half to the even digit), then trailing zeros removed, then a trailing `.` removed: 1.3 → `1.3`, 13 → `13`, 82.2 → `82.2`.
4. Anki writes `<amount> <unit>`, the unit singular when the amount is exactly 1: `1 day`, `10 days`, `1.3 months`, `82.2 years`, `0 seconds`.

Examples: `ivl` 10 → 10 days; 40 → 1.3 months; 45 → 1.5 months; 364 → 12 months; 365 → 1 year; 30,000 → 82.2 years; stability 12.5 → 13 days (12.5 days rounds half away from zero).

Anki also wraps each number it inserts into these texts in the isolation marks U+2068 and U+2069; they are not part of any value.

## C13. Arithmetic kept from Anki

Meguri computes these exactly as Anki 26.09.3 does, overflow included, so that every cell matches the oracle (R1886):

- Avg. Ease sums `factor` values modulo 65,536 before dividing: a note with 27 review cards at 2,500 shows 7, not 250 (scenario `browser/columns/aggregates`).
- Avg. Interval multiplies the summed days by 86,400 modulo 2^32 before dividing: two review cards with `ivl` 30,000 show 14.1 years, not 82.2.
- Retrievability takes now − `lrt` and now − due modulo 2^32 (C11).
- Due adds days as multiples of 86,400 seconds (C7.5).

Decided 2026-10-27: Meguri keeps these results and shows no "true" average. Changing them would be a deliberate deviation, which needs a requirement amendment and a `conformance/deviations.yaml` entry against the scenario's golden.

## C14. Strings

The UI takes these strings only from this table (role: `docs/process/roles/spec-writer.md`):

| Use | String |
|---|---|
| Due of a new card | `New #` followed by the position |
| Due of a filtered card | `(filtered)` |
| Ease or Interval of a new card | `(new)` |
| Interval of a learning card | `(learning)` |
| a wrapped Due | `(` value `)` |
| Deck of a filtered card | deck name, ` (`, home deck name, `)` |
| notes-mode Deck spread over decks | `(` count `)` |
| Card Type of a cloze card | template name, U+0020, number |
| column labels | C2 |

## C15. Scenarios

Observation: `browser_rows` (`specs/conformance/browser-ops.md` B3, B4). Each scenario needs its golden (browser-ops B6).

| Scenario | Covers |
|---|---|
| `browser/columns/cards-mode` | C4, C5, C7 for every card state: new, suspended new, buried new, learning, relearning, interday learning, review (overdue, today, future), suspended review, suspended learning (empty Due), filtered new and review; flags 1 and 9, marked (AE44), ease truncation, interval spans from 0 s to years, FSRS cells with and without `lrt`, Position from `pos` and from `due` |
| `browser/columns/notes-mode` | C8: first-card values (a note whose first card is ordinal 1), Cards count, deck count, earliest Due over eligible cards only, AE50's note with only new cards, averages with learning cards, sums, latest Card Modified, marked tint |
| `browser/columns/question-answer` | C10: answer prefix removal, FrontSide empty, `<br>` and `<div>` to spaces, image and sound names, TTS and other directives, cloze question and answer, notes mode on the first card |
| `browser/columns/due-dst` | C7.5 rule 4 across a daylight-saving change; local dates and date-times in a zone with an offset |
| `browser/columns/aggregates` | C13: the 16-bit ease sum and the 32-bit interval product |
| `browser/columns/row-errors` | C3.1 in both modes: a deleted card, a deleted note, a note without cards (notes mode `damaged`), a card whose deck is missing |

- These six scenarios and their goldens are not on this branch yet: they need the `browser_rows` observation, which neither the oracle driver nor Meguri's runner implements here (browser-ops B6). They come back with that observation, their goldens checked against the scenarios as ported (or re-recorded when a scenario changed).
- The cards-mode Question and Deck cells (C7.2, C7.4, C10 steps 1 to 3) and the cards-mode row errors also have the `card_excerpts` scenarios of C16.6, which are active now: `question-deck/browser-format`, `media-line`, `cloze` and `deck` for the cells; `question-deck/missing` for `deleted` (the excerpt's `missing`); `question-deck/damaged` and `damaged-notetype` for `damaged` (a missing deck, home deck, note, note type or template). `row-errors` adds what only Browse rows show: the notes-mode errors (a deleted note, a note without cards).

Not covered by scenarios yet: browser answer formats (`bafmt`) and fonts (`bfont`, `bsize`), which `update_notetype` template edits can set (`notetype-ops.md` T5) but no active observation reads; RTL sort fields (field config); template errors and blank cloze cards in Question and Answer (Anki's cells hold localized messages, R1887). U84's unit tests cover them from this text.

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
