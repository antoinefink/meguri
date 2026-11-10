# Tag editor (U193)

Spec-writer notes for `specs/editor/tag-editor.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Written 2026-11-10 (session
s-20261110T085429Z-165d83d3) by mapping each section of the committed text to
the files that back it. Suggestions' query and its preparation are
`specs/tags/complete.md` (`tags-complete.md`); save normalization is
`specs/tags/tags.md` (`tags.md`).

Meguri's own design, with no Anki source: Space ending a tag (R885),
case-insensitive duplicates (R887), paste at the caret and its immediate
duplicate drop, Delete and cut on selected chips, the touch context menu,
condensed chips tested on the full layout, untouched chips kept byte for byte
(G12), the marked-text rule and U+3000 as Space (G4), Space accepting a
highlighted suggestion, full-folding equality (G1), the selection keys table
(G8), the canonical list after a commit (G11), the flush that leaves the
input on screen (G11) and the device-local collapse memory per screen kind.

| Spec section | Anki files read | Read for |
|---|---|---|
| tag-editor G1, G2 | `ts/lib/tag-editor/tags.ts` (separator glyph, display and sent forms), `ts/lib/tag-editor/TagEditor.svelte` (rows test, condensed chips latched on), `ts/lib/tag-editor/TagWithTooltip.svelte` (shortened label, tooltip with the full name) | U+2237 as the glyph, every glyph written back as `::`, the `…∷leaf` label, more than two rows |
| tag-editor G3, G9 | `ts/lib/tag-editor/TagEditor.svelte` (append an empty tag unless the last is empty, selection cleared), `ts/lib/tag-editor/TagSpacer.svelte`, `ts/lib/tag-editor/tag-options-button/TagAddButton.svelte`, `ts/lib/tag-editor/TagEditMode.svelte` (click edits) | Add tag, the space after the chips, the shortcut that appends and focuses |
| tag-editor G4 | `ts/lib/tag-editor/TagInput.svelte` (Return splits at the selection, Space and colon handling, joins at the edges, arrow moves, empty-input arrows, select-all only with other tags, copy and cut writing `::`), `ts/lib/tag-editor/TagEditor.svelte` (join, split, move handlers) | each key's effect and Anki's Space rule listed in G13 |
| tag-editor G5 | `ts/lib/tag-editor/tags.ts` (trimming outer colons and glyphs with white space), `ts/lib/tag-editor/TagInput.svelte` (blur), `ts/lib/tag-editor/TagEditor.svelte` (exact-name uniqueness and flash) | the trim order, empty removal, Anki's case-sensitive duplicate rule |
| tag-editor G6 | `ts/lib/tag-editor/WithAutocomplete.svelte` (highlight cycle through none, choose), `ts/lib/tag-editor/TagEditor.svelte` (Up/Down always, Tab only while shown, Return chooses; a chosen suggestion ends the tag) | the cycle order, the typed text restored at none, acceptance ending the tag |
| tag-editor G7 | `ts/lib/tag-editor/TagInput.svelte` (paste: plain text, appended to the whole text, split on white space, trimmed, last piece kept in the input) | the split and the piece order; Anki's append-at-end listed in G13 |
| tag-editor G8 | `ts/lib/tag-editor/TagWithTooltip.svelte` (Ctrl toggles, Shift ranges), `ts/lib/tag-editor/TagEditor.svelte` (anchor, range, copy joined by LF, remove selected, deselect on focus out), `ts/lib/tag-editor/TagEditMode.svelte` and `TagDeleteBadge.svelte` (× ignored in select mode), `ts/lib/tag-editor/tag-options-button/TagsSelectedButton.svelte` | selection semantics, the copied text, which keys act on a selection |
| tag-editor G10 | `qt/aqt/profiles.py` (collapsed flag per editor kind, default false), `qt/aqt/editor_legacy.py` (collapsed state sent per editor mode) | per-kind memory and the expanded default |
| tag-editor G11 | `qt/aqt/editor_legacy.py` (tags received from the page on save, saved with the note outside add mode), `ts/lib/tag-editor/TagEditor.svelte` (list sent on accept and delete) | when the list is handed on |
| tag-editor G13 | the files above, plus `ftl/core/editing.ftl` (tag menu labels) | every difference listed |
| tag-editor G13 (review round, 2026-11-10) | `ts/lib/tag-editor/WithAutocomplete.svelte` (the acceptance dispatched with the highlighted entry, or with none, whenever the list holds suggestions; the highlighted index kept across list updates), `ts/lib/tag-editor/TagEditor.svelte` (Return handled by both the input's split and the forwarded acceptance; each pasted piece but the last inserted only when unique, with the flash), `ts/lib/tag-editor/TagInput.svelte` (forward Delete checks the second character after the caret), `ts/lib/tslib/keys.ts` (Control mapped to Meta on Apple platforms, for shortcuts and for Ctrl-click) | the Return, paste, highlight, forward Delete and keys bullets |
