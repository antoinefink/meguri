# Anonymized clone and the schedule check (U15)

Read at tag 26.09.3 (`refs/anki-26.09.3`) for `specs/fixtures/anonymizer.md`
and `specs/conformance/collection-check.md` K11. No code was copied; the specs
restate behavior in Meguri's words.

| Anki file | Read for |
|---|---|
| `rslib/src/cloze.rs` | the cloze opener (`{{c`, digits and commas, `::`, a number list that must parse), closer, nesting depth, hint split, image-occlusion clozes recognized by their first text; cloze numbers read from the raw field (A7) |
| `rslib/src/image_occlusion/imageocclusion.rs` | occlusion shape text: shape name, `:name=value` properties, `\:` and `\\` escapes, the property names including `text` (A7) |
| `rslib/src/image_occlusion/imagedata.rs`, `notetype.rs`, `proto/anki/notetypes.proto` | image-occlusion note types by `original_stock_kind` 6, field roles by field tag with an index fallback, the image extensions Anki accepts (by extension only) (A10.2) |
| `ts/routes/image-occlusion/review.ts` | the review page sizes its canvas from the image's natural width and height (A10.2) |
| `rslib/src/scheduler/queue/builder/gathering.rs`, `sorting.rs`, `mod.rs`, `rslib/src/storage/deck/mod.rs`, `active_deck_ids_sorted.sql`, `update_active.sql`, `rslib/src/storage/card/mod.rs`, `active_new_cards.sql` | new cards gathered deck by deck ordered by deck name; the active deck list filled without an order clause and used by the deck review sort; random orders hashing card or note ids with the day, review random hashing id and mod; tie order from the scan (A11.1, A14.2, A15) |
| `proto/anki/deck_config.proto`, `rslib/src/deckconfig/mod.rs` | default gather order by deck, default review order by day (A11.1, A14.2) |
| `proto/anki/decks.proto` | `KindContainer` normal and filtered, `description`, `search_terms`, `search` field numbers (A11.3) |
| `rslib/src/decks/name.rs` | deck name components: control characters removed, so the separator sorts below them (A11.1) |
| `rslib/src/tags/register.rs`, `rslib/src/tags/mod.rs` | tag separators (space and U+3000), canonical tag strings sorted and deduplicated under unicase, registration adopting existing case (A12) |
| `rslib/src/dbcheck.rs` | which note and tag rewrites Check Database makes without counting a problem, so the clone may differ only where the source would be rewritten (A12.2, A17 V3) |
| `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/text.rs` | search term reading (quotes, partial quotes, escapes, `and`/`or`), the keys and their kinds, deck and tag matching (glob to regex, case-insensitive, prefix ending at a separator, `current`, `filtered`, `none`, `*`), `did:` ids (A13) |
| `qt/aqt/browser/sidebar/tree.py`, `qt/aqt/flags.py`, `qt/aqt/profiles.py`, `qt/aqt/browser/browser.py` | collection config keys holding saved searches and flag labels; search history kept in the profile, not the collection (A13.5) |
| `rslib/src/scheduler/queue/mod.rs`, `learning.rs`, `main.rs`, `entry.rs` | a queue's presentation order (learning due now, main, learn-ahead), its counts, queue entry kinds, the queue cache cleared on deck change (K11.1) |
| `rslib/src/scheduler/answering/mod.rs`, `rslib/src/scheduler/states/load_balancer.rs`, `rslib/src/scheduler/queue/builder/mod.rs` | next states use the load balancer only while a queue is built; the load balancer covers all cards due in its window, grouped by preset, whichever deck is studied (K11.1) |
| `rslib/src/scheduler/service/mod.rs`, `rslib/src/decks/tree.rs`, `rslib/src/scheduler/bury_and_suspend.rs` | computing the day and the deck tree unbury cards on a new day; the empty default deck hidden from the tree (K11) |
