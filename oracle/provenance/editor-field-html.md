# Editor field HTML (U190): 2026-11-10 additions

Spec-side provenance for the sections of `specs/editor/field-html.md` changed
on 2026-11-10 (session s-20261110T085429Z-165d83d3). Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied into specs or scenarios. The rest
of the file has no provenance entry yet.

Meguri's own design, with no Anki source: H3 step 4's `&quot;` for a decoded
quote (decided), T3's scenario list and the executor's use of the patch form
(decided).

| Spec section | Anki files read | Read for |
|---|---|---|
| field-html H3 step 4, X1 (quotes in media names) | `qt/aqt/editor_legacy.py` (the default editor's save of a field: the page's serialized text, then null bytes removed and media names unescaped), `pylib/anki/media.py` (the escape and unescape calls), `rslib/src/text.rs` (the encode and decode the calls reach) | that Anki decodes media names after the page serialized the field, so a decoded `"` lands inside a double-quoted value |
| field-html H3 table (first row) | none | the caret's place follows from H1 step 2, Meguri's own wording |
| field-html T3, conformance/record-ops W5 | `rslib/src/notes/mod.rs` (note update: field count check, normalization of every field) | that a patch built from the live note stores what the whole-note update stores, refusals included |
