# U35 spec record and repeated field origins: review log, 2026-11-21

Branch `unit/u35-spec-record` (PR #57), spec-writer session `s-20261121T104205Z-fb5625d8`. The lines marked "Changed 2026-11-21 (implementation review)", or carrying that marker inline, record the coordinator's rulings on U35 (PR #46) and specify the follow-up unit U35a. They were decided by Claude (coordinator) under Antoine's permission of 2026-11-20 to update specs.

## Decisions

1. `notetypes/change-notetype` M5, M8, M10, M12 and `conformance/change-notetype-ops` H1, H2, H4 record what U35 built, as the coordinator ruled on PR #46: the layout token, the whole refusal order, `NoteFieldCountMismatch` in check 4, progress during planning only, and the executor's handling of the token and of its fallback read.
2. The same-type `type_changes` entry is an open point in M10 and in the paused-iCloud row of `docs/process/verification-status.md`. A change within one note type reports reason 6 alone.
3. R945's repeated field map between two note types (`sync/cloudkit-records` K19.2 "Repeated field origins", K19.6, `engine/undo` M9.11):
   - A `TypeChange` field map may repeat an old ordinal, in authored and retained reports. Nothing else may.
   - The inverse takes the lowest copy.
   - Undo restores notes from before-images, never through a map.
   - Receivers map through the entry as through any map. K19.6 and K19.8 keep a conflict copy only for a higher copy that was edited and now differs from the lowest copy.
   - The receiver side is parked with iCloud.
   - U35a removes the temporary `Unsupported` refusal and activates `notetypes/change/repeat-field`.
4. Plan section 9 records:
   - the merged units: #38, #41, #45 and #46;
   - U35a and its tests;
   - the `fsrs: true` check needed before activating scenarios that answer cards.

## Independent review

Reviewer: an independent Claude subagent (Opus). It received the diff, PR #46 and its threads, and the questions, not the writer's conclusions. It checked the text against the merged code (`notetypes/change/mod.rs`, `commit.rs`, the testkit executor, `journal_structure.rs`, `undo/entry.rs`) and the neighbouring specs.

- P1: none.
- P2, all fixed:
  - The paused-iCloud row had a duplicated last cell.
  - This log was missing.
  - K19.2's inverse rule was ambiguous for retained (composed) reports, against "Retained joined reports". It now applies to the final composed map. The text says why this is the exact reverse when an origin repeats, with an example.
  - The receiver wording differed between K19.2 and K19.6, and K19.6's would make needless copies. One rule now covers both: a conflict copy only when a higher copy differs both from its base value and from the lowest copy. K19.8 is named too.
  - The U35a tests lacked expected values. M12 now gives:
    - the inverse examples with their arities and results, which catch an inverse that keeps the highest copy;
    - a concrete composed report and its maps;
    - whole-row comparisons of notes, cards, graves and `col`;
    - the cards expected per note;
    - the cloze variants' template inverses;
    - the retained admission path that undo and redo use (also stated in K19.2).
- P3, all fixed:
  - Status lines now cover the lists and sub-bullets a marked line introduces.
  - M8 check 1 names the rule that holds after U35a.
  - H1 and H2 of `conformance/change-notetype-ops` are corrected as the reviewer asked (see their marked lines).
  - The staleness lists in M8 and in `notetypes/structure-changes` S1.2 and S12 name check 1's layout token.
  - M10 names the O9 catalog row.
  - The open point says the resolution must use a `TypeChange` entry or revisit K19.2.
  - M12 lists two more U35 tests.
  - Whole-order step 11 follows the code's order.

Confirmed by the reviewer:
- The refusal order matches `change`, `plan` and `check_live`.
- The layout token matches `LayoutToken::of`.
- `NoteFieldCountMismatch` runs in both passes.
- Progress comes from planning only.
- H1 and H2 match the executor.
- The open point matches `StructureReport::validate` and the pair selection.
- Undo restores before-images exactly (`mod` and `usn` included). The reviewer found no undo or redo path that misrestores a field value. It checked a short note, redo then undo, standard and cloze pairs, and composition.
- The changed lines agree with the neighbouring rules the reviewer was asked to check (draft save, structure changes, op model, undo, change stream, CloudKit records).
- The cross-references are right.
- No Anki code, path, test name or non-interface identifier appears in `specs/`.
