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

The isolated native run `swift-test/20261114T165150Z-40353` compiles and
runs 54 tests: 53 pass and one fails. The failing equality expects an ungrouped
UInt64 maximum; the native English formatter preserves every digit and adds
thousands separators. Follow-up `cleanroom/20261114T165532Z-42538` changes
only that exact expected string. Production formatting and all assertions
remain intact. The original failing result is retained.

`swift-test/20261114T165629Z-43001` passes all 54 focused tests after the
exact locale expectation correction. Native Core products are reused. This
covers the store, timeline, standalone Settings component, presentation and
real safe-copy producer controls; it does not cover the pending app wiring.

## Production wiring checkpoint

Author `cleanroom/20261114T164556Z-37662` wires the store through production
composition and the existing disclosure. Review `165752Z-44726` finds no
P0/P1 and four accuracy issues. Repair `170255Z-50752` addresses them;
review `171457Z-53607` finds no P0/P1 and a blocked fixture. Follow-up
`171941Z-57429` tests the actual owned-command order. Review
`172650Z-62829` finds no P0/P1 in that scope and one P2 cancellation-wait gap.

`swift-test/20261114T172656Z-63225` compiles and runs 83 tests: 78 pass,
five fail with eight issues. The fixture requests failed-attempt history
after successful exchanges, which populate completed history instead. Two
cleanup failures expose the no-host close path attempting a host-required
flush. The assertions remain intact; these causes still need repair.

`private/20261114T172732Z-64949` passes. Test-only compile follow-ups
`165852Z-47034` and `171654Z-56059` change no production behavior. Failed
compiler/signature results and affected generated bundles are retained.
Exact rendering, app-hosted exclusion and final integration remain open.
This is WIP, with no Unit or gate acceptance.

## Verified repair and signed preview

The five-file repair preserves the original no-file close path and admits
fresh no-collection close without a host flush. Directory authority uses
Darwin search-only descriptors and opens before creating missing components;
nofollow, parent identity and substitution checks remain. The native view
uses leading content, semantic metadata, separators and adjacent actions.
Moving the disclosure identifier to its label preserves child identifiers.

Source review `cleanroom/20261114T180044Z-75790` and UI review
`175553Z-74236` found no P0–P2. Native run
`swift-test/20261114T180053Z-76203` passes all 86 focused tests.
The final export `cleanroom/20261114T180704Z-81405` contains five files.
It differs from the tested source only by a deleted test comment.

Final checks pass: privacy `181034Z-83947`, static `181046Z-85736`, and
similarity `181101Z-86324` (five files, no flags). Signed standalone build
`build-app/20261114T181042Z-84732` passes. Direct inspection of that exact
normal sandboxed build confirms available saved history, distinct Refresh
and Copy Debug Info identifiers, native layout and copy feedback. After
two normal quits and relaunches, each view retains observations from the
preceding process. No collection changes or manual sync were performed.
This is host feedback under the authorized exception, not a VM gate pass.

App-hosted composition check `mac-hosted-test/20261114T173630Z-67823`
passes with no skips on the earlier wiring checkpoint. Real failed-attempt
native cursor coverage remains a gap; pure projection coverage is retained.
U113, P24, full integration and peer round-trip acceptance remain open.
No Unit trailer or milestone acceptance is claimed here.
