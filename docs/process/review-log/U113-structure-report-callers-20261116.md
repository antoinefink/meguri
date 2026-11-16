# Structure-report caller repair

Starting commit: `40e601fae369bc85d79a72c35b4c56ab2a7e949a`. Integration base: `1416a520542a8c5daa38a23bfe398da704c3d6c9`. This is unaccepted U113 WIP, with no Unit trailer.

The legacy aggregate adapter now plans explicit ordinal maps, schema reasons and safety admission. It checks the exact source, destination or absence, and name namespace before writes. Previous physical keys resolve to logical positions; sort-field identity is separate from position. The observer, regeneration and freshness fixtures author their existing structural edits without changing their original assertions or timing limits. The kind-only incoming fixture checks the approved accepted-aggregate preconditions.

Review `cleanroom/20261116T000436Z-28329` found five P2 issues. Review `20261116T003630Z-62855` closed them and found two P2 issues: unrelated-row decoding and incomplete raw preservation assertions. Repair `20261116T004502Z-75131` adds a bounded target-only membership/ordinal query and exact stored-card comparisons. Its race test now changes the stamp in each case; the earlier test assigned the existing value in its second case. All prior Stale, aggregate and SCM assertions remain.

Independent review `20261116T005031Z-83086` is clear of P0–P2 findings in this scope. Export `20261116T005354Z-85467` differs only in two API-comment lines, checked by exact comparison. All three engine fixture files remain byte-identical to the earlier reviewed candidate.

Focused evidence:

- Baseline `core-test/20261115T235456Z-27062`: all six original failures reproduced.
- Initial repair: six regressions passed in `20261116T000504Z-28874`; 81 tests from the affected suites passed in `20261116T000905Z-30072`.
- Later candidate `20261116T004048Z-71531`: 13 of 14 new adapter tests passed; the new race fixture failed. This failure is retained.
- Repaired candidate `20261116T005100Z-83756`: all 15 focused tests passed, including bounded pagination, malformed target ordinals, unreadable unrelated text, raw card retention and real stamp races.

- Final source `core-test/20261116T005524Z-87048`: all 82 tests passed across CLI, notes, observers, regeneration, freshness and the new storage query.
- Final guards: `core-lint/20261116T005619Z-88556` passed; `private/20261116T005519Z-86640` passed for all six source files; `similarity/20261116T005628Z-88860` scanned six files with no flags or allowlist use. The filtered implementer report was read.

Unsupported mixed-kind physical arity and retained out-of-domain cards still return `StructureNotReported`. The proposed contract clarification remains under review and is not implemented. No full compatibility claim is made.

Full gate `20261115T231935Z-98189` was cancelled after the six completed failures; it is not a pass. The complete integration gate remains required. Native peer convergence is separate and remains unproved.
