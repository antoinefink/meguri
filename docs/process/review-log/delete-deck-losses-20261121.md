# Delete Deck's confirmed losses: review log, 2026-11-21

Branch `unit/delete-deck-losses-spec` (PR #51), spec-writer session `s-20261121T074351Z-13f3004f`. The lines marked "Changed 2026-11-21 (confirmed losses)", or carrying that marker inline, close the window the round 6 implementer reported: Delete Deck had one write between its last summary read and its commit, so a card arriving then (an import, a sync round, another window, the local API) was deleted without having been shown. They were decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs.

## Decisions

1. `decks/decks` D8 step 6 (new), the shape of `notetypes/structure-changes` S12 and `notetypes/draft-save` D8:
   - The delete takes confirmed losses: the `decks` and `cards` counts of D8.5's summary, totals over the list, each compared with its own live count. `decks` is compared too because N20.7's question states the subdecks; at `cards` 0 this costs at most one automatic retry, accepted to keep one rule.
   - Live counts twice: on the planning snapshot before the backup (a refusal there takes no backup), and in the write transaction after the preconditions and before the first write.
   - A larger live count: `LossesChanged { decks, cards }`, nothing written, no undo step, undo and redo as they were. `Stale` wins and replans.
   - A planning count with no cards and a write-transaction count with cards ends `Stale` and replans, so the R2008 backup is never skipped (also with `any`).
   - Callers with no question pass the counts they read at the call (the conformance executor, K5). The one deliberate difference from draft-save D8: AnkiConnect's `deleteDecks` passes `any`, since `cardsToo` true accepts every loss and counts would add a refusal Anki never gives.
2. N20.7 and `browser/sidebar` S9.3: the app passes the counts of its last summary read; on `LossesChanged` it shows the question again with the new counts, or deletes at once when `cards` is 0; no failure alert.
3. D8.4, D8.5, D14 and D16 follow; D16 lists the tests.

## Independent review

Reviewer: an independent Claude subagent (Opus). It received the diff and the background problem, not the writer's conclusions, and read D8, N20.7, `engine/op-model` O6 and O7, S12, draft-save D8, `decks/delete.rs`, `notetypes/remove.rs` and `DeckDeletionModel.swift`.

- P1: none. The only unconfirmed deletion left is a swap that keeps both counts equal, the by-count trade-off draft-save D8 already chose.
- P2, all fixed:
  - The backup could be skipped while cards are removed if the table preconditions were dropped (planning saw 0 cards; `any`, or a count back at its confirmed value at commit). New "Backup at commit" bullet, and a D16 test.
  - Browse's sidebar Delete (S9.3) is a second caller: it now has its own marked line, and its Status line names it.
  - Step 6 and N20.7 named two different inputs for one case: a question confirmed, then a recheck that found no cards. Both now say: the summary of the most recent query before the op, which matches the question when one with cards was confirmed.
  - New Status lines in three files that had none broke `specs/README-format.md`; removed, the marked lines carry the decision inline as PR #37 did.
  - This log was missing.
- P3, all taken: K5 now follows `conformance/notetype-ops` T9 (numbers taken at the step); `any` is named as a deliberate difference; comparing `decks` at zero cards is stated as accepted; D14 and D16 wording; the filtered-deck sentence names home decks outside the subtree; N20.7's failure alert excludes `LossesChanged`; the second commit's `Spec:` trailer names S9.3.

Second pass on commit 1e258d08f: no P1 or P2 left; each fix matches its finding. Its three P3 notes were taken: the backup check counts any attempt's backup, it runs before the second count's comparison (its `Stale` wins), and the code list names the bridge error.

Confirmed by the reviewer: every interleaving that raises a count is refused (cards or decks added to the deck or a subdeck, a deck renamed into the subtree); filtered-deck moves leave the counts unchanged; a removed listed deck is a no-op without backup; `Stale` precedence and the kept backup match `package/backup-triggers` T6.2; the refusal matches `engine/undo`; the app loop needs a write per repeat; no sentence describing the window as open is left in `specs/` or `docs/`.

## Code changes required

- Core: `decks::delete` takes the confirmed losses (counts or `any`), the two counts, `LossesChanged`, the backup-at-commit check; the FFI's `start_delete_decks` takes the counts and reports `LossesChanged { decks, cards }` as its own bridge error, apart from the `Couldn’t Delete` failures; the testkit's K5 executor passes the counts it reads.
- App: `DeckDeletionModel` passes its last read and handles `LossesChanged` as N20.7 says.
