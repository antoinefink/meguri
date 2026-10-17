# Card render pipeline

Spec: render/card-render-pipeline
Requirements: R469, R470, R472, R525, R527, R528, R531, R559, R583, R935, R960, R961, R976, R1907, R1908
Anki: 26.09.3
Scenarios: conformance/scenarios/render/pipeline/, conformance/scenarios/render/errors/, conformance/scenarios/render/av-tags/

How the engine renders a stored card for review, preview and the browser: which templates it uses, the field map, the order of template rendering, the blank check, audio extraction and FrontSide, what it returns, and what it never does. The template engine is `template-syntax.md`, `filters.md`, `special-fields.md` and `cloze.md`; audio tags are `av-tags.md`; review HTML is `display-transform.md`. The `render` observation (`specs/conformance/render-ops.md` Q5) compares the K4 and K5 outputs with Anki's.

## K1. Modes

- **review**: a card in a study session.
- **preview**: the card previewer and other read-only card views. Its outputs equal review's; the mode only tells U64 and U71 which type-in form to show.
- **browser**: the Browse question and answer columns (U84), with the browser templates.

## K2. What is rendered

- The card row, its note and the note's note type, as stored now. A card whose note or note type does not exist, or a card of a standard note type with no template at its ordinal, cannot be rendered: the render fails with a not-found error, which is not a template error (Check Database repairs such cards, U128).
- The **template**: for a standard note type, the template whose ordinal is the card's `ord`; for a cloze note type, its first template, whatever the card's ordinal.
- The note type's **kind** (standard or cloze) and its **CSS** (`model/protobuf-records`, note type config).
- The card **ordinal** is the card's `ord`. For a cloze card it is the cloze number minus one.

## K3. Field map

Built from the stored card, note and template exactly as `special-fields.md` P2 and P3 say (R960): the note's fields, then `Tags`, `Type`, `Deck` (the original deck for a card with `odid` set), `Subdeck`, `Card` (the K2 template's name), `CardFlag` (`flag` + the whole `flags` value), `CardID`, and `c<ord+1>`, each special only when no note field has its name. `FrontSide` is never in the map; the engine resolves it (P4, and K4 step 4 below).

## K4. Review and preview

For one card:

1. **Question.** Render the template's question text (`qfmt`) with the field map, the card ordinal, the question side and the note type's kind (`template-syntax.md` T1–T8, `cloze.md` for the `cloze` filters). On an error, the card's outcome is that error with side `question`; stop.
2. **Blank check** (`template-syntax.md` T10, `cloze.md` C15). When the card is blank, its outcome is **blank**, with kind `empty_front` for a standard note type and `missing_cloze` for a cloze note type. The question output of step 1 is kept and goes through step 3; the answer template is not rendered (so its errors stay hidden, T9) and the answer has no AV entries.
3. **Question audio.** Extract audio from the whole question output with side letter `q` (`av-tags.md`). The result is the **question text** and the question AV list.
4. **Answer.** Render the template's answer text (`afmt`) with the same field map, ordinal and kind, on the answer side, with the question text of step 3 as the FrontSide text (`special-fields.md` P4). On an error, the outcome is that error with side `answer` (the question's outputs are dropped).
5. **Answer audio.** Extract audio from the whole answer output with side letter `a`. The result is the **answer text** and the answer AV list.

Outcomes:

| Outcome | Question text | Answer text | Question AV | Answer AV |
|---|---|---|---|---|
| rendered | step 3 | step 5 | step 3 | step 5 |
| blank (`empty_front` or `missing_cloze`) | step 3 | none | step 3 | empty |
| template error (kind, side) | none | none | empty | empty |

Consequences:

- The answer of `{{FrontSide}}<hr id=answer>{{Back}}` starts with the question text exactly, play markers included, and without any style block (R469; the style block is added later, `display-transform.md` D9).
- The answer's FrontSide part keeps the question's `[anki:play:q:N]` markers, and the answer AV list holds only the answer's own tags (R528, R961, `av-tags.md` A8).
- Type-in markers (`filters.md` F7) stay in both texts exactly as the template engine wrote them, the answer's FrontSide part included; U64 turns them into the box and the comparison (`display-transform.md` D2 step 3), and the `render` observation compares them as text (R1908). The same holds for the K5 texts.
- A blank card still has its question audio: a question template made of `Listen [sound:x.mp3]{{#Extra}}{{Extra}}{{/Extra}}` with Extra empty is blank, its question text is `Listen [anki:play:q:0]`, and `x.mp3` is its question entry.

The display HTML for a host is built from these outputs by `display-transform.md` (R470, R526, R527, R531, R564).

## K5. Browser

1. **Templates.** The question template is the template's browser question format (`q_format_browser`, schema-11 `bqfmt`) when it is not the empty string, otherwise `qfmt`; independently, the answer template is the browser answer format (`a_format_browser`, `bafmt`) when not empty, otherwise `afmt` (R935). A format of only white space is not empty and is used. Note type saves do not check browser formats, so they may hold template errors that the review templates do not have.
2. **Question.** As K4 step 1 with the browser question template. An error has side `question`.
3. **Blank check: cloze only.** A card of a cloze note type is checked as in C15 and, when blank, its outcome is blank with kind `missing_cloze` and no answer. A card of a standard note type is never blank in browser mode (R976), even when its question renders nothing.
4. **Answer.** As K4 step 4 with the browser answer template, except that `{{FrontSide}}` outputs nothing in browser mode. An error has side `answer`.
5. **No audio extraction.** Sound tags and directives stay exactly as the templates produced them.

