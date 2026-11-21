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

On macOS, identify each actual native WebKit replay link through its public
`AXDOMIdentifier`, read with `AXUIElementCopyAttributeValue` and
`kAXDOMIdentifierAttribute`. Require a successful string value equal to the
trusted transform-emitted ID and exactly one matching link in the owned page.
This is a native AX observation, not a JavaScript DOM lookup. Retain raw
`AXIdentifier` and raw `AXDOMIdentifier` separately; the former may be empty.
Never alias, fill or normalize either field from the other. Names, ordinals,
frames and hit points cannot substitute for identity. An unsupported, missing,
wrong-type, duplicate or stale native web identity fails the check.

Capture and revalidate the actual page/card/generation/side binding under
`render/card-behaviors` B1/B3. Generated ownership still requires D4's trusted
metadata; marker classes and ID prefixes do not prove it. Native activation
uses `AXUIElementPerformAction` with `kAXPressAction` on that same actual AX link,
with existing document/frame/origin and bridge checks intact. Add no proxy leaf,
JavaScript activation or action retry. This macOS observation rule establishes
no iOS/iPadOS mapping and changes none of their native identity obligations.

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

Meguri's wording (D10) in Anki's layout:

- **empty front** (standard note type, K4 step 2): `<div>` + `blank.front` + `<br><a href='https://docs.ankiweb.net/templates/errors.html#front-of-card-is-blank'>More information</a></div>`
- **missing cloze** (cloze note type): `<div>` + `blank.cloze` + `<br><a href='https://docs.ankiweb.net/templates/errors.html#no-cloze-filter-on-cloze-note-type'>More information</a></div>`, with `{N}` in `blank.cloze` being the card's ordinal + 1 (D10).

Each notice is one line of text: no U+000A or other line break before the `<br>`. The text is inserted as written, with no HTML escaping (it holds no `&`, `<`, `>`, `"` or `'`) and no character reference: U+2019 is written as the raw character in UTF-8, never as `&rsquo;` or `&#x2019;`. The link opens like any card link (R568). The wording differs from Anki's by decision (D11).

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
| `notice.more` | `More information` |

Meguri's wording (R959; for the blank notices, D11):

| Id | Text |
|---|---|
| `blank.front` | `This card’s front shows no field content.` |
| `blank.cloze` | `This card’s note has no cloze {N}.` |
| `error.header.question` | `The front template has an error:` |
| `error.header.answer` | `The back template has an error:` |

The apostrophe in `blank.front` and `blank.cloze` is U+2019 RIGHT SINGLE QUOTATION MARK, never U+0027 (the typographic-apostrophe rule of `review/reviewer-strings` S1). `{N}` is the card's ordinal + 1 written in ASCII decimal digits, with no padding, grouping or locale digits (ordinals go up to 499, so `{N}` is 1 to 500).

`blank.front` says what makes a standard card blank (`template-syntax.md` T10): no field with content appears on its front. The front may still show template text or special fields such as the deck name, which D9 puts above the notice; the sentence stays true then. `blank.cloze` is true both for a cloze number that was removed and for card 1 of a cloze note with no cloze at all. Image Occlusion notes are cloze notes, so a removed mask gives `This card’s note has no cloze {N}.` too; that is intended, as Anki shows its missing-cloze notice there.

Links (exact): `https://docs.ankiweb.net/templates/errors.html#front-of-card-is-blank`, `…#no-cloze-filter-on-cloze-note-type`, `…#template-syntax-error` (same page, three anchors).

## D11. Differences from the Product Contract's wording

