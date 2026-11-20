# Template and note previews

Spec: render/preview
Requirements: R938, R939, R940, R941, R1835; supports R442, R502
Anki: 26.09.3
Scenarios: conformance/scenarios/render/preview/
Status: Approved 2026-11-20 by Antoine in chat, after an independent review and five reviewer rounds (`docs/process/review-log/browse-note-types-specs-20261120.md`) (R915; round 9, decided 2026-11-20) for the lines of V12 that begin "Changed 2026-11-20 (round 9)". Every other section is in force and unchanged.

How the engine renders a card that is not stored as it would look: a note type draft (the template editor, U200 and U202) or a note draft (the Add and Edit previews, U190 and U195), with Fill empty fields and the simulated type-in. The pipeline is review's (`card-render-pipeline.md` K4, `display-transform.md`), with the inputs this file names. U71 owns the engine call and the preview page. Debounce, layout and pane switching are U200's and U202's.

## V1. Terms

- **Note type draft**: a note type as an editor holds it, saved or not: name, kind (standard or cloze), field names in order, templates in order (each a name, a question text and an answer text) and CSS. A draft of a stored note type keeps that note type's id.
- **Note draft**: field values in the draft's field order, a tag list, and the id of a stored note when the values are that note's (edited or not). A new note has no id.
- **Target**: the card ordinal to show. For a standard draft it is a template index (0 for the first template). For a cloze draft it is the cloze number minus one. The target a template editor opens at is `notetypes/editor-actions` A5.
- **Preview render**: one render of one target of a note draft with a note type draft, in preview mode (`card-render-pipeline.md` K1).

## V2. The engine call (Meguri contract)

`renderUncommitted(notetypeDraft, noteDraft?, target, fillEmpty, platform)` returns one `RenderedCard` (`card-render-pipeline.md` K11) in preview mode and the **preview note**: the id (0 for a new note), values and tags of the note it rendered, before V6.

- `fillEmpty` is a boolean (V6). `platform` is `display-transform.md` D1's `mac` or `ios`, for the device actually running the page (V12). The host's `hidePlayButtons` is not a caller input: the engine reads it from the call's snapshot (K9), as D1 says.
- `noteDraft` is optional. When the caller gives one (V5 rules 1 and 4, V13), it is used as given. When it is absent, the engine applies V5 rules 2 and 3 in the call's snapshot: the stored note of `notetypeDraft`'s id with the smallest id, or, when the draft has no id or that note type has no notes, a new note with every value empty and no tags. A stored note's tags are its stored tag list (`special-fields.md` P3). Its stored values are matched to the draft's fields by position: missing trailing values are empty and values beyond the draft's field count are dropped.
- The preview note lets the caller show which note it is and list its cards (V6). The type-in is always simulated (V9). There is no typed text and no reveal call.
- Field values of a given note draft: the i-th value belongs to the i-th draft field. Missing trailing values are empty.
- Field names and template texts are taken as given. A draft with no fields renders with the special values only. When two draft fields have exactly the same name, the first one in field order gives the map's value; the later one is not reachable by name.
- Refusals, each without a render:

| Input | `CoreError` |
|---|---|
| a given note draft with more values than the draft has fields | `InvalidArgument` |
| a target outside 0 to 65534 | `InvalidArgument` |
| a standard draft with no template at the target index (a draft with no templates included) | `NotFound`, like a missing template in K2 (not a template error) |
| a cloze draft with no template | `NotFound` |

- The call runs on the interactive lane and never on the main thread (K11). It reads the collection for the note choice, V4 and K9 from one reader snapshot (K11 read consistency). A newer call from the same preview replaces an older one: the caller cancels the older one or drops its result, and a dropped result is never shown.

## V3. Templates, kind and CSS

