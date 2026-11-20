# Independent review of the Browse and Note Types specs, 2026-11-20

Scope: PR #17 (`unit/browse-specs`): `search/filter-tokens`, `browser/workspace`, `notetypes/workspace`, `notetypes/draft-save`, `sync/meguri-provider` MP7, the changed G4 of `notetypes/schema-changes`, the "Changed 2026-11-20" lines in `search/*`, `browser/*`, `notetypes/editor-actions`, `render/preview`, `stats/card-info`, `storage/config-table` C8, contract N21 and N22, the inventory entries and the implementation plan.

Author: spec-writer session `s-20261120T154704Z-1e3b6b1f`. Reviewer: a separate read-only agent session that wrote none of the text and read nothing under `oracle/` (except the new provenance file), `refs/` or `local-data/`, and no Anki source.

## Pass 1 (on 0c2c2a8f and earlier)

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