Outcomes: rendered (question and answer text), blank (`missing_cloze`: question text, no answer), or template error (kind, side). U84 turns these into column text, including how the answer column leaves out a leading copy of the question.

## K6. Errors

Template errors are the five kinds of `template-syntax.md` T9 with the side of the template that failed. The question side is checked first; the answer side only when the question rendered and was not blank. The message is the template engine's own (`template-syntax.md` T9); the display wraps it as `display-transform.md` D8 says.

## K7. Rendering changes nothing (R583)

Rendering in any mode only reads. It writes no note, card, note type, deck, config key or media file, records no undo step and emits no change event: `mod`, `usn` and fields of the note stay as they were, whatever the templates hold (MathJax, `{{tts}}` filters, unknown filters such as add-on filters, `[latex]` text). In particular no media file is ever generated.

## K8. Not part of the pipeline

- **LaTeX.** Anki's review rendering also turns `[latex]…[/latex]`, `[$]…[/$]` and `[$$]…[/$$]` into references to generated images (and appends an error when an image is missing and cannot be made). That step is outside this pipeline and outside the `render` observation; whether Meguri converts these tags so that already generated images show (R565) is open in the plan. Until it is decided, the text stays as written.
- **Media-name encoding and the type-in box** are U64's display steps (`display-transform.md` D2).
- **Rendering a note not yet added** (the template editor, the Add screen's preview) is U71's.

## K9. Config values for the card page

Read from the collection config when a card is rendered:

| Key | Type | Missing or another type | Use |
|---|---|---|---|
| `hideAudioPlayButtons` | boolean | `false` | `display-transform.md` D6 (R527) |
| `reviewExtra` | string, HTML | empty | appended after `#qa` by the card page (R472, U66), exactly as stored |

`hideAudioPlayButtons` is the preference "Show play buttons on cards with audio", inverted. Anki inserts `reviewExtra` when it builds the review page and prints a non-string value as Python text; Meguri uses only a string value.

## K10. Card state for the page

For `_drawFlag` and `_drawMark` (R478, U66):

- **flag**: the card's `flags` value with only its low three bits (0–7): a stored 9 gives flag 1, unlike `CardFlag` (P3), which shows the whole value;
- **marked**: the note has a tag equal to `marked` after both are lower-cased with the Unicode default lowercase mapping (`Marked` and `MARKED` count).

## K11. Meguri contract

- **Call.** One engine call renders one card in one mode for one host and returns a `RenderedCard`: card id, ordinal, mode; for review and preview, the question and answer display HTML (`display-transform.md` D9) and both AV lists (sound: name; TTS: language and content); for browser, the K5 texts; the outcome (rendered, blank with its kind, or template error with kind and side); the note type CSS; the K9 values; the K10 flag and marked state; a type-in descriptor that U64 fills (none until then). It runs on the interactive lane (KTD9, R1857) and never on the main thread.
- **Comparable form.** The engine also exposes the K4 and K5 outputs before any display step, which the testkit's `render` executor reports (`render-ops.md` Q5); its `render_status` executor reports the K4 outcome as a status (Q4).
- **Parse cache.** Parsed templates may be cached per note type. A note type's `mtime_secs` has one-second resolution, so two edits within a second keep it: a cache keyed by note type id and mtime must also be cleared on every note type change event (local edit, undo, sync, import, Check Database, collection replaced), or be keyed by the template texts themselves. A render never uses a parse of text that is no longer stored.

## K12. Differences from the Product Contract's wording

- R976 places both notices outside Browse. In Anki 26.09.3 the missing-cloze check also runs in browser mode (K5 step 3); only the empty-front check is skipped there. Meguri follows Anki, since U84's columns must match.
- R531 and R959 disagree on template-error wording; `display-transform.md` D11 records the choice.

## K13. Scenarios

- `render/pipeline/basic`: K4 on a custom Basic-like note type with a reverse template: FrontSide holds the question text with its play markers (R469, R528).
- `render/pipeline/specials`: K3 through stored templates: nested decks, `Subdeck`, a card with `odid` set showing its original deck, flags, tags, the template name (R960).
- `render/pipeline/blank`: K4 step 2 and K5 step 3: an empty front in review but not in browser mode, a blank question that keeps its text and audio, a missing cloze in both modes (R976).
- `render/pipeline/browser`: K5 step 1 and 4: browser formats, the fallback per side, a white-space-only format, FrontSide empty, errors only in browser formats (R935).
- `render/pipeline/type-in`: type-in markers through K4 and K5: standard, `nc` and cloze markers kept as text, FrontSide carrying the question's marker, audio around them still extracted (R1908).
- `render/pipeline/read-only`: K7: the notes and cards tables before and after rendering MathJax, `{{tts}}`, `{{hint}}` and an unknown filter (R583).
- `render/errors/stored`: K6 through stored templates: errors on each side, a blank question that hides an answer error in review but not in browser mode, a missing cloze that hides it in both.
- `render/av-tags/*`: `av-tags.md` A12.
- `render/special-fields/cards` (`template` card cases, `render-ops.md` Q1.3): the field map of stored cards; U62 makes Meguri's executor run it.
- Observation kinds: `render` (Q5), `render_status` (Q4, the review outcome as a status only; `notetypes/records/stock-render-handoff` observes it) and `template` (Q1).
