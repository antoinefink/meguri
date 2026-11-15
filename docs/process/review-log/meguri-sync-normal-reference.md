# Current Desktop incremental reference observations

Twelve selected reference observations are independently reviewed and reproduced
from committed source. This prepares incremental implementation; it is not a
candidate-server normal-sync result or whole-family acceptance.

The source unit is `bf2173a2`; the canonical rerun used
`d4264d50ea2cc2a3292df4566eb81e45ff82df6f` with reference-input SHA256
`31d39b7f70b26c1865dad39ed4bb1968a4eb659c8dc34ff4fd854eb9bd289a3e`.
Desktop backend 26.09.4, build c06370ff, and its Python lock remain unchanged.
The invocation retains its historical harness batch base `38235917`; the server's
current coherent batch still starts at `aa856571`. Neither field is relabelled.

Canonical receipts under `build/mg/sync-server-reference/`:

- `20261115T221621Z-36883/result.json`: 39 selftests passed.
- `20261115T221627Z-38008/result.json`: 12 selected, completed and matched cases.
- `20261115T221649Z-40070/result.json`: 21 unchanged smoke cases passed.
- `20261115T222012Z-41596/result.json`: retained twelve-case validation passed.

All four runs have zero skips. The preceding absolute-path validation invocation
was refused before execution at `20261115T221657Z-40684`; the corrected run uses
the documented build-relative path. The source author retains earlier SDK
boundary failures and the ten-match/two-difference result unchanged.

Independent source review found two issues: checkout-dependent author lookup and
a server last-sync claim measured from receiver-local state. Both are repaired.
Explicit committed source-author records replace path inference. Raw served-file
stamps now distinguish finish from the later download's separate stamp advance.
Targeted closure: `/private/tmp/meguri-sync-normal-reference-p2-independent-review.md`.
Original review: `/private/tmp/meguri-sync-normal-reference-independent-review.md`.
Both review sessions and the source author's original session commit remain in
history. No source commit was reconstructed from failed exports.

The neutral archive
`conformance/fixtures/meguri-sync-server/observations/desktop-26.09.4-normal.json`
has SHA256 `33e4f576a85f19bf877baefc382e91121e4e0307fe73b4b06fc8e24807e4d4a7`.
It records original synthetic outcomes and explicitly separates Desktop traffic
from auxiliary protocol fixtures. The current-client specification documents
only these selected facts. Card/type/preset conflict matrices, name collisions,
configuration replacement/null, lifecycle faults, Mobile and candidate normal
sync remain unobserved here. Reviews and deletions run forward only; media is off.

All eight source hashes match the independent review. Canonical static and
privacy checks passed. The scoped similarity job passed for supported files;
quarantined oracle files remain excluded from similarity coverage. Five matching
files use reviewed exact-path and full-file-hash privacy entries; matcher logic
and thresholds are unchanged. The reference source stays quarantined.
Production authors receive neutral
specifications and original neutral observations, never oracle source or raw
runtime artifacts. Public classification is not legal ownership certification.

The required T1 reference-normal run preserves this finite selection at coherent
gates. Its registry and existing planner/scanner checks passed 4/4 at
`build/mg/selftest/20261115T221406Z-35705/result.json`. This adds no candidate
normal lane and grants no AnkiMobile, GUI, package or final server acceptance.
