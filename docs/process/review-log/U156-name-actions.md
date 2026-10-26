# U156 — native deck-name actions

Starting commit: `3e8808ed0784467bbab89d86f37298e860a0b437`.
The intake record is `build/unit-starts/U156-name-actions-20261026.json`.
Integration base remains `1416a520542a8c5daa38a23bfe398da704c3d6c9`.

This slice connects Create Deck, Rename and New Subdeck to the existing U28
services. It does not complete U156. Go to Deck, drag/reparent, deletion,
collection Undo presentation and later provider actions remain outstanding.
No `Unit:` acceptance trailer is warranted.

## Source and independent review

The existing prepared audit supplied the contracts; no Anki source was read.
The filtered implementer session is `s-20261025T134357Z-235af9a2`, export
`cleanroom/20261026T005758Z-29480`. Fixes use
`s-20261025T162405Z-9dc656e4`.

R1 found two P2 issues: a hidden empty Default deck returned by Create was not
opened, and a post-commit selection failure discarded its retry. Fixer export
`cleanroom/20261026T011620Z-37511` retains the returned identity/detail and a
retry of navigation without repeating the mutation. R2 closed both in source.

R2 identified a follow-on P2: Rename still required a visible tree row for that
hidden selected deck. It also recorded the native compiler failure described
below. A bounded second fixer addresses both and rewrites only synthetic test
names that the privacy check flagged. Source and runtime acceptance remain open.

The independent reviewer also inspected the integration-only MG change that
stages the generated local-review fixture for the exact Mac deck-management
suite. It does not change other selectors or use personal app data.

## Verification

- `selftest/20261026T011824Z-39027`: seven UI-job tooling tests passed, no skips.
- `swift-test/20261026T012132Z-40797`: failed compilation at the two default
  async service closure properties under Swift 6 actor isolation. No model test
  execution is claimed from that run.
- `private/20261026T012153Z-42155`: failed at synthetic name regions in the new
  native UI test. No private spelling was disclosed; the guard remains intact.
- `similarity/20261026T012152Z-41902`: passed for the pending own-code changes.
- `doctor/20261026T011752Z-38413`: VM SSH/Xcode available; 54 GiB disk free.

R3 confirmed one further P1 compiler error: constructing the associated-value
`staleGeneration` case without its arguments. The exact two-line filtered repair
is `cleanroom/20261026T012839Z-46875`; R4 confirmed the repair and found no further
actionable issue in that delta. The other edit removed one flagged test comment;
the executable regression remains unchanged.

- `swift-test/20261026T012650Z-44596`: retained failure for that enum construction.
- `private/20261026T012717Z-46067`: retained failure at that one test comment.
- `swift-test/20261026T012958Z-47764`: 30 tests passed in two suites, no skips.
  This includes real returned IDs, hidden Default, rename/subdeck paths,
  post-commit failed selection without repeat mutation, and close/background drain.
- `private/20261026T013022Z-49194`: passed after neutral source rewrites.
- `similarity/20261026T013023Z-49427`: passed for the repaired source.
- `ui-test/20261026T013036Z-49947`: PASS; 1 executed; 0 skipped.
  It exercised New Deck and Rename shortcuts, Return/Escape, draft preservation
  across resize, actual rename preserving ID, and context-menu subdeck creation
  with the native insertion point after the stored parent path.

R5 inspected five actual Mac images from `drive/20261026T013233Z-52015`.
All 20 actions executed successfully, but the run failed with 86 hierarchy
findings. Fifteen were a real sheet-field overflow: its right edge extended one
point past the sheet, clipping the native focus ring in both appearances.
The filtered one-file repair `cleanroom/20261026T013539Z-55268` adds only the
standard Mac Form inset with `.padding()`. It is a functional clipping repair;
no custom style, fixed control size or numeric inset was introduced.

R6 inspected the same five states from `drive/20261026T013637Z-56115` and confirmed
the repair: all 15 field-overflow findings are gone and names remain readable.
No P0/P1 or new P2 remains in the new sheet. The whole drive still fails with
71 inherited findings: system identifiers (49); app disclosure duplicates (22)
in the existing Home/sidebar tree. The checks are
unchanged; these failures remain open. No full drive pass is claimed.

`ui-test/20261026T013802Z-58875`: PASS after the inset repair; 1 executed;
0 skipped. The 30-test model receipt remains valid because the sole later
production change is the Mac Form inset. `similarity/20261026T013836Z-60477`
passed for the final source.

The inherited expanded-tree failures are repaired by the separate U152 Mac
follow-up in `U152-mac-hierarchy.md`. Its final native interaction test passes,
and `drive/20261026T032905Z-27996` passes all 20 steps
with zero hierarchy issues. Independent final visual review is clear. Earlier
failed receipts remain retained; this does not complete U156 or its batch gate.

Mobile rendering, system accessibility audits and integration gate verification
remain pending. A build-created Xcode project rewrite outside
this slice is preserved separately in an owned stash and
`build/unit-intake/U156-name-actions/unowned-xcode-project-rewrite.patch`;
it has not been committed or incorporated into the feature.
