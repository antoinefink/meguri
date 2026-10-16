# Oracle driver: search and canonical_search (U80)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to choose the `anki` Python APIs
behind the `search` and `canonical_search` observations
(`driver/observe/search.py`); meanings in `specs/conformance/search-ops.md`.
No code was copied. The specs' own sources are in `search.md`.

| Anki file | Read for |
|---|---|
| `pylib/anki/collection.py` (`find_cards`, `find_notes`, `build_search_string`, `group_searches`) | card and note searches with no order; one text given to the builder becomes one parsable node, not a group |
| `rslib/src/search/service/mod.rs` | the builder only parses and writes, it reads nothing from the collection |
| `rslib/src/error/search.rs`, `ftl/core/search.ftl` | each search error's message and its arguments; backticks become apostrophes in every argument except the prop operator's property (which cannot hold one); `is:` and `prop:` share one message told apart by its term; the generic message |
| `rslib/src/search/parser.rs` (`check_id_list`, `parse_dupe`, `parse_prop`) | the two reasons written in English without a translation, and that the id-list text names the key as typed; custom-data property names are ASCII letters and digits |
| `rslib/src/error/mod.rs`, `rslib/src/error/db.rs`, `rslib/src/backend/error.rs`, `pylib/anki/_backend.py`, `pylib/anki/errors.py` | search errors reach Python as `SearchError` with the localized message; an invalid regex is an invalid-input error whose message is the SQLite reason in `<pre>`; every other statement failure is a `DBError` |

Probed in the oracle image (pinned by `driver/tests/test_search.py`): every
placeable is wrapped in U+2068 ... U+2069, the reason inside the outer
`Invalid search:` message included, so the marks delimit the reported texts;
the regex message prefix; the database errors of a negated operator word and
of non-ASCII id digits.
