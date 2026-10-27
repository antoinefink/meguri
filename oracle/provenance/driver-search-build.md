# Oracle driver: search_build, search_join and search_replace (U88)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind the built-search observations (`driver/observe/search_build.py`);
meanings in `specs/conformance/search-ops.md` S5, S6 and
`specs/search/join-and-replace.md`. No code was copied.

| Anki file | Read for |
|---|---|
| `proto/anki/search.proto` (`SearchNode`) | the structured items each neutral item maps to; which J3 items have none (note type id, card and deck id lists, the two buried states) |
| `pylib/anki/collection.py` (`build_search_string`, `group_searches`, `join_searches`, `replace_in_search_node`) | the backend calls and their arguments; the joiner enum |
| `rslib/src/search/service/mod.rs`, `rslib/src/search/service/search_node.rs` | join and replace convert the existing node first, then the item; a group of one is its item; a negated negation is the item; the empty group is an invalid-input error; nothing reads the collection |
| `rslib/src/search/builder.rs`, `rslib/src/search/writer.rs` | the flat join keeps the existing node whole (a group stays bracketed); replace unwraps a top-level group and replaces terms of the same variant at any depth, only for a plain-term item |
| `rslib/src/text.rs` (`escape_anki_wildcards_for_search_node`) | deck, tag, note type and field-present names keep `_*` unescaped (J10) |
| `rslib/src/notetype/mod.rs`, `rslib/src/notetype/fields.rs`, `rslib/src/decks/name.rs`, `rslib/src/tags/register.rs` | which names `search/builder/names` can create: note type names drop `"`, field names drop `:` |

Probed through `tools/mg oracle test test_search_build`: every example of
join-and-replace.md J9 that J10 does not set apart gives J9's text.
