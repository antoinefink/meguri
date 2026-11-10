# Search syntax reference (U214)

Spec-writer notes for `specs/search/syntax-reference.md` (session
s-20261110T085429Z-165d83d3). No Anki manual text was used, and only one Anki
file was read for it (below): the examples were chosen and described from
Meguri's own search specs (`search/grammar`, `search/canonical-form`, `search/text-matching`,
`search/fields`, `search/names-and-ids`, `search/state-and-flags`,
`search/properties`, `search/history`), whose rules were checked against the
26.09.3 oracle by their own sessions. Each example's canonical form was derived
by hand from `search/canonical-form` F2 to F6; the draft scenario
`search/reference/examples` (not recorded: the oracle was busy) is what
confirms Y1 once recorded.

| Spec section | Source | Read for |
|---|---|---|
| Y1 | `search/canonical-form` F4, F5; `search/text-matching` T7.2, T7.3, T9.1 | canonical examples; the collections on which plain text, `nc:`, `sc:`, `re:` and `w:` fail |
| Y2 to Y9 | the sections each row cites | what each example finds |
| Y10 | `search/editing-services` D3.1 | the span kinds every family must cover |
| Y8 `resched:7` | `rslib/src/revlog/mod.rs` at 26.09.3 | an FSRS reschedule writes its review-log entry with button 0, so it counts as an entry without an answer (`search/history` H1) |
| Y1 inserting | none | Meguri design |
