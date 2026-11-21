# Built searches, joining and replacing (U88)

Spec-writer notes for `specs/search/join-and-replace.md`. Read at tag 26.09.3
(`refs/anki-26.09.3`); no code was copied. Not yet checked against the oracle:
the driver has no build, join or replace observation (the spec's J11 lists the
scenarios to record), so the J6 bracket rule and the J7 kinds rest on reading
alone until those goldens exist.

| Spec section | Anki files read | Read for |
|---|---|---|
| join-and-replace J2 | `qt/aqt/browser/browser.py` | the search field's white-space replacement before its text is used (Python `\s`, which adds U+001C–U+001F to the parser's set) |
| join-and-replace J3, J3.1 | `rslib/src/search/service/search_node.rs`, `rslib/src/search/builder.rs`, `rslib/src/text.rs`, `proto/anki/search.proto`, `qt/aqt/browser/sidebar/tree.py` | each structured item's term: wildcard escaping of names with the `_*` exception, the any/none flag items, day and due items, id lists, duplicates, template by ordinal, a group of one item, parsed text as one item or a group; the items the sidebar builds |
| join-and-replace J4 | `rslib/src/search/service/mod.rs`, `rslib/src/search/builder.rs`, `rslib/src/search/writer.rs` | building writes the item as a whole search, one top-level group unwrapped |
| join-and-replace J5 | `rslib/src/search/builder.rs` | negating a negation unwraps it |
| join-and-replace J6 | `rslib/src/search/service/mod.rs`, `rslib/src/search/builder.rs`, `qt/aqt/browser/sidebar/tree.py`, `pylib/anki/collection.py` | the existing text read as one item (a group when it has several), joined without grouping either side again, so a group is written in brackets; modifier keys of the sidebar |
| join-and-replace J7 | `rslib/src/search/writer.rs`, `rslib/src/search/service/mod.rs`, `rslib/src/search/parser.rs` | replacement only for a single term, by term variant, through negations and groups; the variants that share one kind (`rated:`/`resched:`, every `prop:`, every field term, both template forms) |
| join-and-replace J10 | `rslib/src/search/writer.rs`, `rslib/src/search/parser.rs`, `oracle/AGENTS.md` (white space in built searches) | where Anki's text for a built item reads back as another search |

## Session s-20261117T152613Z-96466c16 (spec writer): deferred harvest rows, batch 2

Read at tag 26.09.3 (`refs/anki-26.09.3`) to classify deferred harvest rows, write their scenarios and draft spec text; no code, test text or fixture value was copied unless a row says otherwise, and every expected value comes from the recorded oracle goldens.

| Spec section and scenarios | Anki file | Read for |
|---|---|---|
| search/builder/negation-twice and search/builder/grouped-precedence (join-and-replace J3, J4, J5, J6) | `rslib/src/search/builder.rs` (tests negating, joining) | which behaviors to prove: double negation restores the item, state items included; composed groups keep inner brackets, and a flat text means something else. Items, values and expectations are Meguri's own and come from the oracle; the rework did not reread the Anki file. |
| search/join-and-replace J7/J8 standalone same-kind replacement (covered by search/join-replace/replace) | `rslib/src/search/service/mod.rs`, `rslib/src/search/service/search_node.rs` | Which behavior the harvest test proves (a single non-group existing node is replaced by a node of the same kind), and how parsable text of one term turns into a single node rather than a group, which confirms that the existing scenario's one-term cases reach that path. |
| Harvest triage of the structured search-item conversion behind join-and-replace J3/J4 and conformance/search-ops S5 (whole collection, note ids, flag colors 1-7, added/edited day items, due-within operator, literal-text escaping, empty-group refusal) | `rslib/src/search/service/search_node.rs` | Which built-item behaviors the harvested tests require (no-filter item as whole collection, a single id written in full, colored flags as flag numbers, day counts kept, due-within as an inclusive <= property, literal text escaping backslash/star/underscore, an empty group refused), checked against the existing search/builder/items golden. No spec text was written from it. |
| join-and-replace J7 (replacing terms of one kind through groups and negations); harvest row h75a546f3b68f mapped to existing scenario search/join-replace/replace | `rslib/src/search/writer.rs` | which behavior the harvested test exercises: replacement by term kind through nested groups and negations, unrelated kinds and negations kept, a negated replacement item and an item of another kind leaving the text unchanged. The existing J7 row of this provenance file already covers this file; at most a harvest-link note is needed. |

## Session s-20261121T011230Z-86ff77a3 (spec writer): U88a review gaps

Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. The other decisions of this session (negative ids, refusal reasons, error order, the whole collection in J8) rest on Meguri's own specs (`search/grammar` G8, `search/query-model` Q2, Q5, Q6, `storage/ids-usn-mtime` I1, I2) and on the U88a builder's reason names, not on Anki sources.

| Spec section | Anki file | Read for |
|---|---|---|
| join-and-replace J3.2 (a space in a regex-form tag written `\x{20}`) | `rslib/src/storage/sqlite.rs`, `rslib/src/search/sqlwriter.rs` | the `tag:re:` expression is compiled by the `regex` crate after a `(?i)` prefix and tested against the pieces of the tags column split at a space, so a `\x{..}` escape reaches that engine unchanged and no piece holds a space |
