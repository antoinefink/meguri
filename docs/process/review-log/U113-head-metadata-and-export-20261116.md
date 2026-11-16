# Head metadata and export fixture repair

Start: `35323460b`. Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`. Unaccepted U113 WIP; no Unit trailer.

Full gate `20261116T005859Z-94440` completed with a failed result. Its Rust suite ran 5,254 tests: 5,249 passed and five failed. The receipt retains other native/tooling failures, an error and pending work. T1 checks skipped after T0 are not passes.

Four active Rust failures are repaired here:

- A stamp-only note-type head write no longer decodes unchanged child rows to classify structural effects. Exact journal config images decide whether kind/sort need comparison. Differing unreadable configs still fail closed; actual structural effects retain the full shape and authored-report checks. Both damaged-ordinal host cases preserve their original inputs and assertions.
- The export preservation fixture waits for startup maintenance after derived idle, before taking its full-tree baseline. A queued Query attachment was a reachable source of shared-memory changes; the observed actor is not proved. Original Busy, zero-capacity-call and exact tree comparisons remain intact. No production export or lifecycle change.
- The existing CloudKit sort-field fixture brackets its declared identity layout and sort-index change with a checked report. Source, stamps, schema admission, note writes, sending fences and reopen checks remain unchanged.

Filtered exports: `cleanroom/20261116T012218Z-31840`, then `20261116T013445Z-34857`. Root read both complete patches. Independent reviews `20261116T012807Z-33181` and `20261116T065856Z-23616` are clear of P0–P2 findings in their scopes. Root verified the original assertion bodies and the unchanged seed files; review 33181 did not independently compare its unavailable baseline.

Verification:

- `core-test/20261116T065928Z-24828`: nine focused tests passed, including all four active failed cases, the second export control and four new guard tests.
- `core-test/20261116T070312Z-26769`: 361 guard, Undo, export and CloudKit regression tests passed. Excluded tests are not counted as passing.
- `private/20261116T065930Z-25109`: all four source files passed.
- `core-lint/20261116T070515Z-30765` passed. `similarity/20261116T070519Z-31081` scanned all four files with no flags or allowlist use; its filtered implementer view was read. Static checks and record privacy checks passed.

The fifth Rust failure, `normal_sync::delayed_meta_structural_schema_change_requires_one_way_before_start`, is a frozen server-only scenario. It remains failed and unchanged. The shared gate cannot separate that frozen requirement; this records the limitation, not a pass or waiver.

The batch gate remains failed. Native failures and production structural-generation wiring remain open. Real Mac–phone review convergence remains unproved; native local review persistence is recorded separately.
