# Search names, ids, states, properties and history (U82)

Read at tag 26.09.3 (`refs/anki-26.09.3`) to write `specs/search/names-and-ids.md`,
`state-and-flags.md`, `properties.md` and `history.md`. No code was copied;
every rule was restated in the spec writer's words and checked as a black box
against the oracle image's Anki (three probe scenarios for the day clock in UTC,
Europe/Paris and America/New_York, then the kept scenarios under
`conformance/scenarios/search/{tags,decks,notetypes,ids,dupe,state,props,history}/`).

| Spec section | Anki file | Read for |
|---|---|---|
| names-and-ids N1, N2 | `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/text.rs`, `rslib/src/storage/sqlite.rs` | tag value readings per mode, the special values and their order, the tag expression and its non-space wildcard, the per-tag test of `tag:re:` (split at spaces), the combining-free transform before the wildcard conversion in `tag:nc:` |
| names-and-ids N3.2, N3.3 | `rslib/src/search/sqlwriter.rs`, `rslib/src/decks/name.rs`, `rslib/src/text.rs` | `*`, `filtered`, `current`; glob conversion before the level normalization of the deck name; the anchored expression on the stored name; `did`/`odid` |
| names-and-ids N3.4 | `rslib/src/search/sqlwriter.rs`, `rslib/src/decks/current.rs`, `rslib/src/config/mod.rs` | current deck id default, the name fallback when the deck is missing (a name, not an id), escaping of the name |
| names-and-ids N3.5, N5 | `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs` | id lists written into the statement, `did:` with home decks and without children, `mid:` |
| names-and-ids N3.6 | `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/storage/deckconfig/mod.rs`, `ts/routes/deck-options/lib.ts` | raw preset value, first row by name under the column collation, decks by preset id with home decks; the deck options page escaping names it builds into `preset:` searches |
| names-and-ids N4 | `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/text.rs` (column collations from `specs/storage/schema-18.md`) | template number reading, name equality through the `unicase` column collation versus the case-insensitive pattern for globs, the `(ntid, ord)` template match |
| names-and-ids N6 | `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/storage/note/mod.rs` (checksum and strip rules from `specs/text/`) | dupe text reading, conditional NFC, strip keeping media names, checksum candidates then the stripped first-field comparison |
| state-and-flags S5, S6, and S3's learn-ahead source (S1–S4 otherwise: `search-state-and-flags.md`) | `rslib/src/search/sqlwriter.rs`, `rslib/src/search/parser.rs`, `rslib/src/config/mod.rs`, `rslib/src/scheduler/mod.rs` | type and queue conditions, the learn-ahead source and default, the flag mask |
| state-and-flags S7 (unbury) | `rslib/src/scheduler/mod.rs`, `rslib/src/scheduler/service/mod.rs`, `rslib/src/scheduler/bury_and_suspend.rs` | the day query a search uses does not unbury; the timing service does |
| properties P2–P5 | `rslib/src/search/sqlwriter.rs`, `rslib/src/search/parser.rs` | column comparisons, the ease product and integer cast, the due rule per queue with the original due, integer division, 32-bit day addition; position by type |
| properties P6–P9 | `rslib/src/search/sqlwriter.rs` | operands formatted into the statement (Rust `Display` of `f32`), the difficulty mapping, the new-card case of retrievability, custom-data number cast and text comparison |
| properties P12, P15 (custom data); card-extractors X3, X5–X7 (the former P10, P11, P11.1, P13) | `rslib/src/storage/sqlite.rs`, `rslib/src/storage/card/data.rs`, `rslib/src/serde.rs`, `fsrs` 6.6.2 `src/inference.rs` (crate, not Anki) | the card data reader (per-key leniency, whole-text failure on a non-string `cd`), the custom-data value text, retrievability elapsed time (u32 casts, saturating subtraction), relative retrievability and its fallback, original position |
| properties P4.1 | `rslib/src/search/mod.rs` | the browser's Due sort uses another rule |
| history H1–H8 | `rslib/src/search/parser.rs`, `rslib/src/search/sqlwriter.rs`, `rslib/src/scheduler/mod.rs` (day values from `specs/sched/day-clock.md`, U40) | days raised to 1 except `resched:`, the cutoffs from next-day-at, strict versus inclusive comparisons per term, the answer-button filters, the first-answer test of `introduced:`, the six operator windows of `prop:rated` |

Added 2026-11-10, read at the same tag; the scenarios named here were first recorded on 2026-11-10 and their goldens agree with these readings:

| Spec section | Anki file | Read for |
|---|---|---|
| names-and-ids N3.6 (deleted preset row; `search/decks/preset-missing`) | `rslib/src/search/sqlwriter.rs`, `rslib/src/storage/deckconfig/mod.rs` | the preset found by name in storage, then decks compared by stored preset id with no fallback |
| names-and-ids N6 step 2 (`search/dupe/entities`) | `rslib/src/search/sqlwriter.rs`, `rslib/src/notes/mod.rs` | one strip of the dupe text, the checksum of that result without a second strip |
| history H1 (`search/history/odd-ease`) | `rslib/src/search/sqlwriter.rs` | `ease > 0` for answers, `ease != 0` for the first-answer test, `ease = 0` for manual entries; no `type` condition |
| properties P4 (`search/props/due-extremes`) | `rslib/src/search/sqlwriter.rs` | the learning-due difference and division written into the statement, evaluated by SQLite (real on 64-bit overflow) |
