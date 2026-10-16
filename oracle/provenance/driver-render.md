# Oracle driver: the `template` observation (U60)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind `driver/observe/template.py`; meanings in
`specs/conformance/render-ops.md` Q1-Q2, the spec writer's sources and hints
in `render.md`. No code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/template.py` (`TemplateRenderContext._partially_render`, `PartiallyRenderedCard`, `apply_custom_filters`) | the card layout screen's call (uncommitted legacy render, `partial_render` on); how pylib completes the nodes: text as is, a replacement's current text after the (here empty) filter hooks, FrontSide injected on the answer side only |
| `pylib/anki/hooks.py`; no `template_legacy.py` at the tag | the field filter hooks and legacy `fmod_` filters are empty unless an add-on registers some |
| `rslib/src/card_rendering/service.rs` (`render_uncommitted_card_legacy`), `rslib/src/notetype/schema11.rs` (`CardTemplateSchema11`) | the request: note, card ord, template JSON with optional `ord`; which template keys default |
| `rslib/src/notetype/render.rs` (`render_uncommitted_card`, `existing_or_synthesized_card`, `add_special_fields`) | the stored card is used only when the template has an `ord` and the note a card at it; otherwise a default card (deck 0, flag 0, id 0) |
| `rslib/src/template.rs` (`render_into`, `render_card`, `template_error_to_anki_error`, `localized_template_error`) | FrontSide always comes back as an empty replacement node in partial mode; the blank notice is pushed as its own last question node and the answer is not rendered; the error message layout (side header, `<br>`, escaped details, `<br>`, help link) and which message and arguments each error kind uses |
| `ftl/core/card-template-rendering.ftl` | the messages' arguments; no-closing-brackets is the one message outside Q2 |
| `rslib/src/notetype/mod.rs` (`prepare_for_update`, `normalize_names`, `ensure_names_unique`, `ensure_notetype_name_unique`, `ensure_valid_parsed_card_templates`), `rslib/src/notetype/fields.rs` (`fix_name`) | what a note type save changes (name `"` removed, NFC, field names stripped and made unicase-unique by appending `+`, a taken name suffixed), so the driver compares the saved names with the request's; the dummy template a save accepts |
| `pylib/anki/models.py` (`new`, `add_dict`, `ensure_name_unique`, `remove`, `get`), `pylib/anki/cards.py` (`note`, `note_type`, `template`), `pylib/anki/notes.py` (`_to_backend_note`) | building, saving and removing the scratch note type; the cached note type dict (the driver reads the backend's instead); the note sent as given |

Probed in the oracle image (`driver/tests/test_template.py`): the details are
escaped exactly as `html.escape` escapes (the driver unescapes them instead of
re-escaping the translations), arguments are wrapped in isolation marks, and a
no-closing-brackets message also fits the not-closed message's pattern, so the
most specific pattern wins.
