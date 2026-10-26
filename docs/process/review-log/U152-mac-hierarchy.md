# U152 — native Mac deck-tree interaction

Supplemental start: `be370f3d9a0e7d0f80aa4d03e94b07302294be6a`, recorded
before work in `build/unit-starts/U152-mac-hierarchy-20261026.json`.
Original U152 start and pending integration base remain those in `U152-home.md`.
This repairs the expanded-tree failures found during U156 visual verification.
Full U152 remains open; no Unit trailer or platform acceptance is claimed.

## Scope and independent review

Filtered implementer and fixer sessions reuse the recorded sessions from
`U156-name-actions.md`. No Anki source or private collection was used.
The Mac tree keeps ordinary native DisclosureGroup, List, row selection and
menus. Whole-disclosure accessibility overrides propagated the parent ID into
children. The final smaller view repair removes those Mac-only overrides;
the iOS disclosure metadata remains unchanged.

AppKit's repeated `NSOutlineViewDisclosureButtonKey` is classified only for
the exact identifier, disclosure-triangle role, direct cell parent and ordered
outline-row/outline ancestry. App IDs, missing adjacent IDs and geometry
checks retain their assertions. Adversarial tests exercise wrong names, roles,
tree structure, repeated app IDs and nearby text bounds.

Independent source reviews R1–R3 retained actual row identities, full labels,
counts, unique IDs, native triangle state and real selection/menu actions.
The native test uses the generated local-review fixture, not a screen route.
It checks both trees and a selected child's page during ancestor collapse.
Persistence uses native Quit, waits for termination and relaunches without
reseeding the database. Unsupported synthetic container IDs and Group values
were replaced by assertions on the actual native rows and triangles.

## Retained failures and focused evidence

- `image-tools-selftest/20261026T020748Z-77931`: PASS, 160 checks; the final
  checker and its assertions have not changed since this run.
- `drive/20261026T023548Z-94752`: PASS, 20 actions and no hierarchy issues,
  for the earlier label-container candidate. It is not a final-source render.
- `ui-test/20261026T023759Z-97838`: FAIL, two tests expected an absent label
  container. The smaller native repair is `cleanroom/20261026T024412Z-1548`.
- `ui-test/20261026T024913Z-3751`: persistence PASS; interaction FAIL at a
  label-only menu lookup. Actual native menu items have titles, not labels.
  Exact owned action IDs now select them; caption, order, uniqueness and enabled
  assertions remain. Export: `cleanroom/20261026T030203Z-8797`.
- `ui-test/20261026T030539Z-10202`: FAIL at sidebar parent selection after
  prior disclosure, root selection and child menu assertions passed. The actual
  parent cell became one row-wide MenuButton; clicking opened its actions menu.
  This is a product hit-target defect, not a selector exception.
- `cleanroom/20261026T031558Z-15443` tried native borderless MenuStyle only.
  `ui-test/20261026T031652Z-16670` still FAILS at that same parent click. The
  failure's native menu height changed, but the row-wide hit target remained.
  That proposed repair is insufficient.
- `cleanroom/20261026T032000Z-20212` uses explicit native button MenuStyle
  and borderless ButtonStyle on the trailing Mac menu. The original test and
  navigation requirements stay unchanged. `ui-test/20261026T032127Z-21204`
  still FAILS at the same parent click, with the same combined MenuButton.
  Styles alone do not repair this row structure.
- Final structural export `cleanroom/20261026T032509Z-24194` gives the Mac
  sidebar selection a plain native Button beside the trailing Menu, matching
  existing sidebar section actions. It calls the existing awaited owner and
  keeps real disclosure, tags, badge, full labels and context actions. Only
  the deliberate leaf Text-to-Button role assertion changes.
- `ui-test/20261026T032642Z-25356`: PASS, one executed and no skips. The
  original complete interaction method now passes: both trees' disclosure,
  row identity/labels/counts, parent and child selection, child-page continuity
  during ancestor collapse and both child context menus. The parent click
  opens its deck page instead of its menu. No assertion or timeout was relaxed.
- `selftest/20261026T031847Z-19376`: PASS, seven UI-job tooling tests.
- `private/20261026T031744Z-18239` and
  `similarity/20261026T031746Z-18480`: PASS for the preceding candidate.
  Final-source and log scans remain required.

Native triangle values are checked as exact 0/1; unknown values fail.
Menu captions are checked within the exact action-ID query. No test is skipped,
repeated until green, relaxed to accept a menu instead of selection, or given
a larger time budget. The pending original-base integration gate and all
remaining U152 requirements remain open.

Independent R4 finds no actionable P0/P1 or separate P2 in the final structural
delta. Native single-dispatch instrumentation is not provided by this test;
existing owner and real-core no-op checks remain separate evidence. The saved
collapse test passed in `024913Z-3751`; its body and persistence owner are unchanged.
The final interaction run also proves the updated shared native row-role check.
`drive/20261026T032905Z-27996` passes all 20 actions, with zero hierarchy
issues across 33 captures. Independent visual R5 inspects six final images:
the expanded tree and native sheet/result states at default/minimum sizes
in light and dark. It finds no actionable issue and recommends no change.
Menu operation is proven by the native interaction test, not these stills.
This is scoped visual review, not the official milestone critic or acceptance.

Final structural privacy `032734Z-26911` passes all five selected source,
guidance and log files after a neutral log rewrite. Similarity
`032734Z-27106` passes the changed Home and native-test files; the unchanged
final Menu file also passed `032307Z-22898`. The tooling and classifier receipts
above cover their unchanged final inputs. Documentation updated after these
checks receives a final focused privacy scan before commit.
