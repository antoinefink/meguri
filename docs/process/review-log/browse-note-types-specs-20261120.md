# Independent review of the Browse and Note Types specs, 2026-11-20

Scope: PR #17 (`unit/browse-specs`): `search/filter-tokens`, `browser/workspace`, `notetypes/workspace`, `notetypes/draft-save`, `sync/meguri-provider` MP7, the changed G4 of `notetypes/schema-changes`, the "Changed 2026-11-20" lines in `search/*`, `browser/*`, `notetypes/editor-actions`, `render/preview`, `stats/card-info`, `storage/config-table` C8, contract N21 and N22, the inventory entries and the implementation plan.

Author: spec-writer session `s-20261120T154704Z-1e3b6b1f`. Reviewer: a separate read-only agent session that wrote none of the text and read nothing under `oracle/` (except the new provenance file), `refs/` or `local-data/`, and no Anki source.

## Pass 1 (on 2a9b5448c)

6 must-fix, 17 should-fix and 8 nits. All were resolved in the commit "Fix the specs where the independent review found them inexact", except as noted below.

- Must-fix 1 to 6: fixed. These covered the single-OR round trip, the ambiguous flag, untagged and leech readings, the reading of "all bulk actions" (now an open point for Antoine), the retry upload, kept column sets for deleted note types, and safe CSS insertion with a defined malformed-CSS test.
- Should-fix 7 to 23: fixed. Two changes go further than the finding asked:
  - Column-set writes take the current-deck selection's skip-undo class, which keeps the undo steps and empties the redo history.
  - The Delete rule is now listed as an open point, because the design's text and the decision row differ.
- Format and privacy (24, 25): fixed.
  - Status lines now name R1232 or R915 and list their sections.
  - Sample fields are now neutral.
  - The speed tables cite the 150k fixture.
- Nits: all applied.
  - The system font is written as `system-ui, -apple-system, sans-serif`.
  - The search-token exception is recorded in N21.2.

Not changed: none.

## Approval, 2026-11-20

Antoine approved these specs in chat on 2026-11-20 ("I approve. And even more generally you have full permission to update specs yourself"), after PR #17 merged. Before that, the AI reviewer ran five rounds on the PR; every finding was fixed or answered. The later rounds added the confirmed-losses precondition for the note type draft save, Change Note Type, Delete Note Type and Restore to Default (`notetypes/draft-save` D8). The reviewer did not run on the last commit of round five (8657e4530), which wrote the confirmed-losses lines of `notetypes/change-notetype` M8, `notetypes/structure-changes` S12, `notetypes/restore-to-default` D7 and `browser/config-keys` K6.4; the first independent review named above did not cover them either. They were reviewed afterwards, on PR #22: the AI reviewer found that D7 left out the cards a kind change removes (D4.1) and that `browser/sorting` S1's refusal sentence contradicted the field sort key; both are fixed in PR #22. A separate independent review of that commit's lines is recorded below when done.

The Status lines of the round 8 and round 9 specs, MP7 in `sync/meguri-provider`, N11.10, N21 and N22 in the contract, and the inventory entries now say approved. The empty section list in `notetypes/restore-to-default`'s Status line now names D7.

From this date Claude (coordinator) may change specs without asking first; each change still gets an independent review and is recorded with that attribution.

## Review of the confirmed-losses rule (on 8657e4530 and 62e7f72ca)

A second independent read-only review covered the confirmed-losses rule of the four destructive note type ops and the lines around it. It found 17 findings: 4 must-fix, 9 should-fix and 4 nits. The coordinator accepted all of them and chose among the reviewer's options. All are fixed on `unit/confirmed-losses`:

- **Must-fix:**
  1. M8 now counts with M5's field-content test.
  2. M8 now confirms M9.1 and M9.2 card removals as one more count.
  3. The conformance executors (T9, H2, M1, M2) now pass the counts they read at the step.
  4. Restore to Default has a `restore_summary` read (D7.1, owned by U34).
- **Should-fix:**
  - D8 defines the comparison by count, per category.
  - The four ops count again before the backup and inside the transaction. A staleness refusal wins, a refusal records no undo step, and the backup stays (T6.2 also says so).
  - MP7 and D5 use one trigger, and the MP7 job now has a defined end.
  - The G4 paragraph covers all four ops.
  - The old input and carry-over sentences of M8, M10, S12 and D1 now name the confirmed losses.
  - Restore carries a digest, and `LossesChanged` replaces the silent replan when losses were confirmed.
  - A vanished note type is `Stale`.
  - The plan rows of U34, U35, U267, U269 and U284 list their cases and owners.
  - K6.4 and E7 now agree with the round 8 field.
- **Nits:**
  - The descending-order wording in `browser/sorting` S1.
  - The restore sync condition in D3.
  - "The cards S6 and D4.1 remove" in D7.
  - D4's "No cards." row.

Also checked:

- `package/backup-triggers` T6.1 and T6.2: a new line covers a refusal after the backup.
- No local API or AnkiConnect action exposes the four ops. AnkiConnect's field, template and model-change actions are unsupported, and the MCP note tools are read-only for note types. D8 says so.

## Re-review of the confirmed-losses fix (on 3f8cb1503)

The same reviewer re-read the fix. All 17 findings are closed, and the reviewer found no remaining path to unconfirmed loss. Ten small items remained, and two CodeRabbit comments on #22 were added to them. The coordinator handles item 5 (the Status lines). The rest were fixed in one commit:

- The order paragraphs say the first count refuses before the backup, and that the second count alone guarantees the commit. The staleness list now names all of each op's checks.
- D7.1:
  - It defines `kind_change_cards_removed` as the cloze-to-standard removals only.
  - It refuses with `NotFound` and `NeedsStockKind`.
  - It requires the caller to pass the same `kind` to the summary and to the op.
- D7's card line points to D7.1's lines, and D7 begins with its Status line's prefix.
- D8's restore test list includes the case with confirmed losses.
- MP7 step 2 states the restore outcomes, and step 3 counts a changed digest as an unconfirmed loss.
- D9 lists "No cards.".
- E7 names the bullets it replaces.
- T6.2's bullet states the coalescing rule for retries.
- `conformance/browser-ops` B1 has the `fld:` exception.