- The template is the draft's template at the target index for a standard draft, and the draft's first template for a cloze draft (K2's rule, applied to the draft).
- Template texts are used as given. Nothing validates them: unknown fields, structural errors and templates a save would refuse render as template errors (`template-syntax.md` T9). Browser formats play no part.
- The kind is the draft's. Besides choosing the template and the meaning of the target (V1, and the first bullet above), it decides only the blank check (T10, `cloze.md` C15).
- The CSS is the draft's CSS, as given, in the style block of `display-transform.md` D9. It is never the stored CSS.

## V4. Special fields

The field map follows `special-fields.md` P2 and P3, with these values:

- `Type`: the draft's name. `Card`: the V3 template's name in the draft.
- `Tags`: the note draft's tags as given, in order, joined by one U+0020.
- The **card** behind `Deck`, `Subdeck`, `CardFlag` and `CardID`:
  - when the note draft (or the note V2 picked) has a stored note id and that note has a stored card whose `ord` equals the target, that card as stored (its `did`, `odid`, `flags` and `id`);
  - otherwise a card that does not exist: `Deck` and `Subdeck` `(Deck)`, `CardFlag` `flag0`, `CardID` `0`.
  - A note id whose note no longer exists in the call's snapshot, or whose note belongs to a note type other than the draft's id (or the draft has no id), gives the card that does not exist. It is not an error.
- The card is found by ordinal alone. When a draft has moved or added templates, the stored card at the target's ordinal can belong to another template. This changes only those four values.
- `c<target + 1>` is `1`, unless a field has that name (P2).

## V5. Field values

- Values are used exactly as the editor holds them. Nothing applies the save normalization (`text/checksum-and-sort-field#C1`). Emptiness, for sections, for the blank check and for V6, is `text/field-emptiness#E1` on the given text. The `template` observation's new-note cases do the same (`conformance/render-ops#Q1.2`).

| Given value | Empty in a preview | Empty once saved (E2) |
|---|---|---|
| `<br>` + LF | yes | yes |
| U+0001 | no | yes |
| `<b` U+000D `r>` | no | yes |
| `&nbsp;` | no | no |

- **Which note** (R938):
  1. Opened from an editor (Add, Edit, Browse's editor, the review screen's Edit): that editor's note, with its unsaved values and tags, and its stored id when it has one. The caller passes it as the note draft. Fill-empty starts off.
  2. The template editor opened from note type management: the type's first note, which is the stored note of that note type with the smallest id. Its stored values and tags are used. The caller passes no note draft and the engine picks it (V2). Fill-empty starts off. Anki's Cards… from its Note Types list previews a new empty note with fill-empty on instead, whatever the note count (V15). Open question for Antoine: this rule follows R938's "or the type's first note"; following Anki would change only this rule.
  3. A note type with no notes: a new note with every value empty and no tags, also picked by the engine (V2). Fill-empty starts on (as Anki does when its card layout screen is opened from note type management). The user can turn it off, as in Anki.
  4. The template editor opened from Browse's sidebar, Edit Template on a card type child (`browser/sidebar` S9.7, `notetypes/editor-actions` A5): a new note with every value empty and no tags, as in rule 3, with fill-empty on, even when the note type has notes (as Anki does). Since the engine picks the first note whenever the note type has notes (V2), the caller passes this new note as the note draft: no id, no tags and no values, which V2 reads as all empty. The user can turn fill-empty off.
- With no notes and fill-empty off, the preview shows what review would show for such a note: usually a blank card (`display-transform.md` D7). It shows a template error only when the draft has one.

## V6. Fill empty fields (R939)

With `fillEmpty` on, the field map is built from filled values:

- **Which values.** A value that is empty (E1 on the given text) is filled. A non-empty value is kept as it is. Special values are never filled. A note field named like a special (`Tags`) is a field like any other and is filled when empty.
- **What it becomes.** The V7 sample cloze text when the field's name is a cloze reference of the V3 **question** template, compared by exact code points (T1). Otherwise `(` + the field name + `)`, with the name as it is: no escaping, no translation (`A&B` gives `(A&B)`).
- **Cloze references** are T11.1's: the keys of replacements whose filter chain has a piece exactly equal to `cloze`, anywhere outside comments, inside sections included. So `{{cloze:Text}}`, `{{type:cloze:Text}}`, `{{text:cloze:Text}}` and `{{#Text}}{{cloze:Text}}{{/Text}}` reference `Text`. `{{cloze-only:Text}}`, `{{Cloze:Text}}` and an answer-only `{{cloze:Text}}` do not.
- **No fill on a broken question.** A question template with a structural error (`template-syntax.md` T6) fills nothing. The render then fails on that error anyway.

Consequences:

- Every empty field is filled, including fields that only the answer template uses or that no template uses.
- Filled values count as non-empty everywhere: `{{#Extra}}` shows, `{{^Extra}}` hides, a question made of one empty field is no longer blank, and FrontSide carries the filled question.
- The rule ignores the draft's kind. A standard draft whose question uses `{{cloze:Text}}` gets the sample when `Text` is empty.
- The sample holds cloze 1 only. With an empty cloze field filled, card 1 shows the sample's cloze. Any higher card of a cloze draft whose other values hold no cloze of its number stays blank (`missing_cloze`, `cloze.md` C15).
- The card list of a cloze draft (U202's card picker) comes from the unfilled values, never from the filled ones: the cloze numbers of `cloze.md` C5 in the preview note's values (V2), plus the current target's number when it is missing, in ascending order. So an empty cloze note opened at target 2 lists only 3, as in Anki.
- Filling happens only in the render's own copy of the values (V10).

| Fields (values) | Question / answer | Card | Values used |
|---|---|---|---|
| Front `neko`, Back empty, Extra empty | `{{Front}}` / `{{Back}}<br>{{Extra}}` | 1 | `neko`, `(Back)`, `(Extra)` |
| Text empty, Extra empty (cloze draft) | `{{cloze:Text}}` / `{{cloze:Text}}{{Extra}}` | 1 | sample, `(Extra)` |
| Text empty (cloze draft) | `{{cloze:Text}}` | 2 | sample: blank `missing_cloze` |
| Text `<br>` | `{{cloze-only:Text}}` | 1 | `(Text)` |
| Text `&nbsp;` | `{{cloze:Text}}` | 1 | `&nbsp;` (not empty) |

## V7. Sample cloze text

The English sample, exactly:

`This is a {{c1::sample}} cloze deletion.`

- It is a field value, rendered like any other: `cloze.md` C6 turns it into card 1's cloze on both sides.
- The engine writes this English text in every interface language, as it does the blank notices (`display-transform.md` D7). It has no String Catalog entry and `renderUncommitted` takes no localized text, so the oracle, the conformance runs and Meguri's tests all see this text. Anki translates it with its interface language (V15).
- If a later unit translates it, the caller passes the translated text to the engine, and a test over every translation checks the rule that keeps V6 and C6 working: exactly one cloze, `{{c1::` + a word + `}}`, and no other `{{` or `}}`.

## V8. Rendering

A preview render is `card-render-pipeline.md` K4 steps 1 to 5 on the V3 template, the V4 field map and the V5 or V6 values, with the draft's kind. Its outcomes are K4's: rendered, blank (`empty_front` or `missing_cloze`) or template error (kind and side). Then come the display steps of `display-transform.md` D2:

1. media-name encoding (`media-display.md` D1);
2. play markers, replay buttons and TTS content (D3 to D6), with `hideAudioPlayButtons` from the call's snapshot (K9);
3. in place of review's type-in (`type-answer.md` A4, A5): the V9 simulation;
4. the style block, with the draft's CSS (D9).

Steps 1 to 3 see the whole side text with its `<style>` element and the draft's CSS in it, as `type-answer.md` A3 says for review and as Anki's preview does. Running them on the side without that element and adding it last (D2) gives the same output unless the draft's CSS itself holds a type-in marker, the text `[[type:`, the exact text `<hr id=answer>` or a media tag with a local name to encode. For such CSS this order holds: a `[[type:` in the CSS alone makes V9 transform the side, and an `<hr id=answer>` in the CSS is removed with the others.

Blank and error notices are D7 and D8, composed as D9 says. LaTeX stays as text and no image is generated (K8).

## V9. Simulated type-in (R941)

`type-answer.md` A11.1 transforms each side: the first marker of the question becomes a read-only box holding `example`, and the first marker of the answer becomes the comparison of expected `example` with typed `sample` in plain mode. Later markers get A11.1's warning, whose text W the engine writes in English in every language, as V7's sample. The note's fields play no part. The comparison is the oracle's `type_answer` `compare` output for the pair `["example", "sample"]` (`render/type-answer/compare`, `render/preview/type-in`). The `RenderedCard` has no type-in descriptor, and the preview never asks for a comparison at reveal.

R941 says `example` is typed on the front and compared with `sample` on the back. Meguri reads it as Anki's preview does: the front's box shows `example`, and the back compares `example` as the expected text with `sample` as the typed text, in that order (`type-answer-ops` K1's `[<expected>, <typed>]`). Tests keep the pair in this order.

## V10. A preview changes nothing

As `card-render-pipeline.md` K7 says: no note, card, note type, deck, config or media write, no undo step and no change event. The V6 values never reach the editor's note or the stored note. A preview render records no answer and starts no timer (R442's behavior).

## V11. Result

The `RenderedCard` of K11, in preview mode:

- card id: the V4 stored card's id, or 0; ordinal: the target;
- question and answer display HTML from V8; both AV lists (for replay buttons; V12 says what plays);
- the outcome; the draft's CSS; the K9 values from the collection;
- K10 state: the stored card's flag, or 0 for a card that does not exist; marked from the note draft's tags by K10's rule.
- Beside it, the preview note of V2.

The comparable form (K11) is K4's question and answer texts before display. The `template` observation reports them without AV extraction (`conformance/render-ops#Q1.4`).

## V12. The preview page (R940, R442, R502)

Changed 2026-11-20 (round 9): the Note Types preview has a Light and Dark switch and no device or platform-class picker (`notetypes/workspace` W9).

U71's `CardPreviewView` shows one card side in a single persistent page (`card-runtime.md`), not a page pair. It uses the shared web view configuration and the persistent website data store of R502, so `localStorage` and IndexedDB are shared with review.

- **Showing a side.** Every side the page shows is one `_showAnswer(side, bodyclass)` call (`card-runtime.md` C6.1): the question display HTML for the front, the answer display HTML (which carries FrontSide) for the back. `bodyclass` is always the full C3.1 string for the target (`card{target + 1}`), the chosen platform and the chosen appearance. The preview never calls `_showQuestion`, as Anki's preview does. So every update sets the body classes, never focuses `#typeans` (V9's box is read-only) and scrolls to `#answer` when the side has one (C6.4).
- **When it shows.** A new `RenderedCard` (after an edit, a target change or a fill-empty toggle) and a front or back toggle each queue one such call with the latest result. Updates are serialized (C6.2): an update in progress finishes, then the newest one runs. A result that V2 dropped is never shown. Each side is shown alone, so a back script cannot rely on the front's script having run just before.
- **Appearance and platform class** are preview options, independent of the device in hand. The page's classes are `card-runtime.md` C2.2 (`<html>`) and C3.1 (`<body>`), computed for the chosen platform and appearance. `data-bs-theme`, the web view's appearance and C5's night-mode rules follow the chosen appearance. Example: iPad and dark on a Mac gives `<html class="night-mode ipad ios mobile safari">` and `<body class="card card1 nightMode night_mode">`. Mac and dark on an iPhone gives `card card1 isMac nightMode night_mode macos-dark-mode`.
- **Changing the appearance** while a side is shown switches classes as C5 says, without an update.
- **Changing the platform** is done by native from the isolated world, with no page-world global (C9): on `<html>`, every class of the previous platform's C2.2 set is removed wherever it is, then the new set is appended in C2.2's order. Other classes, `night-mode` and classes card scripts added, stay where they are. So Mac to iPad in dark turns `night-mode mac safari` into `night-mode ipad ios mobile safari`. Native then shows the current side again at once, with the same display HTML and the new `bodyclass`. The page is not recreated.
- The chosen platform changes classes only. The replay action (`display-transform.md` D4) and the page globals follow the device actually running the page: `pycmd` on Mac, the `meguri` message handler on iOS and iPadOS.
- **reviewExtra.** The preview page includes `reviewExtra` as `card-runtime.md` C2.3 says, as both of Anki's preview pages (the card layout screen and the Add and Edit previewer) do, since they are built like its review page (R472). The value is the K9 `reviewExtra` of the first `RenderedCard` the page is created to show, placed after `#qa`. Its scripts run once, when the page is created. Later updates, target changes, fill-empty toggles, side toggles and platform switches never insert it again. If the key changes while the page lives, the page keeps its old copy until it is recreated. U71's test: card-runtime's SU6 `reviewExtra` in a preview page, then three updates, a side toggle and a platform switch, gives `#qa.nextElementSibling.id === "rx"` and `__rx === 1`.
- **Media.** The preview never plays audio by itself. A replay button plays its entry. Card-authored `<audio>` and `<video>` do not start without a user gesture in the page: `autoplay` and a script's `play()` outside a gesture do not play. A control the user taps plays.
- **Image occlusion.** Note types of the image occlusion kind preview through the same pipeline, and the U68 runtime is part of the preview page as in every card page (`image-occlusion-review.md`).
- It draws no flag or mark (`#_flag` and `#_mark` stay hidden), has no answer gate and calls no scheduling op.

## V13. Add and Edit previews

The Add and Edit screens' card previews are preview renders with the stored note type as the draft (its templates and CSS unchanged) and the editor's note as the note draft (V5 rule 1). The target is the card the user picks: a template index, or a cloze number minus one. Fill-empty is off unless the user turns it on.

They use the same `CardPreviewView` and V12's page. Their appearance and platform class start as the device's own and the app's appearance. The type-in is simulated (V9) here too, as V2 says for every preview render.

## V14. Budget (R1835)

R1835 allows 100 ms from a pause in typing to the updated preview. The engine's share is at most 25 ms at the 95th percentile. The rest belongs to U200's debounce and the page update. Parsed templates may be cached by template text (K11), never by note type id alone, since drafts change without a save.

U71's timing test:

- **Draft.** A standard draft with fields Front and Back (values `neko` and `cat`) and one template. The question is `{{Front}}` followed by a 20 KB inline `<script>`. The answer is `{{FrontSide}}<hr id=answer>{{Back}}` followed by a 10 KB inline `<script>`. The CSS is 2 KB of rules. Fill-empty is off.
- **Cache state.** Each measured call changes one character of the question's script, so the question is parsed cold every time and the answer may come from the cache, as after typing.
- **Runs.** 20 warm-up calls, then 200 measured calls, in a release build.
- **Interval.** From the engine call's entry to its returned `RenderedCard`, inside the core, without the FFI crossing.
- **Where.** On the reference devices (R1290). Until those runs, during the macOS-first pass, the same test runs on the host Mac with the same 25 ms budget as a proxy. The reference-device run remains required for final evidence.

## V15. Differences from Anki

The oracle can observe none of these (they belong to Anki's desktop interface). Meguri's tests check them against this file.

- Anki's template editor opened from note type management shows a new, empty note with fill-empty on. Meguri shows the type's first note (V5 rule 2, R938) and keeps the empty note for types with no notes.
- Anki previews a stored note as saved. One of its editors saves the note before opening the card layout screen, and the other reloads the note from the collection by its id, so the save normalization (`text/checksum-and-sort-field#C1`) can apply there. Meguri previews the editor's unsaved values as given (V5, R938), so the V5 table's U+0001 and `<b` U+000D `r>` rows can differ. For a new note (Add), Anki also uses the unsaved values.
- Anki autoplays the previewed side's audio when the Default deck's options (deck id 1) autoplay, not the options of the previewed card's deck, and only on the first render after the target or the side changes. Card-authored media play without a gesture only then. Meguri never autoplays (V12).
- Anki's mobile option adds only the class `mobile` to `<html>`. Meguri chooses one of its three platform class sets (V12, R940).
- Anki offers Fill empty fields only when some value is empty after Unicode white space is trimmed (that check ignores HTML). So a value of only U+00A0 makes Anki offer the option, although V6 does not fill it (E1). Showing the option is U202's choice. When the option is on, V6 applies as specified.
- Anki takes the note type's name, field names and kind from the stored note type. Its card layout screen cannot change them, so Meguri's use of the draft's (V3, V4: `Type` included) gives the same result.

## V16. Scenarios

| Scenario | Status | Covers |
|---|---|---|
| `render/preview/drafts` | recorded | V3 to V5 through the `template` observation (`conformance/render-ops#Q1`): a stored card with draft templates that match its stored template, and the stored card's specials; unsaved values without save normalization; the no-note case; drafts with errors a save would refuse |
| `render/preview/type-in` | recorded | V9: the markers that A11.1 transforms in a draft's question and answer, and the comparison of `example` with `sample` in both modes |
| `render/preview/fill-empty` | planned | V6 and V7 on new notes: answer-only fields, conditionals, the blank check, cloze references through every filter form, the kind-independence, cloze card 2, special-named fields, unescaped names, E1 edge values |
| `render/preview/fill-empty-cards` | planned | V4 with V6: a stored note with an empty field, previewed with draft templates and fill-empty on, and the stored values unchanged afterwards |

The two planned scenarios have no file and no golden yet. They use the `template` observation's `fill_empty` case key (`conformance/render-ops#Q1`, Q1.4 step 0), which needs the finer capability `template.fill_empty` from Meguri's executor. The oracle driver and the scenario schema accept the key, but the testkit's scenario model refuses it, so `tools/mg goldens-check` would report either file invalid and neither can sit under `conformance/scenarios/` yet. A spec writer writes them and records their goldens on a held branch; implementers do not write scenarios. They land in `conformance/scenarios/render/preview/` and `conformance/goldens/render/preview/` in the same change as U71's activation steps (`conformance/render-ops#Q3`):

1. add the `fill_empty` key to the testkit's scenario model and `template` adapter, on new-note and card cases;
2. declare `template.fill_empty` in Meguri's executor;
3. add `preview/*` to `conformance/scenarios/render/area.yaml`.

Until they land, U71's engine tests check V6's table and V7's text, and the oracle comparison of R939 waits for the two scenarios.

These have no oracle and are covered by U71's Meguri tests (`CardPreviewTests` and the engine's own tests):

- V2's note choice, the preview note and the refusals;
- two V4 rules that the `template` card case cannot reach, because it always uses the card's stored ordinal and stored template name: a draft whose templates were moved or added, so the stored card at the target's ordinal belongs to another template, and `Card` taken from the draft's template name for a stored card;
- V6's card list;
- V8's display steps, including draft CSS that holds `[[type:` or `<hr id=answer>`;
- V12, its `reviewExtra` test included, and V14.

## V17. Plan alignment

- The call runs on the interactive lane (V2, K11, U71). U202's approach says the U60 to U64 renders run on the query lane; this file supersedes that wording for previews.
- U202 says fill-empty is forced on for a type with no notes. This file reads it as its initial state: the user can turn it off (V5 rule 3), as in Anki.
- U71 lists the first-note fallback in the engine's preview module; V2 puts it in `renderUncommitted`, with the preview note in the result.
- R941's order of the two strings is V9's last paragraph.
