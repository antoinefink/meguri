# Display transform of a rendered card

Spec: render/display-transform
Requirements: R469, R470, R526, R527, R528, R531, R559, R564, R976, R1907
Anki: 26.09.3 (behavioral interfaces only; replay graphic is Meguri's original design)
Scenarios: none (Meguri snapshot tests; see D12)

What turns the outputs of the card render pipeline (`card-render-pipeline.md` K4) into the HTML that a card page shows in review and preview: replay buttons, TTS text, hidden play buttons, the blank-card and template-error notices, and the style block. The oracle does not produce this display HTML. Behavior follows R469–R470, R526–R528, R531, R559 and R564. The replay graphic is the original Meguri design specified in D4 and the revised R526; the earlier literal did not establish independent authorship and has been superseded. Meguri's own tests verify the display transform, with the original graphic taking precedence under R1960 and R1907's replay-markup normalization. Browser mode has no display transform (K5).

## D1. Inputs and output

For one card and one **host** `{platform: mac | ios, hidePlayButtons: bool}` (`ios` covers iPhone and iPad):

- the review outputs of K4: the question and answer texts with play markers, both AV lists (`av-tags.md` A6), the blank kind or the template error, the note type's CSS;
- output: the question display HTML and the answer display HTML (D9).

`hidePlayButtons` is the collection config value of `card-render-pipeline.md` K9, read when the card is rendered.

## D2. Order of the steps on one side

1. Media-name encoding of `src` attributes (U64, `text/media-references#M6`).
2. Play markers (D3–D6).
3. The type-in answer box and comparison placeholder (U64).
4. The style block (D9).

Until U64 lands, steps 1 and 3 do nothing.

## D3. Markers

A **marker** is the text `[anki:play:` + side + `:` + digits + `]`, where side is `q` or `a` and digits is one or more ASCII digits. Its **index** is the digits' value; its **command** is `play:` + side + `:` + digits, exactly as written (`[anki:play:q:00]` has index 0 and command `play:q:00`).

- The side is read in one left-to-right pass; text that replaced a marker is not read again.
- Markers come from the pipeline (`av-tags.md` A5), from FrontSide on the answer (A8), and from note text that happens to contain marker text; all are treated alike.

## D4. Replay button

Every marker that D5 does not claim becomes this original Meguri replay control, with ACTION and ID in place. The generated control has a meaningful accessible name and stable identifier (R1964); it is not exempt card-authored content:

```html
<a class="replay-button soundLink" id="ID" data-meguri-replay="true" aria-label="Play audio" href=# onclick="ACTION; return false;" draggable="false"><svg class="playImage" aria-hidden="true" focusable="false" viewBox="0 0 48 48" version="1.1"><rect x="3" y="3" width="42" height="42" rx="12" fill="none" stroke="currentColor" stroke-width="2" /><path d="M18 14L34 24L18 34Z" fill="currentColor" /></svg></a>
```

The transform adds no leading or trailing line break and no indentation. The rounded-square frame and filled triangle were specified for Meguri from simple geometry on 2026-10-19, without an external image, code or asset as an input. This graphic replaces the earlier graphic literal; its presence in the old Product Contract did not prove independent authorship. R1960 and R1907 require the original graphic while preserving the outer anchor attributes and classes, native actions and AV behavior. Base styling preserves the unfilled frame and inherits the card foreground for its `currentColor` stroke and triangle fill, including dark mode and preview (R483).


ID uses `review.card.replay.` followed by the displayed document side (`q` or `a`) and a zero-based occurrence ordinal, for example `review.card.replay.a0`. It identifies an emitted control, not its AV command. Allocate in document order, counting generated replay controls only; repeated identical markers and every FrontSide occurrence receive distinct IDs. The question and answer each have their own display document. Identical display inputs produce identical IDs, independent of locale. If an authored ID already occupies a candidate, choose the next unused ordinal for the generated control; do not change any authored ID or HTML.

`Play audio` is Meguri's own default accessible copy for the standalone display transform. In the native card page, use the existing CardRendering `CardBehavior.xcstrings` catalog and `String(localized:table:bundle:)` pipeline to supply its localized form through the narrow `CardRuntime.pageDocument` configuration. The runtime applies that label only to controls positively identified as transform-generated through transform/host metadata or an equally safe existing ownership mechanism. The `data-meguri-replay` marker is a locator, not ownership proof. Authored lookalikes remain untouched even when they use matching classes, markers or ID prefixes. Apply the final generated-control labels before card readiness, interaction or accessibility exposure. Use text/attribute assignment, not HTML interpolation. Do not add a locale subsystem to `RenderHost`, rewrite user-authored controls or create a new page-world global. The SVG is decorative; its geometry contributes no separate accessible name or focus target.

These attributes extend only Meguri-generated replay markup. Keep the existing classes, graphic, card CSS and native play action. Preserve AV lists, exact command text (including repeated commands and leading zeroes), marker order, TTS replacement and hidden-button behavior. Comparable render HTML and user-authored styling remain unchanged. This is required control naming/identification, not the optional VoiceOver cursor/speech follow-up. R1965 hierarchy checks remain in force; this change grants no ruby, geometry, hit-target, missing-ID or card-content audit exception.

ACTION:

| Platform | ACTION |
|---|---|
| `mac` | `pycmd('COMMAND')` (R506) |
| `ios` | `window.webkit.messageHandlers.meguri.postMessage('COMMAND')` |

COMMAND is the marker's command (D3). On iOS and iPadOS the page has no `pycmd` (R505, R513): the button posts the command string to the page-world script message handler named `meguri`, which U67 registers and maps to "play that side's entry" (R551). This adds no page-world global (R516). The markup holds no backtick and no `${` on either platform (R526), so a sound tag inside a template literal of a card script stays a valid string.

## D5. TTS markers

A marker whose side's AV list (the question's list for `q`, the answer's for `a`) has a TTS entry at its index is replaced by that entry's content (`av-tags.md` A6), with no button and no sound (R564). The index is kept, so the following sounds keep Anki's numbering (R559). On the answer side, the question's TTS markers inside FrontSide show the question's TTS content.

The content is inserted as it was written inside the directive, markup included. When U64's media-name encoding (D2 step 1) exists, the content is inserted in encoded form too.

## D6. Hidden play buttons (R527)

With `hidePlayButtons` on, every marker that D5 does not claim is removed (replaced by nothing). TTS content still shows. The AV lists are unchanged, so autoplay and replay work as before.

## D7. Blank-card notices (R531, R976)

Shown with Anki's English wording (D10), as Anki shows them:

- **empty front** (standard note type, K4 step 2): `<div>` + `The front of this card is blank.` + `<br><a href='https://docs.ankiweb.net/templates/errors.html#front-of-card-is-blank'>More information</a></div>`
- **missing cloze** (cloze note type): `<div>` + `No cloze N found on card.` + U+000A + `Please either add a cloze deletion, or use the Empty Cards tool.` + `<br><a href='https://docs.ankiweb.net/templates/errors.html#no-cloze-filter-on-cloze-note-type'>More information</a></div>`, N being the card's ordinal + 1 in decimal.

The link opens like any card link (R568).

## D8. Template-error notice (R531, R959)

`HEADER` + `<br>` + `MESSAGE` + `<br><a href='https://docs.ankiweb.net/templates/errors.html#template-syntax-error'>More information</a>`, where:

- `HEADER` names the side that failed (D10): the question side's or the answer side's header;
- `MESSAGE` is the template engine's message for the error (Meguri's wording, `template-syntax.md` T9; it repeats the offending tag), with `&`, `<`, `>`, `"` and `'` written as `&amp;`, `&lt;`, `&gt;`, `&quot;` and `&#x27;`.

