# Browser columns and rows

Spec: browser/columns
Requirements: R1119, R1122, R1268; supports R96, R935, R976, R1859
Anki: 26.09.3
Scenarios: conformance/scenarios/browser/columns/question-deck/

What Browse shows for one card, and the card excerpts call (C16) that gives the review summary a card's Question and Deck cells before Browse exists. This file is written in two passes. This pass (U85a) holds the terms, the row errors, the cards-mode Deck cell, the Question and Answer render path and the card excerpts call. U84's pass adds the column catalog, the row record, the other cells, notes mode and Anki's English display, under the section ids this text leaves free (C2, C3 apart from C3.1, C4 to C6, C7.1 to C7.3, C7.5 to C7.12, C8, C9, C11 to C15), and keeps the ids used here. Column keys are `browser/config-keys` K3.

## C1. Terms

- **Mode.** Cards mode has one row per card, notes mode one row per note (R1115). Everything in this pass is cards mode.
- **Card columns.** As stored and read by `model/row-records`: `nid`, `did` (the deck the card is in), `ord` (the template ordinal; for a cloze card, the cloze number minus one), `odid` (the home deck of a card in a filtered deck, 0 otherwise).
- A card is **filtered** when `odid` is not 0.
- **Full deck name.** A deck's stored name in its human form, the components joined by `::` (`model/schema11-json` S11), as Browse and the deck list show it: `Japanese::Mining`.
- **The template** of a card: for a standard note type, the template whose ordinal is the card's `ord`; for a cloze note type, its first template, whatever the card's ordinal (`render/card-render-pipeline` K2).

## C3. Row records

The row record is U84's. Its errors, which the card excerpts of C16 also report, are C3.1.

### C3.1 Row errors

| Error | When |
|---|---|
| `deleted` | the row's own record no longer exists: in cards mode the card, in notes mode the note |
| `damaged` | in cards mode the card exists but its note, its note type, its deck (`did`), its home deck (`odid`, when not 0) or its template (C1) does not (whatever columns were requested); in notes mode the note exists but has no card, or one of those records is missing for its first card |

Check Database repairs damaged rows (U128). A row error never stops the other rows of the request.

## C7. Cells in cards mode

The row's card is "the card". This pass gives the Question cell (C7.2) and the Deck cell (C7.4); the other cells are U84's.

### C7.2 Question and Answer (`question`, `answer`, R1119, R1120)

C10.

### C7.4 Deck (`deck`, R1122, R1268)

Text: the full name of the deck `did` names. When the card is filtered: that name, ` (` (U+0020, U+0028), the full name of the deck `odid` names, `)`: `Filtered 1 (Japanese::Mining)`, `Cram (Spanish::Verbs)`. The deck shown first is the filtered deck the card is in; the one in parentheses is its home deck.

- Only the two names count. Whether the deck `did` names is itself a filtered deck plays no part, nor do the card's other columns (`odue`, `queue`, `due`).
- Nothing is shortened: elision is the UI's (U84's catalog shortens deck names in the middle).
- Examples: a card in `Japanese::Mining` → `Japanese::Mining`; a card in `Default` → `Default`; a card with `did` naming `Filtered 1` and `odid` naming `Japanese::Mining` → `Filtered 1 (Japanese::Mining)`.

## C10. Question and Answer

Steps 1 to 3, 5, 6 and 7 make the Question cell; step 4 is the Answer cell and step 8 the row cache, both U84's. The card excerpts of C16 use the Question cell only.

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
