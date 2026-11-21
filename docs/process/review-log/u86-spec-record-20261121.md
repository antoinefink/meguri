# U86 spec record: review log, 2026-11-21

Branch `unit/u86-spec-record` (PR #73), spec-writer session `s-20261121T153312Z-252d5e5c`. The lines marked "Changed 2026-11-21 (implementation review)", or carrying that marker inline, in `browser/find-replace` F6 step 7, F11 and F12, `browser/bulk-actions` intro, B16 and B17, `tags/tags`' scenario table, `conformance/bulk-ops` intro, X1 and X5, `conformance/dsl` D7.2, `conformance/observations` O4 and `conformance/harvest-followups` H7 record what U86 (PR #63) built. The coordinator approved the sentences and rulings under Antoine's permission of 2026-11-20 to update specs (`docs/process/standing-decisions.md`).

## Decisions

1. The approved sentences are written as given, in the sections they name. Before the ten scenario rows changed from held to active, each was checked on `master`: the scenario file and golden exist, the scenario is under `active:` in its `area.yaml`, and the inventory test in `tags_conformance.rs` asserts that the four tags rows are active in T0.
2. F11's confirmed count is a rule that lands with U265 (in progress), not with U86, in the style of `notetypes/draft-save` D8.
Reviewer: a separate Claude subagent (Opus), given the diff and the sentence file only. It compared them with the other browser and conformance specs and with the scenarios, area lists and inventory test on the branch.
4. `browser/bulk-framework` keeps no Status line: `specs/README-format.md` requires none.
5. Plan section 9 records U86 (#63) as merged and U263 as PR #62, open.

## Independent review

Reviewer: an independent Claude subagent (Opus). It received the diff and the sentence file, not the writer's reasoning. It checked them against the rest of `specs/browser/` and `specs/conformance/`, and checked the scenarios, `area.yaml` lists and inventory test on the branch.

It confirmed that every approved sentence landed with its wording, that each replaced text is gone, that the ten scenarios are active, and that F11's rule matches D8 and `browser/bulk-framework` K3 to K5.

- P1:
  - Fixed. `tags/tags`' scenario table still called the four `tags/find-replace/` rows held. They are now active.
  - Fixed. The approved D7.2 cell carried neither "draft" nor "stable", against D7.2's status rule, while the bulk-ops intro called the shapes draft. The cell now reads "draft: the oracle implements it; Meguri runs it (U86; `bulk-ops.md` X1)": the approved words, after the prefix D7.2 allows. This needs the coordinator's agreement (reported on the PR).
- P2:
  - Fixed. `browser/bulk-actions` B17 said the Change Deck scenarios land with U86; now U87.
  - Fixed. X5's oracle bullet named D7.2's rows as draft; it now names the rows. "Those of X1 and X3" now reads "the scenarios that use X1 and X3".
  - Fixed. The bulk-ops intro now says "the ops' shapes are draft".
  - Fixed in part. `conformance/harvest-followups` H7 said the testkit does not parse `find_replace` or `find_replace_fields`. It now says those scenarios landed with U86, and that their `blocked` rows become `covered-new` when their catalog rows are linked. Linking the 11 rows in `conformance/catalog/harvest.csv` is left as a follow-up for the rows' owners.
  - Fixed. F11's refusal now says where it comes in the order (after every other refusal) and that U265 names it, as a K4 `EngineError::FindReplace` refusal.
  - Fixed. Each of the ten F12 State cells now carries the marker.
  - Fixed. This log was missing.
- P3:
  - Fixed. F11 no longer repeats F6 step 7's reason for the `NotetypeMissing` name.
  - Fixed. F11 points to V9's count c only. F10's "Matching notes" bullet counts matches.
  - Fixed. X1's Meguri bullet says that from U265 the executor passes the count it reads at the call.
  - Fixed. The bulk-ops intro wording, F11's sentence order, "lands with" used in both places, and the plan entry (it lists every changed section).
  - Not changed. F2 and F13 gain no note on the new refusal. F11 is the operation's contract and U265 writes the sheet's handling.
  - Not changed. No owner is named for the planned scenarios. Harvest-followups H1 already gives U246 the missing differential scenarios.
  - Not changed. The planned scenarios get no named owner here; harvest-followups H1 already assigns them.
