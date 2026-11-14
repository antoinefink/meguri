# Native diagnostics work — 2026-11-14

Corrective scope for U153/U161. Source start:
`3817df62bcb4304a9c6bd8249baf065fed68ab8f`. No unit or gate acceptance is recorded here.

The user approved a local timeline and safe Copy Debug Info. The existing
store remains the owner. Raw system reports stay internal; export uses explicit
scalar projections. UI and operation producers are a separate pending pass.

Author: `cleanroom/20261114T150835Z-76162`, fixer session
`s-20261110T214025Z-20fa15fd`. Independent review:
`cleanroom/20261114T152157Z-83039`, session
`s-20261109T210639Z-47d06d5b`. The review found no P0/P1 and four P2 findings.
All four remain pending: event order, archive decoding, directory binding and
cloud export allocation. The complete report remains in that run's `codex.out`.

Evidence for this component:

- `swift-test/20261114T151741Z-81420`: nine baseline tests pass.
- `swift-test/20261114T152253Z-84058`: 21 store/timeline tests pass.
- `swift-test/20261114T152359Z-86017`: real producer test passes both cases.
- `similarity/20261114T152358Z-85751`: five files, no flagged matches.
- `private/20261114T152240Z-83784`: source passes after complete comment
  rewrites and an equivalent local test-variable rename. Assertions and
  privacy rules are unchanged; the earlier failure is retained.

The next pass fixes the review findings before exposing copied output. Native
UI, real operation observations, remaining integration checks and release
acceptance remain open. Existing system-report archive retention is unchanged.

## Store repair subpass

Author: `cleanroom/20261114T152953Z-88128`; independent review:
`cleanroom/20261114T154253Z-95524`. The review finds no P0/P1. Event order
and archive summary decoding are repaired by inspection. Two P2 issues remain:
child-directory authority reuse and recovery after initial directory failure.
The bounded cloud accessor remains pending for the integration pass.

`swift-test/20261114T154753Z-1380` passes 35 diagnostics/presentation tests.
`static-checks/20261114T154853Z-4036` passes. Shared source similarity
`similarity/20261114T154853Z-4156` scans 34 files with no flags. The source
privacy check passes after whole comment and local identifier replacements;
assertions and runtime behavior are unchanged. Earlier failures stay retained.

This remains WIP. Real operation producers and native copy actions are absent.
The integration pass must resolve the remaining review work before UI exposure.

## Bounded cloud export

Author `cleanroom/20261114T155500Z-9817` adds the read-only projection and
store authority fixes. `core-test/20261114T160741Z-18384` passes both Rust
boundary tests; original history stays complete. Source privacy passes at
`private/20261114T160720Z-18071`.

Independent review `cleanroom/20261114T160706Z-17646` is running. Native
bindings, the changed store callbacks and UI still need verification. This is
WIP and changes no sync decisions.

## Native view draft

Author: `cleanroom/20261114T155858Z-12586`; independent review:
`cleanroom/20261114T161259Z-23781`. No P0/P1 found by source inspection.
Three P2 items remain: refresh admission, callback owner identity and lengthy
introductory copy. The view is not mounted in normal Settings yet.

Native build `swift-test/20261114T161408Z-25019` fails catalog symbol
generation for a formatting-only key. No native test pass is claimed for
that run. The next source pass fixes the catalog, view findings and the
remaining read-only export refinements before normal app wiring.

Export review `cleanroom/20261114T160706Z-17646` found no P0/P1, with
three P2 defects (failed-write persistence, authority publication on read,
no-host initialization) and a production mapping coverage gap. All remain
explicitly pending.

## Read-only export and view repair

R4 source: `cleanroom/20261114T162006Z-28121`; follow-ups:
`163353Z-30613` and `163853Z-33518`. Review
`cleanroom/20261114T163029Z-29292`: no P0/P1. One P2 remains: closing
and reopening the log panel must share the same limit on concurrent reads.
The production callback owner must enforce that limit for its full lifetime.

The repair preserves pending writes after storage failure, avoids publishing
runtime authority while reading diagnostics, and avoids initializing Core when
no host exists. `core-test/20261114T163205Z-30149` passes all four focused
boundary tests. The catalog now uses named keys and observation dates use the
platform formatter with the selected locale and time zone.

`swift-test/20261114T163627Z-31528` fails before tests on a captured mutable
test variable. The final follow-up replaces only that fixture state with an
actor-isolated reference. Assertions and limits remain unchanged. The next
native result must establish compilation; this record claims no such pass.
Production producers, the Settings mount, rendered inspection and integration
acceptance remain open.

The combined native run `swift-test/20261114T164225Z-35243` fails before
tests in the separate R7 coordinator change: it throws a presentation enum
that is not an Error. The R7 export is retained for its final repair; it is
removed from this WIP checkpoint. No diagnostic test pass is inferred.
The combined privacy scan `private/20261114T164240Z-36411` flags four R7
files and no diagnostic file. Earlier failed scans remain retained.

Checkpoint `f4b3160d`: `static-checks/20261114T164611Z-38022` passes.
Similarity `20261114T164614Z-38371` scans eight files with no flags; the
review log is outside its source scope. Normal app wiring is a separate
filtered implementation pass. This checkpoint remains WIP.
