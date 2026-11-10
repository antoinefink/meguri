# Template and note preview spec (U71)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/render/preview.md`
and the scenarios under `conformance/scenarios/render/preview/`. No code was
copied. Every rule was restated in the spec writer's words. The oracle
driver's `template` observation (`oracle/driver/observe/template.py`) runs the
backend's uncommitted render; its `fill_empty` case key
(`specs/conformance/render-ops.md` Q1.4 step 0) passes the case's value as the
render's fill flag, so V6 and V7 are observed by `render/preview/fill-empty`
and `render/preview/fill-empty-cards`.

| Spec section | Anki file | Read for |
|---|---|---|
| preview V1, V3 | `rslib/src/notetype/render.rs`, `rslib/src/card_rendering/service.rs`, `proto/anki/card_rendering.proto`, `pylib/anki/template.py`, `pylib/anki/notes.py`, `qt/aqt/clayout.py` | uncommitted render inputs (note, template object, card ordinal, fill flag, partial render); the template is used as given; the first template for a cloze kind; the CSS comes from the in-memory note type, not the stored one; kind and field list come from the stored note type |
| preview V2 | none at the tag | Meguri's own design (plan U71; the optional note draft, the engine's first-note choice, the preview note, the refusal table and the lane); the `NotFound` refusal mirrors `card-render-pipeline.md` K2 |
| preview V4 | `rslib/src/notetype/render.rs`, `pylib/anki/notes.py` | the stored card is looked up by note id and the template's ordinal, which the layout screen always sets to the card ordinal; otherwise a default card (deck 0, flags 0, id 0); a note id with no card at that ordinal gives the default card; the special-field values are `special-fields.md` P3's; the note-type mismatch rule is Meguri's |
| preview V5 | `qt/aqt/editor.py`, `qt/aqt/editor_legacy.py`, `qt/aqt/models.py`, `qt/aqt/browser/sidebar/tree.py`, `qt/aqt/clayout.py` | the editor opens the layout screen on its note with fill off; note type management and the sidebar open it on a new empty note with fill on (Anki's choice, against R938's first-note rule); the fill option is a toggle |
| preview V6 | `rslib/src/notetype/render.rs`, `rslib/src/template.rs`, `qt/aqt/clayout.py` | fill only when the question template has no structural error; cloze references are replacements whose filter list has an exact `cloze` piece, sections recursed, comments excluded; each empty field (the emptiness regex of `text/field-emptiness` E1) becomes the sample or the parenthesized raw name; the kind is not consulted; the card picker lists the cloze numbers of the unfilled note plus the current card's number |
| preview V7 | `ftl/core/card-templates.ftl` | English text of the sample cloze; English-only in the engine is Meguri's choice |
| preview V8 | `qt/aqt/clayout.py`, `qt/aqt/main.py`, `pylib/anki/template.py` | the preview side is the card's styled side, then media escaping, then play buttons (or their removal when hidden), then the type-in simulation, all on the side with its style element; errors give the same text on both sides |
| preview V9 | `qt/aqt/clayout.py`, `pylib/anki/collection.py` | fixed `example`/`sample` inputs and their argument order (expected, then provided), combining characters counted; already in `type-answer.md` A11.1 |
| preview V12 | `qt/aqt/clayout.py`, `qt/aqt/theme.py`, `qt/aqt/reviewer.py`, `ts/reviewer/index.ts` | both sides shown through the answer entry point with a body class string; body classes for a card ordinal and night mode; night mode toggled on the root element; no `reviewExtra` in the preview page; media playback gesture setting; the platform-class switch, the media rule and the IO line are Meguri's design (R940, U68) |
| preview V15 | `qt/aqt/clayout.py`, `qt/aqt/editor.py`, `qt/aqt/editor_legacy.py`, `qt/aqt/browser/sidebar/tree.py`, `pylib/anki/notes.py`, `pylib/anki/cards.py`, `rslib/src/notetype/render.rs` | the mobile option adds one root class; autoplay from the Default deck's options, once per target or side change; the fill option offered only when a value is empty after Unicode trimming; the sidebar's card type item opens a new note with fill on; the editors save or reload a stored note before the layout screen; the note type name comes from the stored note type |
| preview V10 | `rslib/src/notetype/render.rs` | the filled values live only in the render's copy of the note |
| preview V11 | none at the tag | Meguri's own design: `card-render-pipeline.md` K11's `RenderedCard` in preview mode, plus V2's preview note |
| preview V13, V14 | none at the tag | Meguri's own design (plan U71, U190, U195, U202; R1835's share and the timing fixture) |
| preview V16, V17 | none at the tag | scenario index and plan alignment (plan U71, U202) |
| render-ops Q1.2-Q1.4 (`fill_empty`, step 0) | `rslib/src/notetype/render.rs`, `rslib/src/card_rendering/service.rs`, `proto/anki/card_rendering.proto` | the uncommitted render's fill flag fills the note copy it is sent before the field map is built, from the question template only, only when that template has no structural error, and never writes the stored note |
