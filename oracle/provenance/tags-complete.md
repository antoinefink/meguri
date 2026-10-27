# Tag completion (U191, U193, U218)

Spec-writer notes for `specs/tags/complete.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-10-27 by mapping each
section of the committed text to the files that back it. Tag terms and the
registry are `specs/tags/tags.md` (`tags.md`); simple case folding is
`specs/search/case-folding.md`.

Meguri's own design, with no Anki source: the cancel token and cancellation
(C1), `limit` as Anki's limit plus one (C4), at most 500 suggestions in the
editor and C5's colon rule (C6, decided).

| Spec section | Anki files read | Read for |
|---|---|---|
| complete C1 | `rslib/src/tags/complete.rs`, `rslib/src/tags/service.rs`, `proto/anki/tags.proto` (completion request and response), `rslib/src/storage/tag/mod.rs` | the input used as given, the rows read (implicit parents never returned, unused rows returned) |
| complete C2 | `rslib/src/tags/complete.rs`, `rslib/src/tags/mod.rs` (component split) | the input split into filters at each `::`, empty filters included |
| complete C3 | `rslib/src/tags/complete.rs` (one escaped case-insensitive pattern per filter, the first component that fits, prefix or inner), `Cargo.lock` (regex 1.11.2, regex-syntax 0.8.6, Unicode 16.0 simple case folding) | when a filter fits a component, the order in which filters take components, what makes a prefix match |
| complete C4 | `rslib/src/tags/complete.rs`, `rslib/src/storage/tag/mod.rs` and `rslib/src/storage/tag/get.sql` (rows in key order under `unicase`) | the cut at the limit before prefix matches are put first; at most the limit plus one tags |
| complete C5, C6 | `ts/lib/tag-editor/TagEditor.svelte`, `ts/lib/tag-editor/tags.ts` (separator display, colon removal), `qt/aqt/tagedit.py` (Browse's tag prompts and their limit) | the editor's limit of 500, its removal of outer single colons and when it applies, the separator shown as `∷`, the Browse prompts sending the text unchanged |
| complete C7 | `proto/anki/tags.proto`, `qt/aqt/tagedit.py` | the backend request the requested observation kind would call (pylib has no wrapper; Anki's prompts call the backend directly) |
