# Deck description HTML (harvest rows)

Spec-writer notes for `specs/text/description-html.md`; the sources of the original spec are in `text.md`. Read at tag 26.09.3 (`refs/anki-26.09.3`) in session s-20261117T152613Z-96466c16 (spec writer) to classify deferred harvest rows; no code was copied.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| text/description-html D2/D3 (Markdown deck descriptions), classification of rows hebddab91941f and h82c4a097602a | `rslib/src/card_rendering/service.rs` (render_markdown backend call with sanitize, and its two tests), `rslib/src/markdown.rs`, `rslib/src/decks/mod.rs` (rendered_description), `qt/aqt/overview.py` (description display) | Which behavior is required (a heading renders as a heading; a script block and its content are removed while ordinary text stays), and confirming that no collection-library read the oracle drives exposes the rendered description. |