- Anki turns more text into buttons than D3: a marker with any single character other than U+000A in place of `q`/`a`, or with non-ASCII decimal digits (any Unicode decimal digit), becomes a button, or is removed when buttons are hidden. Most of these buttons work in Anki's review and preview screens. A side other than exactly `q` selects the answer list, on either side of the card, so `[anki:play:x:0]` plays answer entry 0. A side of `\` also plays from the answer list, because the button's script reads it as an empty side. Non-ASCII digits count by their numeric value, so `[anki:play:q:١]` plays question entry 1. Some buttons fail when clicked: an index past the end of the list raises an error, a side of `:` makes the command unreadable, and a side of `'`, `"` or U+000D breaks the button's markup or script, so the click does nothing. The pipeline never writes such markers; they appear only when note text contains them literally. Meguri leaves all of them as text, in both `hidePlayButtons` states, which keeps the markup free of quotes and backticks it does not control. This loses Anki behavior in two ways. A question field with the literal text `[anki:play:x:0]`, on a card whose answer has `[sound:a.mp3]`, shows a button in Anki that plays a.mp3 from the question, while Meguri shows the text. With buttons hidden, Anki removes that text and Meguri still shows it.
- R526 leaves the iOS action open (a post to Meguri's native message handler), and the render coverage note proposed a play message to AnkiMobile's `cb` handler. D4 uses a handler of Meguri's own, `meguri`, instead: `cb` keeps exactly AnkiMobile's protocol, under which every message other than its taps is ignored (R511), and a plain command string needs no JSON quoted inside the `onclick` attribute. A handler name is a member of `window.webkit.messageHandlers`, not a page-world global, so R516 holds.
- R531 asks for Anki's wording for template errors too, while R959 makes it Meguri's; this file follows R959 for template errors (as `template-syntax.md` T9 does).
- **The two blank notices use Meguri's wording** (D10 `blank.front`, `blank.cloze`), not Anki's, though R531 asks for Anki's. Decided 2026-11-21 by the coordinator under Antoine's delegation: these are user-facing sentences, and compatibility does not depend on them. The notice is display output only: it is never stored in the collection, never synced, and never part of the comparable render outputs that the oracle and Meguri both produce (`conformance/render-ops` Q5 reports the blank kind, `empty_front` or `missing_cloze`, and a blank card's question without any notice). The layout, the `More information` text and the three links stay as D7 and D10 give them. The notice names no feature (Anki's names its Empty Cards tool), so it stays correct whether or not a Meguri screen offers Empty Cards (`notes/empty-cards`; `settings.maintenance`).
  - The same notice reaches AnkiConnect clients in `cardsInfo` (`servers/ankiconnect-actions` A11.3); a client does not depend on its sentence. A18's `ankiconnect-cardsinfo-blank-notice` covers it.
  - Product Contract requirements R531, R399 and R976 still ask for Anki's wording. This decision replaces that for the two blank notices only, as R959's deviation does for template errors. Amending the plan, and recording the decision among the standing decisions, is left to the coordinator.
  - Register entry `render-blank-notice-wording` (R1889, `conformance/dsl` D13; the `conformance/deviations.yaml` header links this register). No committed golden or recording holds a D7 blank notice (render-ops Q5 never shows one, and no `cardsInfo` transcript has a blank card), so the entry authorizes no transform and no comparison changes. Its proof is D12's display snapshot test `display_outcomes_on_every_host_and_button_setting` (`core/crates/meguri-template/tests/display_snapshots.rs`), cases `blank_front` and `missing_cloze`, together with `blank_notices_use_meguri_wording` (D12).

## D12. Tests

Snapshot tests (insta, `core/crates/meguri-template/tests/snapshots/`) pin the D9 output for both platforms and both `hidePlayButtons` values on at least: a question with two sounds and its answer with FrontSide plus one sound (the FrontSide buttons carry `play:q:0` and `play:q:1`, the answer's own button `play:a:0`); a TTS directive followed by a sound (TTS content shown, the sound's command `play:q:1`); hidden buttons with the AV lists unchanged; a literal `[anki:play:x:1]` and `[anki:play:q:١]` left as text; an empty-front card, a missing-cloze card and a template error on each side. Tests assert that the markup the transform adds never contains a backtick or `${`. These snapshots are Meguri-produced and never serve as goldens.

- `blank_notices_use_meguri_wording`: the empty-front notice is exactly D7's markup with `This card’s front shows no field content.`, and the missing-cloze notice is D7's markup with `This card’s note has no cloze 1.` for a card rendered at ordinal 0 and `This card’s note has no cloze 10.` for a card rendered at ordinal 9 (its `CardSpecials` ordinal and the ordinal passed to `display::compose` both 9), on both platforms and both `hidePlayButtons` values, on the question and the answer side (D9: the question's notice is appended after T, the answer's goes through T). Each apostrophe is U+2019 and the text holds no line break. The test compares whole strings built from D7 and D10, building the docs host the way `display_snapshots.rs` does (no external hostname written whole), and contains no other wording: no Anki sentence, not even in a negative assertion.


Focused generated-control checks:

- `generated_replay_controls_have_names_and_unique_deterministic_ids`: verify nonempty default labels, decorative SVG and per-document IDs on both platforms, including repeated identical markers, FrontSide repeated twice, leading-zero commands and authored-ID collision. Rerendering preserves IDs and exact play-command order.
- `generated_replay_accessibility_preserves_display_contract`: hidden controls and TTS create no replay element; comparable HTML, user-authored HTML/IDs/styles, AV lists, graphic and native messages remain unchanged. Preserve the existing no-backtick/no-`${` checks for generated display markup.
- `native_generated_replay_controls_use_catalog_localization`: the final native page exposes catalog-localized names before readiness; quote/ampersand/non-ASCII copy stays plain attribute text and creates no extra HTML or script. Preserve authored marker-lookalike anchors with matching replay classes, `data-meguri-replay` and reserved-looking IDs; only positively identified transform-generated controls receive localized labels.
- `native_generated_replay_controls_are_named_and_identified_in_webkit_hierarchy`: with multiple sounds and repeated FrontSide, verify distinct stable identifiers, meaningful localized labels and activation of the exact intended command in the actual native WebKit accessibility hierarchy. DOM `id` or `aria-label` assertions alone do not establish R1964 compliance. On macOS use D4's exact native web-identity rule and the same AX link's native press; preserve twenty exact command activations over question/answer/question/answer, including repeated commands, leading zeroes and authored-ID collision/name preservation. Keep the existing bounded publication readiness, localized names, keyboard ownership and all geometry/ruby/hit-target checks.

The macOS native hierarchy export and structural driver must expose and consume
that same raw web-identity field, alongside the actual raw AXIdentifier/XCTest
identifier. Report the observed XCTest value verbatim and read the native tree
from AX, never from the DOM. The checker must enforce exact unique trusted identity through
this named native-web rule; no missing-ID exemption or label/ordinal/geometry
fallback is permitted. This corrects only the assumption that a WebKit DOM ID
must populate AXIdentifier. Preserve all other checker rules and findings,
other controls and mobile requirements. Capture the actual native hierarchy
and screenshots before and after each action; classify unrelated findings
separately. Retained failures 7686 and 93192 remain failures: neither establishes
AXDOMIdentifier availability or successful native activation on its destination.
