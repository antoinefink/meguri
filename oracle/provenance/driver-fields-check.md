# Oracle driver: the `fields_check` observation (U27)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind `driver/observe/notes.py`; meanings in `specs/conformance/note-ops.md`
N4, the spec writer's sources in `notes.md`. No code was copied.

| Anki file | Read for |
|---|---|
| `pylib/anki/notes.py` (`fields_check`, `_to_backend_note`, `NoteFieldsCheckResult`), `proto/anki/notes.proto` (`NoteFieldsCheckResponse`) | the check takes the unsaved note as given (id 0 for a new note); the six state values |
| `rslib/src/notes/mod.rs` (`note_fields_check`, `is_duplicate`) | the draft's own id is skipped; the check compares stripped text after NFC when the key is on |
| `pylib/anki/collection.py` (`new_note`, `get_note`, `build_search_string`, `group_searches`, `find_notes`), `proto/anki/search.proto` (`SearchNode.Dupe`, `parsable_text`) | the dupe search as a node; a string passed to `build_search_string` is parsed and written back, which the driver uses to check that the search string carries the text |
| `qt/aqt/editor_legacy.py` (`showDupes`) | Show Duplicates searches the Dupe node with the note type id and the raw first field |
| `rslib/src/search/writer.rs` (`write_dupe`, `maybe_quote`, `needs_quotation`), `rslib/src/search/parser.rs` (`parse`, `normalize_whitespace`, `parse_dupe`, `unquoted_term`, `quoted_term_str`), `rslib/src/search/sqlwriter.rs` (`write_dupe`, `norm_note`), `rslib/src/search/service/mod.rs`, `search_node.rs` | quoting and escaping of the text; the parser turns every white space character into U+0020 before it splits terms, so text with other white space (or white space at the end of an unquoted term) is not searched as written; the SQL side applies the same NFC and stripping as the check |
| `rslib/src/storage/upgrades/schema15_upgrade.sql` (`notetypes`) | name lookups in SQL are unicase, so the driver matches stored note type names in Python |

Probed in the oracle image (`driver/tests/test_fields_check.py` and a one-off
run): first fields `a` tab `b`, `a` newline `b` and `a` U+00A0 `b` reach the search as two terms
(`a` and `b`) and find nothing; `a` U+3000 `b` (quoted) becomes `a b` and `ab` tab
becomes `ab`, which find other notes; quotes, backslashes (also a trailing
one), brackets, `and`, `or `, a leading `-` and U+0020 round-trip.
