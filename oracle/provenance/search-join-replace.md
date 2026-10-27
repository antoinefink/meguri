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
