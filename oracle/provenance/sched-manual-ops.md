# Manual scheduling operations and Set Due Date (U48 spec writer)

Spec-writer notes for `specs/sched/manual-ops.md` M1-M11,
`specs/sched/set-due-date.md` S1-S10 and `specs/conformance/sched-ops.md`
J1-J11. Read at tag 26.09.3 (`refs/anki-26.09.3`); no code was copied. Every
rule was checked with the oracle library in the 26.9.3 oracle image (one-off
scripts and a scratch, uncommitted ops module that ran all twelve
`conformance/scenarios/sched/ops/` scenarios through a real worker); the
driver ops and goldens are the oracle maintainer's (J11).

| Spec section | Anki files read | Read for |
|---|---|---|
| manual-ops M1 | `rslib/src/search/mod.rs` (cards for ids, preserved order or not), `rslib/src/storage/card/mod.rs` (`search_cids` tables, cards of notes), `rslib/src/card/mod.rs` (card update stamps), `rslib/src/collection/transact.rs` | missing ids skipped, card id order versus caller order, a repeated id failing the keyed temp table, every written card stamped, `col.mod` only with changes |
| manual-ops M2, M3 | `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/scheduler/service/mod.rs`, `pylib/anki/scheduler/base.py`, `rslib/src/ops.rs`, `ftl/core/studying.ftl`, `ftl/core/actions.ftl` | the queue table, suspended cards skipped by bury, note ids expanded, step names |
| manual-ops M4, M5 | `rslib/src/scheduler/bury_and_suspend.rs`, `rslib/src/scheduler/filtered/card.rs` (queue from type), `rslib/src/search/sqlwriter.rs` (deck with children, buried states), `qt/aqt/overview.py`, `rslib/src/scheduler/congrats.rs`, `rslib/src/storage/card/congrats.sql`, `qt/aqt/browser/browser.py` | which queues are restored, the deck tree by name, the deck page's three choices and when they appear, the browser toggles |
| manual-ops M6 | `rslib/src/scheduler/new.rs`, `rslib/src/revlog/mod.rs`, `rslib/src/config/mod.rs`, `rslib/src/config/bool.rs`, `rslib/src/storage/card/data.rs`, `qt/aqt/operations/scheduling.py` | positions and `nextPos`, the last position of new and other cards, the marker row, the remembered choices and their defaults, which data keys go |
| manual-ops M7 | `rslib/src/scheduler/reviews.rs` (Grade Now), `rslib/src/scheduler/answering/mod.rs` (answer without queue update, siblings, leech, load balancer), `qt/aqt/operations/scheduling.py` | per-card states inside the transaction, time 0, ids per card, no queue check |
| manual-ops M8 | `rslib/src/scheduler/filtered/mod.rs`, `rslib/src/scheduler/filtered/card.rs`, `rslib/src/error/filtered.rs` | the cards of the deck, the restore rule, the refusal on a normal deck |
| set-due-date S2 | `rslib/src/scheduler/reviews.rs` (parser), `qt/aqt/operations/scheduling.py` (dialog) | accepted forms, unsigned 32-bit bounds, Unicode digits refused by the number parse, the dialog's blank-text cancel |
| set-due-date S3-S5, S7 | `rslib/src/scheduler/reviews.rs`, `rslib/src/scheduler/new.rs`, `rslib/src/scheduler/filtered/card.rs`, `rslib/src/revlog/mod.rs`, `rslib/src/card/mod.rs` (shifted difficulty), `rslib/src/timestamp.rs` (elapsed days), `rslib/src/scheduler/timing.rs` (timestamp test), `rslib/src/deckconfig/mod.rs`, `rslib/src/storage/deckconfig/mod.rs` (fallback preset, bounds on read) | the three interval rules with FSRS, the SM-2 rule, the starting ease and its fallback, the manual row and its truncated factor, card id order, wrapping |
| set-due-date S6 | `rslib/src/config/string.rs`, `pylib/anki/scheduler/base.py` | the two keys, their defaults, the text stored as typed |

Oracle-library checks behind the spec's examples (FSRS on): difficulty 5.048
gives manual factor 549 and 6.2 gives 677 (truncation); `4294967295` on day 0
stores due -1 and `4294967296` is refused; the Reset order table of M6.2; a
learning card keeps `left` through Set Due Date and Reset; Grade Now makes a
suspended or buried card active; Empty stamps a card with `odid` 0; a
repeated card id fails bury, suspend, unsuspend and Set Due Date with a
database error and is handled twice by Reset and Grade Now.

Harvest rows classified for this unit: the bury, suspend, forget and resched
tests of `pylib/tests/test_schedv3.py`; `rslib/src/scheduler/reviews.rs`
`parse` and `due_date`; `rslib/src/scheduler/new.rs` `scheduling_as_new`; the
bury, suspend, unbury, set due date, schedule-as-new, Grade Now and Empty
tests of `rslib/src/scheduler/service/mod.rs`. `new.rs` `last_position` is
U42's row (`sched/data/positions`); `sched/ops/reset/restore-position` covers
it too.

For the oracle maintainer (the calls the dry run used; J1-J9 are the
contract): `bury` and `suspend` through the backend's bury-or-suspend call
with the card ids or the note ids and the user-bury or suspend mode;
`unsuspend` through the scheduler's unsuspend of cards (the restore of buried
and suspended cards); `unbury_deck` through the scheduler's deck unbury with
the all, user-only and sched-only modes; `set_due` through the scheduler's set
due date with the browser or reviewer string config key or none, mapping the
backend's invalid-input error to `invalid_days`; `forget` through the
scheduler's schedule-as-new with the two booleans and the browser or reviewer
context (the pylib wrapper always asks for the log row); `grade_now` through
the backend's Grade Now with the rating enum; `empty_filtered` through the
scheduler's empty of a filtered deck after checking that the named deck is
filtered (`not_filtered` otherwise, before calling Anki). Deck names resolve
from the collection at the step (deck-ops.md K1); repeated references are
`invalid_scenario` before any call (J1).