The wording is Meguri's (R959); the layout is Anki's. Tests compare the error kind and side only (R1887).

## D9. Composition of the two sides

`WRAP(css, html)` is `<style>` + css + `</style>` + html (R470): the note type's CSS as stored, unchanged, before the side, never moved to the page head. `T(text)` is D2 steps 1–3 applied to a side.

| Card outcome (K4) | Question display HTML | Answer display HTML |
|---|---|---|
| rendered | `WRAP(css, T(question))` | `WRAP(css, T(answer))` |
| blank | `WRAP(css, T(question) + notice)` | `WRAP(css, notice)` |
| template error | `WRAP("", error notice)` | `WRAP("", error notice)` |

- The answer's FrontSide part is the question text without its style block (R469): FrontSide receives the question before D9 wraps it.
- A blank card's answer is only the notice; a template error shows the same notice, with the failing side's header, on both sides, and uses no note type CSS, as in Anki. The card can still be answered (R531; U43, U175).

## D10. Strings

Anki's wording, used exactly (R531):

| Id | Text |
|---|---|
| `blank.front` | `The front of this card is blank.` |
| `blank.cloze` | `No cloze {N} found on card.` U+000A `Please either add a cloze deletion, or use the Empty Cards tool.` |
| `notice.more` | `More information` |

