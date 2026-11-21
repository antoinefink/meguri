# U88a builder spec gaps: review log, 2026-11-21

Branch `unit/builder-spec-gaps`, spec-writer session `s-20261121T011230Z-86ff77a3`. The lines marked "Changed 2026-11-21 (U88a review)" in `search/join-and-replace`, `search/filter-tokens` and `browser/entry-points` settle five gaps that the U88a implementer (PR #23) found. They were decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs.

## Decisions

1. Negative note type ids: refused (`negative_id`) for `mid:` and for the note type id of `dupe:`, as for id lists. One rule for every id an item writes.
2. Refusal reasons: the 13 reasons are listed in J3.6, with the order inside one item. U88a's first code named the due-day reason `day_out_of_range`, which differed from `days_out_of_range` by one letter; it becomes `due_day_out_of_range`.
3. Error order (J1): one walk in item order. The first parse error or refusal met is the result. A refusal carries the refused item's path.
4. A space in a regex-form tag is `\x{20}` (J3.2). This only states what the rule already said.
5. The whole collection (J8): Meguri keeps Anki's `deck:*`. J8 states its meaning in each mode, and the property tests exclude these cases. Browse never builds this item: it runs the empty text (filter-tokens T4 step 4, T7 step 1; entry-points E5 step 3 for Browse).

## Independent review

Reviewer: an independent Claude subagent (Opus). It was given the diff and the implementer's questions, and judged the decisions itself. It read the diff, the neighbouring specs, `builder.rs`, the builder tests and, to check facts, Anki at tag 26.09.3.

- P1: none.
- P2: the cited review log was missing (this file). A query with no token and a text of White_Space only would have built an empty group in T4 step 4: fixed in T4 step 4 and T16.
- P3, fixed: the negative-id reason contradicted J3.5 (reworded); the dupe row did not fit J10's intro (intro widened, U+2212 replaced); the empty-group bullet now defers to J1's error order; entry-points E5 step 3's `deck:*` bullet now says what Browse does; T12.4's saved-search counts read the stored text through T7; J8 says meaning is kept in cards mode always and in notes mode when every note has a card; history removed from J3.6; the consumers of the refusal path are named; the Status lines quote the inline marker form; J3.2's citation includes N2.3 and T12.1; "blank" is defined; J8 says the empty query's rows equal the status line's total.
- Re-review: no P1 or P2. Two P3 points, both fixed: J1 named an AnkiConnect error as a consumer of the refusal path, which no server spec supports (now tests and log lines only); the Status lines quoted only some of the marker's forms (they now take the marker in any letter case or punctuation).
- P3, not taken: keep the canonical check and the cards-mode id check for whole-collection cases in the property test. This is optional and recorded for the implementer.

Confirmed by the reviewer: J3.6 matches `builder.rs` except the rename and the two new negative-id checks; the order inside one item and the error order match the code's walk; `\x{20}` passes through the grammar unchanged and means U+0020 in the `regex` crate; no counterexample to J8's statement under AND, OR, NOT and Q5's scopes; nothing in the specs builds the whole-collection item for Browse.

## Code and test changes required (follow-up unit)

- `core/crates/meguri-search/src/builder.rs`: refuse a negative `NotetypeId` item and a negative duplicates note type id (`negative_id`, checked before the text); rename `day_out_of_range` to `due_day_out_of_range`; add the refused item's path to the `invalid_item` error; update the `InvalidItem` doc comment.
- `core/crates/meguri-search/tests/builder.rs`: expect `negative_id` for `NotetypeId(-5)` instead of `mid:-5`; add a negative duplicates id, the rename and refusal paths.
- `core/crates/meguri-search/tests/filter_tokens.rs` (not yet written): T16's new row.
