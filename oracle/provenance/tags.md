# Tags, flags and marked notes (U30)

Spec-writer notes for `specs/tags/tags.md`, `specs/tags/flags-and-marked.md`
and `specs/conformance/tag-ops.md`. Read at tag 26.09.3 (`refs/anki-26.09.3`);
no code was copied. Black-box probes ran in the oracle image
(`meguri-oracle:26.9.3-34c3a09eb2d55813`, `--network none`) through the `anki`
Python API: tag lists on note adds, every tag operation on small collections,
the tag tree, card flags and undo of a rename. Their outputs agree with the
goldens `tags/canon/*` and with every expectation the scenarios state.

| Spec section | Anki files read | Read for |
|---|---|---|
| tags T1, T2 | `rslib/src/tags/mod.rs`, `rslib/src/tags/register.rs`, `rslib/src/text.rs` (NFC); `Cargo.lock` (unicode-normalization 0.1.24) | separators, per-component normalization order (NFC, removal, trim, `blank`), refusal of an empty result |
| tags T3, T4 | `rslib/src/tags/register.rs`, `rslib/src/notes/mod.rs` (tags canonicalized on add and update), `rslib/src/storage/tag/mod.rs`, `rslib/src/storage/tag/*.sql` | existing-row spelling through the `unicase` key, nearest registered ancestor, dedupe and sort by the `unicase` order, new-row columns |
| tags T5 | `rslib/src/tags/matcher.rs`; `Cargo.lock` (regex 1.11.2, regex-syntax 0.8.6, Unicode 16.0 simple case folding) | escaped names, case-insensitive alternation, `::` or end after the name, first alternative wins |
| tags T6 | `rslib/src/tags/matcher.rs`, `rslib/src/tags/rename.rs`, `rslib/src/tags/reparent.rs`, `rslib/src/tags/remove.rs`, `rslib/src/notes/mod.rs` (`NoteTags` stamping) | in-place replacement with no sort or dedupe, every matched note written |
| tags T7 | `rslib/src/tags/rename.rs`, `rslib/src/tags/service.rs`, `qt/aqt/browser/sidebar/tree.py`, `qt/aqt/operations/tag.py` | refusals, early exit with no matching note, prefix spelled before the old rows go (the deviations), sidebar rename input |
| tags T8 | `rslib/src/tags/reparent.rs`, `rslib/src/tags/service.rs`, `qt/aqt/browser/sidebar/tree.py` | skip rules (exact text prefix with the same first component), first source wins, the failure when a note uses a skipped source next to a kept one (probe: a panic that poisons the backend) |
| tags T9, T10 | `rslib/src/tags/remove.rs`, `rslib/src/tags/bulkadd.rs`, `pylib/anki/tags.py`, `qt/aqt/browser/browser.py` | registry removal without matching notes, add re-sorts only changed notes, removal leaves rows |
| tags T11 | `rslib/src/tags/remove.rs`, `rslib/src/storage/note/mod.rs` (tags in notes with ancestors) | used names, full folding |
| tags T12 | `rslib/src/tags/tree.rs`, `rslib/src/storage/tag/update.sql`, `rslib/src/collection/transact.rs`, `rslib/src/undo/mod.rs` | registration before the update, usn stamping, no undo step (skip-undo), the update looked up with the raw name |
| tags T13 | `rslib/src/tags/tree.rs`, `qt/aqt/browser/sidebar/tree.py` | implicit parents, their spelling and collapsed state, `::` read as U+001F for sorting |
| tags T14, T15 | `rslib/src/tags/undo.rs`, `rslib/src/notes/undo.rs`, `rslib/src/ops.rs`, `ftl/core/actions.ftl`, `ftl/core/browsing.ftl` | row re-insert on undo without `config`, operation names |
| flags F1, F2 | `rslib/src/card/mod.rs`, `rslib/src/card/service.rs`, `pylib/anki/collection.py` | low three bits, other bits kept, `usn` -1 with `mod` kept, refusal above 7, only changed cards written |
| flags F3 | `qt/aqt/reviewer.py`, `qt/aqt/browser/browser.py` | toggle against the current card |
| flags F4 | `qt/aqt/flags.py`, `qt/aqt/browser/sidebar/tree.py`, `ftl/core/actions.ftl` | `flagLabels` layout, empty or equal names, restore, `"` removed by the sidebar, default English names |
| marked M1–M3 | `pylib/anki/tags.py`, `pylib/anki/notes.py`, `qt/aqt/reviewer.py`, `qt/aqt/browser/browser.py` | `marked` constant, case-insensitive test, mark and unmark through bulk add and remove |

Driver hints for the oracle maintainer (U30 role work; meanings in
`specs/conformance/tag-ops.md`):

- `add_note_tags`: `col.tags.bulk_add(note_ids, text)`, output its `count`; the
  listed note ids as stored (deleted notes may stay in the list).
- `remove_note_tags`: `col.tags.bulk_remove(note_ids, text)`.
- `delete_tags`: `col.tags.remove(space_separated_tags=text)`.
- `rename_tag`: `col.tags.rename(old=from, new=to)`; the backend's invalid-input
  kind (read the kind from the backend error, as `ops/model.py` does) is the
  rejection `invalid_name`. The refusal happens before any write.
- `reparent_tags`: `col.tags.reparent(tags=list, new_parent=parent)`. A panic in
  the backend poisons its mutex for the rest of the worker's life; scenarios
  avoid the one known case (tag-ops.md G5).
- `clear_unused_tags`: `col.tags.clear_unused_tags()`.
- `set_tag_collapsed`: `col.tags.set_collapsed(tag=name, collapsed=value)` once
  per name in order; no output.
- `set_flag`: `col.set_user_flag_for_cards(flag, card_ids)`; invalid input is
  the rejection `invalid_flag` (refused before the transaction). A negative
  flag is `invalid_scenario` (the schema's minimum).
- `tag_tree`: `col.tags.tree()`; for each child of the root, recursively,
  `{name, collapsed, children}` (drop `level`).
- All counts come from the returned `OpChangesWithCount.count`.

Harvest classification (same tag): the 38 `tags` rows of
`conformance/catalog/harvest.csv` cover `pylib/tests/test_tags.py`,
`pylib/tests/test_flags.py` and `rslib/src/tags/{bulkadd,complete,findreplace,matcher,register,remove,reparent,service,tree}.rs`.
Tag completion and tag find-and-replace rows are deferred to the units that
own them (U88's search-text services, U86's Find and Replace); the legacy
Python helpers (`canonify`, `rem_from_str`, the legacy register and bulk
wrappers) have no Meguri counterpart.
