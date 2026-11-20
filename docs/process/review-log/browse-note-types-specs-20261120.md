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

Antoine approved these specs in chat on 2026-11-20 ("I approve. And even more generally you have full permission to update specs yourself"), after PR #17 merged. Before that, the AI reviewer ran five rounds on the PR; every finding was fixed or answered. The later rounds added the confirmed-losses precondition for the note type draft save, Change Note Type, Delete Note Type and Restore to Default (`notetypes/draft-save` D8). The reviewer did not run on the last commit of round five.

The Status lines of the round 8 and round 9 specs, MP7 in `sync/meguri-provider`, N11.10, N21 and N22 in the contract, and the inventory entries now say approved. The empty section list in `notetypes/restore-to-default`'s Status line now names D7.

From this date Claude (coordinator) may change specs without asking first; each change still gets an independent review and is recorded with that attribution.
