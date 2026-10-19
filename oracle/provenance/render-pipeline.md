# Render pipeline, AV tags, display transform and the `render` observation (U62)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write
`specs/render/card-render-pipeline.md`, `av-tags.md`, `display-transform.md`
and `specs/conformance/render-ops.md` Q5-Q6. No code was copied; every rule was
re-derived in the spec writer's words and checked by black-box probes of the
oracle image (backend `extract_av_tags` on literal texts, and an emulation of
Q5 over every U62 scenario, composed as below), which agree with the specs.

| Spec section | Anki file | Read for |
|---|---|---|
| av-tags A2-A4 | `rslib/src/card_rendering/parser.rs` | sound node (`is_not("]")`, so a non-empty name), anki tag node (name chars, white space set, option key/value grammar with the quoted alternative, content up to the first matching closing tag, case-sensitive), text node advancing one character then to the next `[` |
| av-tags A5-A7 | `rslib/src/card_rendering/writer.rs`, `rslib/src/card_rendering/mod.rs` | play marker format and per-side counter; sound name through `decode_entities`; TTS lang is the last `lang` option; empty lang writes the bracketed `errors-bad-directive` message and no tag; other directives re-serialized from a HashMap (option order not stable), quoting rule |
| av-tags A6 (speech fields) | `rslib/src/card_rendering/writer.rs` (`transform_tts_content`), `rslib/src/text.rs` (`strip_html_for_tts`, `decode_entities`) | field text and the localized `[...]` blank word, which the observation therefore never reports |
| av-tags A6 (playback name) | `pylib/anki/template.py` (`av_tag_to_native`), `pylib/anki/sound.py` (`SoundOrVideoTag.path`) | only the base name is played, from the media folder |
| pipeline K2-K5 | `rslib/src/notetype/render.rs`, `rslib/src/template.rs` (`render_card`), `rslib/src/notetype/templates.rs` (browser formats), `rslib/src/notetype/mod.rs` (`get_template`) | template choice (first for cloze), special fields, empty-front check skipped in browser mode while the missing-cloze check is not, blank notice pushed as the last question node and as the whole answer, browser format fallback per side on the empty string |
| pipeline K4 | `pylib/anki/template.py` (`TemplateRenderContext.render`, `apply_custom_filters`, `TemplateRenderOutput`) | FrontSide on the answer is the question after AV extraction; on a TemplateError both texts are the message, no AV tags, CSS empty; `card_did_render` hooks |
| pipeline K5 | `rslib/src/browser_table.rs` (`RenderContext`, `rendered_nodes_to_str`) | browser columns use the partial render with FrontSide's current text (empty), no AV extraction, and strip the question prefix from the answer (U84) |
| pipeline K8 | `pylib/anki/latex.py`, `pylib/anki/collection.py` (`setup_hook`), `rslib/src/latex.rs` | the LaTeX hook runs inside `card.question()` in pylib, which is why the observation composes the pipeline itself |
| pipeline K9, K10 | `qt/aqt/main.py` (`_add_play_buttons`, `prepare_card_text_for_display`), `qt/aqt/reviewer.py` (`revHtml`, `_update_flag_icon`, `_update_mark_icon`), `rslib/src/config/bool.rs`, `rslib/src/config/mod.rs`, `pylib/anki/cards.py` (`user_flag`), `pylib/anki/tags.py` (`in_list`) | `hideAudioPlayButtons` default and wrong-type handling, `reviewExtra` placement, flag low three bits, marked tag compared lower-cased |
| display-transform D2-D6 | `qt/aqt/main.py`, `qt/aqt/sound.py` (`av_refs_to_play_icons`), `pylib/anki/sound.py` (`AV_REF_RE`, `strip_av_refs`), `qt/aqt/reviewer.py` (`_mungeQA`, `_showQuestion`, `_showAnswer`), `qt/aqt/browser/previewer.py` | step order (media escaping, play icons or strip, type-in), the exact icon markup with its line breaks, the marker regex (any side character, Unicode digits), preview equal to review |
| display-transform D7, D8, D10 | `ftl/core/card-template-rendering.ftl`, `rslib/src/template.rs` (`template_error_to_anki_error`, link constants) | blank and missing-cloze wording, links, error layout |

## Driver hints for the `render` kind (oracle maintainer)

Meanings are `specs/conformance/render-ops.md` Q5. Never call `card.question()`,
`card.answer()` or `render_output()`: they run pylib's `card_did_render` hooks
(LaTeX image generation and its localized errors). Compose instead, per card
of the scenario's collection, freshly read:

