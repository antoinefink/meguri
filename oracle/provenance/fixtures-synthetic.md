# Synthetic collections and the collection check (U11)

Read at tag 26.09.3 (`refs/anki-26.09.3`) for `specs/fixtures/synthetic-collections.md`
(F5, F6, F7) and `specs/conformance/collection-check.md` (K5, K6, K8). No code
was copied; the specs restate behavior in Meguri's words.

| Anki file | Read for |
|---|---|
| `rslib/src/dbcheck.rs` | the order of Check Database's steps, its twelve problem counters, which repairs mark the schema changed, the silent repairs (note rewrite with card generation, tag table rebuild keeping expanded tags, next position, image-occlusion field tags, stock note type when none) (K5) |
| `rslib/src/storage/card/mod.rs`, `fix_due_new.sql`, `fix_due_other.sql`, `fix_odue.sql`, `fix_ivl.sql`, `fix_mod.sql`, `fix_ordinal.sql` | the card property repairs and their conditions: new-card `due` from 2,000,000, review `due` above 100,000, `odue`, `ivl`, `mod`, `ord` bounds; the last-review-time comparison; `max(due)+1` for the next position (K5, F7) |
| `rslib/src/scheduler/fsrs/memory_state.rs` (last revlog info), `rslib/src/revlog/mod.rs` | which rows set the expected last review time (rated, not cramming) and which reset it (manual with factor 0) (K5) |
| `rslib/src/storage/revlog/fix_props.sql`, `rslib/src/storage/revlog/mod.rs` | review-log repairs (K5) |
| `rslib/src/storage/dbcheck/mod.rs`, `invalid_ids_count.sql` | the future-id cutoff (now plus one day) and the tables it covers (K5) |
| `rslib/src/storage/deck/mod.rs` (`missing-decks.sql`), `rslib/src/decks/tree.rs` | missing deck ids and missing parent names (K5, F6.4) |
| `ftl/core/database-check.ftl`, `rslib/src/collection/service.rs`, `pylib/anki/collection.py` | the problem messages and which carry a number; how Check Database results reach Python (K5) |
| `rslib/src/tags/register.rs`, `rslib/src/tags/mod.rs`, `rslib/src/storage/tag/mod.rs`, `add.sql` | tags are registered without parents, new tags collapsed, note tag order by unicase (F6.6, F6.7) |
| `rslib/src/notes/mod.rs` | the note fields Check Database recomputes (sort field, checksum, NFC) (F7) |
| `rslib/src/notetype/mod.rs`, `rslib/src/template.rs` | card requirements computed on every note type save; identical fronts and unknown fields refused on save (F6.5, F9.2) |
| `rslib/src/notetype/fields.rs`, `rslib/src/notetype/templates.rs`, `rslib/src/notetype/stock.rs` | defaults of new fields and templates (font, size, random ids, template stamps 0); stock kinds (F6.5) |
| `rslib/src/notetype/cardgen.rs`, `rslib/src/config/mod.rs` | one new position per added note, shared by its cards; next position semantics (F6.6) |
| `rslib/src/config/schema11.rs`, `rslib/src/storage/sqlite.rs`, `rslib/src/storage/config/mod.rs`, `rslib/src/config/bool.rs` | the config keys and values of a new collection; bool defaults (F6.2) |
| `pylib/anki/collection.py`, `qt/aqt/deckbrowser.py`, `qt/aqt/reviewer.py`, `rslib/src/preferences.rs` | without `sched2021` the desktop client asks for a scheduler update; the Python layer writes `creationOffset` on open when missing (F6.2) |
| `rslib/src/scheduler/timing.rs`, `rslib/src/scheduler/mod.rs` | day counting with and without `creationOffset`; `rollover` written when missing; `localOffset` written by a client (F4.3, F6.2, K3) |
| `rslib/src/scheduler/states/steps.rs`, `learning.rs`, `relearning.rs`, `review.rs`, `new.rs`, `interval_kind.rs` | step delays (60, 330, 600, 900 s for the default steps), remaining steps, conversion of a delay that reaches the rollover to one day, ease deltas and floor (F5.1, F5.4) |
| `rslib/src/scheduler/answering/learning.rs`, `relearning.rs`, `review.rs`, `revlog.rs`, `mod.rs`, `rslib/src/card/mod.rs` | card columns written per state, the review-log interval and last interval, the FSRS difficulty mapping for the review-log factor, the answer time cap, deck studied-today counters (F5.5, F5.6, F4.4) |
| `rslib/src/decks/mod.rs`, `rslib/src/decks/counts.rs`, `rslib/src/decks/stats.rs`, `rslib/src/decks/limits.rs` | new decks collapsed; studied-today counters and when they count (F6.4, F4.4) |
| `rslib/src/scheduler/bury_and_suspend.rs` | unburying on a new day and `lastUnburied` (F5.5, F6.2) |
| `rslib/src/storage/deckconfig/mod.rs`, `rslib/src/deckconfig/mod.rs` | default preset; answer time cap 60 s (F6.3, F5.6) |
| `rslib/src/card_rendering/service.rs` | rendering an existing card; empty-front and missing-cloze results; template errors (K8) |
