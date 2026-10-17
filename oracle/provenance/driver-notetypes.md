# Oracle driver: note type ops (U29)

Read at tag 26.09.3 to choose the `anki` Python APIs behind `add_notetype`,
`add_stock_notetype`, `clone_notetype`, `update_notetype`,
`ensure_io_notetype`, the `notetype_structure` observation and `add_note` by
stored name (`driver/ops/notetypes.py`, `driver/ops/notes.py`; meanings in
`specs/conformance/notetype-ops.md`), and the `render_status` observation
(`driver/observe/render_status.py`; `specs/conformance/render-ops.md` Q4); no
code was copied. The specs' own sources are in `notetypes.md`.

| Anki file | Read for |
|---|---|
| `pylib/anki/models.py` | `new`, `new_field` and `new_template` build from the stock Basic kind (`ord` null on new fields and templates); `add`, `add_dict` and `update_dict` rename a colliding name with a checksum before the backend sees it, so the driver calls the backend directly; `copy` without adding clears `id` and `originalId` |
| `pylib/anki/_backend.py` | the generated calls go through `_run_command`, which the driver's probe replaces to read the backend error's kind and help page |
| `rslib/src/notetype/service.rs` | the legacy add always runs the template checks; the legacy update takes `skip_checks`; the legacy and protobuf getters read the file, not the note type cache |
| `rslib/src/notetype/mod.rs` | the save's order (structure, names, sort field, requirements, template checks); `+` uniqueness from the file; an add makes the new note type current; the update reads its original from the file, while `get_notetype` and `get_all_notetypes` read through the cache (hence the reopen before each op) |
| `rslib/src/notetype/fields.rs`, `rslib/src/notetype/templates.rs` | field and template name fixes refuse with invalid input |
| `rslib/src/notetype/schema11.rs` | legacy keys for field options and template values; `ord` is how the save tells kept fields and templates from new ones; `originalId` omitted when absent |
| `rslib/src/image_occlusion/notetype.rs` | the on-demand add keeps the current note type; the first valid Image Occlusion note type is the lowest id with stock kind 6 and at least 4 fields |
| `rslib/src/error/mod.rs`, `rslib/src/backend/error.rs`, `proto/anki/backend.proto`, `proto/anki/links.proto` | card type errors carry a help page per kind; parse errors and unknown fields share one, and their message is header, `<br>`, then the see-preview or field-not-found text |
| `ftl/core/card-templates.ftl` | which translations hold the two details (read through the backend's translations, never as literal text) |
| `proto/anki/notetypes.proto` | the structure's protobuf fields; `original_id` and field `tag` are optional (presence) |
| `pylib/anki/template.py` (`TemplateRenderContext.from_existing_card`, `_partially_render`), `rslib/src/notetype/render.rs` (`render_existing_card`), `rslib/src/template.rs` (`render_card`) | `render_status`: the review render of a stored card (browser off), partial for Python, as `collection_check.render_card` does for K8; the backend raises a question-side template error before its blank check and an answer-side one after it; a standard card's ordinal with no template is invalid input; the note type comes through `get_notetype`'s cache, hence the reopen first |

Probed in the oracle image (`driver/tests/test_notetypes.py`,
`test_render_status.py` and one-off runs): every scenario under
`conformance/scenarios/notetypes/` agrees with what it states; a field rename
makes Anki append the first field to browser question formats that reference
no field (U34's behavior, not the driver's).