- **review**: `col._backend.render_existing_card(card_id=..., browser=False, partial_render=True)`.
  - Compose the question nodes as `observe/template.py` does (text as is; a
    replacement's current text; FrontSide nothing). When `is_empty` is set,
    drop the last question node after checking it is Anki's blank notice
    (empty-front or missing-cloze message, read through the backend's own
    translations; card ordinal + 1 for the cloze number), and report `blank`
    `missing_cloze` when the note type is cloze, `empty_front` otherwise.
  - `col._backend.extract_av_tags(text=<question>, question_side=True)` gives
    the question text and AV tags.
  - Unless blank: compose the answer nodes with FrontSide = the extracted
    question text, then `extract_av_tags(..., question_side=False)`.
  - AV entries: `sound_or_video` -> `{"sound": <value as the backend gives it,
    no basename>}`; `tts` -> `{"tts": <lang>}`.
  - Replace every occurrence of `"[" + tr.errors_bad_directive(directive="anki:tts", error=tr.errors_option_not_set(option="lang")) + "]"`
    in both texts by `[tts: lang not set]` (Q5.4), after both extractions.
- **browser**: the same call with `browser=True`; compose both sides with
  FrontSide nothing; no extraction. `is_empty` can only be the missing-cloze
  notice here (drop it; answer null; blank `missing_cloze`).
- **errors**: a `TemplateError` from either call is classified as for the
  `template` kind, with the browser headers (`card-template-rendering-browser-front-side-problem`
  and `-back-side-problem`) added to the side patterns for browser mode.
- A card whose note was deleted gives `null`. A note type with no template at
  the card's ordinal raises "missing template": let it be a run error.
- Setup: `templates[].browser_front` / `browser_back` (dsl.md D6.4) set the
  template dict's `bqfmt` / `bafmt` before the note type is added; a save does
  not check them.

## Harvest links

`conformance/catalog/harvest.csv` render rows classified with U62: the AV
extraction, parser and writer tests (scenarios linked), the pylib play-marker
and strip tests (display transform, deferred: no oracle observation), and the
unknown-card-id service test (not applicable). The test names behind them are
in `oracle/harvest/links.csv` only.

A completion pass (2026-10-17) linked the three rows deferred to U62 (the
uncommitted cloze card render in `rslib/src/card_rendering/service.rs`, and
the full-render and special-field-map tests in `rslib/src/notetype/render.rs`)
to the pipeline, av-tags, cloze and special-fields scenarios, and classified
the `pylib/tests/test_sound.py` path tests: the media-folder join is in scope
(av-tags A6, U67's player, no oracle observation); the absolute-path,
folder-path and `media_file_filter` hook cases are add-on playback paths that
Meguri does not have (`pylib/anki/template.py` reduces card tags to base names
before `SoundOrVideoTag.path` sees them). `render/pipeline/type-in` checks the
K4 statement that type-in markers pass through as text.

## 2026-10-19 prepared-spec intake

Resumed the prepared U62 history with spec-writer session
`s-20261019T051101Z-82de9be1`, without further Anki source research. Both pinned
oracle lanes replayed the 11 new pipeline/AV/error cases and stored-card
special-fields case successfully:

- Linux 12/12: `build/mg/oracle/20261019T051313Z-74771/result.json`.
- Apple 12/12, no overlays: `build/mg/oracle/20261019T051314Z-74991/result.json`.

The display contract now derives its exact compact replay markup directly from
R526; the older GUI-whitespace description in D4 is superseded, with no GUI
asset used for implementation. Browser missing-cloze metadata remains comparable
to the oracle while Browse adds no notices (R976). Raw LaTeX text remains
visible; existing image references are media, with no generated-image rewrite.
The engine read consistency and cache obligations are Meguri design. These
clarifications change no scenario input or golden.

## Stored tags and the preservation pipeline

The focused `render/pipeline/stored-tags` oracle case confirms the existing
two-separator tag contract through both stored-card `template` cases and
`render`: U+0020 and U+3000 split the stored text; TAB, LF and NBSP remain
inside tags. Rendering retains order and duplicates and leaves note tags,
mtime and usn unchanged. K10's marked flag uses that same list in Meguri's
direct engine test; it is not an additional oracle observation.

`preservation/render/every-card` now also observes the stored-template `render`
pipeline beside its existing `template` cases, covering all 19 adopted cards.
Both raw dump observations and all earlier template outputs remain equal to
the prior committed golden; the raw before and after dumps are identical.

Independent Linux recordings:

- Stored tags: `build/mg/oracle/20261019T054152Z-94983/result.json`.
- Preservation: `build/mg/oracle/20261019T054226Z-96284/result.json`.

Both cases replayed successfully on Linux and Apple, with no Apple overlays:

- Linux: `build/mg/oracle/20261019T054243Z-97191/result.json`.
- Apple recording: `build/mg/oracle/20261019T054244Z-97480/result.json`.
- Apple replay: `build/mg/oracle/20261019T054318Z-98612/result.json`.
- Freshness/schema: `build/mg/goldens-check/20261019T054316Z-98364/result.json`.