Meguri's wording (R959):

| Id | Text |
|---|---|
| `error.header.question` | `The front template has an error:` |
| `error.header.answer` | `The back template has an error:` |

Links (exact): `https://docs.ankiweb.net/templates/errors.html#front-of-card-is-blank`, `…#no-cloze-filter-on-cloze-note-type`, `…#template-syntax-error` (same page, three anchors).

## D11. Differences from the Product Contract's wording

- Anki turns more text into buttons than D3: a marker with any single character other than U+000A in place of `q`/`a`, or with non-ASCII decimal digits, becomes a button that plays nothing, or is removed when buttons are hidden. The pipeline never writes such markers; they appear only when note text contains them literally. Meguri leaves them as text, which keeps the markup free of quotes and backticks it does not control.
- R526 leaves the iOS action open (a post to Meguri's native message handler), and the render coverage note proposed a play message to AnkiMobile's `cb` handler. D4 uses a handler of Meguri's own, `meguri`, instead: `cb` keeps exactly AnkiMobile's protocol, under which every message other than its taps is ignored (R511), and a plain command string needs no JSON quoted inside the `onclick` attribute. A handler name is a member of `window.webkit.messageHandlers`, not a page-world global, so R516 holds.
- R531 asks for Anki's wording for template errors too, while R959 makes it Meguri's; this file follows R959 for template errors (as `template-syntax.md` T9 does) and R531 for the two blank notices.

## D12. Tests

Snapshot tests (insta, `core/crates/meguri-template/tests/snapshots/`) pin the D9 output for both platforms and both `hidePlayButtons` values on at least: a question with two sounds and its answer with FrontSide plus one sound (the FrontSide buttons carry `play:q:0` and `play:q:1`, the answer's own button `play:a:0`); a TTS directive followed by a sound (TTS content shown, the sound's command `play:q:1`); hidden buttons with the AV lists unchanged; a literal `[anki:play:x:1]` and `[anki:play:q:١]` left as text; an empty-front card, a missing-cloze card and a template error on each side. Tests assert that the markup the transform adds never contains a backtick or `${`. These snapshots are Meguri-produced and never serve as goldens.


Focused generated-control checks:

- `generated_replay_controls_have_names_and_unique_deterministic_ids`: verify nonempty default labels, decorative SVG and per-document IDs on both platforms, including repeated identical markers, FrontSide repeated twice, leading-zero commands and authored-ID collision. Rerendering preserves IDs and exact play-command order.
- `generated_replay_accessibility_preserves_display_contract`: hidden controls and TTS create no replay element; comparable HTML, user-authored HTML/IDs/styles, AV lists, graphic and native messages remain unchanged. Preserve the existing no-backtick/no-`${` checks for generated display markup.
- `native_generated_replay_controls_use_catalog_localization`: the final native page exposes catalog-localized names before readiness; quote/ampersand/non-ASCII copy stays plain attribute text and creates no extra HTML or script. Preserve authored marker-lookalike anchors with matching replay classes, `data-meguri-replay` and reserved-looking IDs; only positively identified transform-generated controls receive localized labels.
- `native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy`: with multiple sounds and repeated FrontSide, verify distinct stable identifiers, meaningful localized labels and activation of the exact intended command in the actual native WebKit accessibility hierarchy. DOM `id` or `aria-label` assertions alone do not establish R1964 compliance. Keep the hierarchy checker and its ruby/geometric findings unchanged; classify unrelated findings separately.
