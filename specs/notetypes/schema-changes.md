# Schema changes

Spec: notetypes/schema-changes
Requirements: R949, R950, R951, R1475, R1476, R1477, R1590 (core side), R2053 (core side), R918, R948 (schema side), R1889 (G9)
Anki: 26.09.3 for G1's meaning of `scm` and `ls`, G2, G3, G5 and G9's Anki column; G1's CloudKit rules and the scope of `ls`, G4, G6, G7's CloudKit part, G8 and G9's Meguri column are Meguri design from the Product Contract
Scenarios: conformance/scenarios/notetypes/gate/
Status: Approved 2026-11-15 (R2066) for the new normal-transfer aggregate cross-reference paragraph in G7. Under review (R915; round 9, decided 2026-11-20) for the paragraph of G4 that begins "Changed 2026-11-20": do not implement it until approved. Every other section is in force and unchanged.

Which operations mark the collection's schema as modified, what that means for the next manual Anki transfer (KTD26) and for CloudKit (R2053), and the guard that keeps a changed note type structure out of an incremental Anki transfer. Meguri asks no confirmation before a schema change (G4). Destructive-mapping previews belong to their units (U34, U35, U121, U200, U201, U203, U210, U232). The manual-transfer side is U106, U107 and U109 (frozen with the Anki server work); the CloudKit side is U113 and U114 (R2053; generation layout: `sync/cloudkit-records#K19`). Columns are `model/row-records` R2 and `storage/ids-usn-mtime` S6.

## G1. The schema stamp

- `col.scm` is the instant of the last schema change and `col.ls` the instant of the last sync, both epoch milliseconds.
- `ls` records Anki syncs only: in Meguri, a manual Anki transfer (KTD26). CloudKit sync state is never derived from `ls` or `scm`, and a CloudKit exchange never moves `ls`.
- An incoming CloudKit generation is applied as a write on the open collection and passes G6 like any other write. When the applied generation changes a local note type's structure (by G3's effect test: its stored field or template positions or its sort field), removes a note type, removes a deck preset or moves notes to another note type, the apply sets `scm` to `now_ms` in its own transaction (reason `cloudkit_structure_applied`, G2). The next manual Anki transfer from that device is then one-way. The generation's layout and its apply are `sync/cloudkit-records#K19` (K19.2, K19.5); this rule does not depend on them. `cloudkit_structure_applied` publishes nothing (K19.1).
- The schema is **modified since the last sync** when `scm > ls`. The next manual Anki transfer then has to be one-way (U106, U109), and its preview says so before anything is sent or replaced (R1476). A collection never synced has `ls` 0, so its schema always counts as modified.
- After a one-way upload, `ls` equals `scm` (U109); after an incremental transfer, `ls` is the server's time, later than `scm`. Either way the schema is no longer modified until the next schema change.
- A schema change never makes routine CloudKit sync one-way. Its commit starts a coherent new CloudKit generation, which publishes the new structure together with the rows that depend on it (R2053, U114).

## G2. Classification (R949, R950, R1475)

Each op that can change the schema declares a reason. The table is the whole list: no other op ever sets `scm` (R1590).

| Reason | Op (owner) | Sets `scm` | Structural edit |
|---|---|---|---|
| `fields_changed` | a note type save whose fields are added, removed or reordered (U34) | yes | yes |
| `sort_field_changed` | a note type save that makes another field the sort field (U34, R926); an `.apkg` update of a note type that changes its sort field (U120, `package/apkg-import#I7`) | yes | yes |
| `templates_changed` | a note type save whose templates are added, removed or reordered (U34) | yes | yes |
| `notetype_removed` | deleting a note type (U34, R918) | yes | yes |
| `notetype_restored` | Restore to Default when it changes the field count or the template count, or when a given stock kind changes the note type's `kind` (U34, R914; `notetypes/restore-to-default#D5`) | yes | yes |
| `notes_notetype_changed` | Change Note Type (U35, R948) | yes | yes |
| `preset_removed` | deleting a deck preset (U28; the deck options save, U49, U232) | yes | yes |
| `notetypes_merged` | an `.apkg` import that merges incoming note types into local ones (U121) | yes | yes |
| `collection_replaced` | a `.colpkg` import or a backup restore (U124, U127): set on the incoming collection before it replaces the current one | yes | no |
| `check_database_repair` | Check Database, only for the repairs U128 and U129 list as schema changes (Anki 26.09.3: cards whose last-review time was filled, cards without a note removed, filtered-deck fields cleared on cards outside a filtered deck, duplicated card ordinals, field-count mismatches, missing templates or recovered note types, invalid review-log properties, ids in the future) | yes | no |
| `sync_sanity_failed` | an incremental manual Anki transfer whose sanity check failed (U107, R1533; KTD26) | yes | no |
| `full_upload` | a one-way upload in a manual Anki transfer, which then sets `ls` to `scm` (U109, R1541; KTD26) | yes | no |
| `force_one_way` | the "Force a one-way Anki transfer" choice, made for one transfer inside the manual Anki Transfers workflow (U113/U160, R1547; KTD26). It is never a standing setting and never changes CloudKit | yes | no |
| `cloudkit_structure_applied` | the apply of an incoming CloudKit generation that makes one of the structural changes G1 lists (U114, R2053, R1477) | yes | no |

- A **structural edit** is an op with a reason marked "yes" in the last column: a user edit of structure. Each one takes a safety backup before its first write (`package/backup-triggers#T6.1`; `notetype_removed` has its own row there). The other reasons belong to ops that take their own backup (replace, Check Database, one-way upload), to manual Anki transfers, or to the CloudKit apply (U114; `package/backup-triggers` T6.1 has no row for it).
- Changed 2026-10-27: the last column was "Asks first when `scm <= ls`", the old confirmation gate (G4). The row `cloudkit_structure_applied` is new.

Not schema changes (R950), never setting `scm`: renaming a note type, field or template; field options and the Add screen's pin; template text; styling; deck override; browser appearance; adding a note type (stock, from scratch or by import without a merge); cloning; the on-demand Image Occlusion add; and every note, card, deck, tag and config change.

## G3. Decided by the effect

- An op is schema-classified by what it actually changes, not by how it was asked. A U34 save is `fields_changed` when the list of stored field positions differs: a field added or removed, or a kept field at another position. Moving a field to the position it already has, or setting the sort field to the current one, changes nothing (`notetypes/records` N3.1) and is not a schema change.
- A rename alone is never one, even in the same save as a structural change; the save as a whole is then schema-classified.
- Restore to Default is classified by `notetypes/restore-to-default#D5` instead: by its field and template counts and by a change of `kind`, since its by-position origins replace every field and template.
- A save that is schema-classified for several reasons carries all of them; any one is enough.
- Exception: an `.apkg` import's note type merge carries `notetypes_merged` alone, whatever it changes (`package/notetype-merge#J9`). An import's update of a note type without a merge still carries `sort_field_changed` (`package/apkg-import#I7`).
- The classification is part of the op's plan, so it is known before the write phase: the safety backup (`package/backup-triggers#T6.1`, T6.2) is taken from it, before the op's first write.

## G4. No confirmation (R951, R1476)

Changed 2026-10-27. This section replaces the earlier confirmation gate: the `schema_confirmed` input, the `NeedsSchemaConfirmation { reasons }` refusal and the R951 warning shown before the first schema change since a sync. The Product Contract no longer has that gate (R951, R1476; AE39).

- A schema-classified op commits without asking the user, whatever `scm` and `ls` hold. No op takes a confirmation input, and no op ends with `NeedsSchemaConfirmation`; that error no longer exists.
- Ordinary editing shows no Anki full-upload warning. A destructive mapping still has its own preview in its unit: fields, templates or cards that the edit removes (U34, U35), Restore to Default's question (R914), the note type delete question (R918), the decks a preset removal reassigns (R733), and the destructive-mapping preview of an `.apkg` import that merges note types (U121). Whether a preview is shown never depends on `scm` or `ls`.
- R918's and R948's "requires the schema-change confirmation" is superseded by R951 and R1476 (plan corrected 2026-10-27). R948's refusal when the schema changed since the screen opened stays: Change Note Type compares `scm` as a staleness token between its info op and its commit (U35) and asks nothing.
- The one-way consequence is shown only in the preview of the next manual Anki transfer, before anything is sent or replaced (R1476, KTD26). The engine answers `scm > ls`, read from the live `col` row, for that preview.
- Every structural edit (G2) takes its safety backup after its preview, if any, is accepted and before its first write (`package/backup-triggers#T6.1`).
- How Meguri differs from Anki here: G9.

Changed 2026-11-20 (round 9 option 3C; `notetypes/draft-save`). The Note Types section asks once, at Save, in one sheet, when content would be lost or when Meguri Sync is the service and the save is schema-classified (`notetypes/draft-save` D3, D4); with Meguri Sync the save runs as `sync/meguri-provider` MP7's job (round, save, upload). This replaces, for that screen and for Change Note Type and note type deletion, the first two bullets' "commits without asking" and "Ordinary editing shows no … warning", and the separate questions of R922, R932 and R933 (`notetypes/draft-save` D8). What stays: no op takes a confirmation input and none ends with `NeedsSchemaConfirmation` (the draft apply op's confirmed losses, `notetypes/structure-changes` S1.2, are a precondition that refuses with `LossesChanged`, not a confirmation); the question is the app's, asked before the op is submitted; whether it is asked never depends on `scm` or `ls`; the safety backup follows the accepted question.

## G5. Setting `scm`

- Every schema-classified op that commits sets `scm` to `now_ms` inside its own transaction, also when `scm > ls` already (Anki sets it again on each such change; scenario `notetypes/gate/structure`).
- The write is part of the op: it moves `col.mod` with the op (`storage/ids-usn-mtime` S5), it is in the op's change set (`collection_row`, `engine/change-stream` S1), and it is in the op's undo step: undo sets `scm` back to the value it had before the op, redo sets it to the value the op set (Anki does the same).
- `collection_replaced` sets it on the incoming collection (U124, U127); `full_upload` sets it and then `ls` to the same value (U109).

## G6. The storage guard (R1477)

A safety net under G2–G5: a note type whose field or template count changed must never reach an incremental manual Anki transfer (a normal sync, KTD26) with `scm <= ls`.

- At the commit of every write transaction on an open collection, for every note type that exists both before and after the transaction and whose number of `fields` rows or `templates` rows differs between the two, the commit proceeds only if `scm > ls` holds in the state the transaction leaves. Otherwise the transaction rolls back and the op fails with `SchemaNotMarked { notetype_id }`, an internal error (a bug in the op): nothing is written.
- Note types added or removed in the transaction do not count (adding is not a schema change; removing is `notetype_removed`, which sets `scm`).
- The one exception: the undo of a step that set `scm` and changed the counts, which restores `scm` to the step's before-image together with the counts. That leaves `scm <= ls` and the counts as they were at the last manual Anki transfer, which an incremental manual Anki transfer can send.
- Redo gets no exception: redoing such a step sets `scm` to the value the step set, which passes while no manual Anki transfer happened in between. If a manual Anki transfer (which moves `ls`) ran between the undo and the redo, the redo would leave `scm <= ls` with changed counts, and the guard refuses it; U25 drops redo entries such a transfer makes unsafe (KTD7), so this case is a bug report, not a user-visible flow.
- A CloudKit exchange does not count in the two bullets above: it never moves `ls` (G1).
- Collection replacement (one-way download, `.colpkg` import, backup restore) is not a write transaction on the open collection and is not guarded.
- Anki has no such guard; its own ops set `scm` as G2 says, and its sync refuses a note type whose counts changed (U107, R1525).

## G7. Transfers and CloudKit (summary)

- **Manual Anki transfers** (KTD26; owned by U106, U107 and U109, frozen with the Anki server work). An incremental transfer is possible only while `scm <= ls`. With `scm > ls` the status check reports a one-way transfer, and the transfer preview shows it before anything is sent or replaced (R1476). An incoming note type whose field or template count differs from the local one stops an incremental transfer (R1525). A sanity-check failure sets `scm` (`sync_sanity_failed`).
- **CloudKit** (U113, U114). A schema change never makes routine sync one-way and never asks. Each committed schema change reaches CloudKit as a validated coherent generation (R2053; `sync/cloudkit-records#K19`). K19.1 lists the ops that publish as one generation and their reason codes; it also includes every package import (`apkg_apply`), schema change or not, and leaves out the reasons with no CloudKit meaning (`collection_replaced`, `full_upload`, `sync_sanity_failed`, `force_one_way`). An op that changes a note type's fields or templates, removes a note type or moves notes to another note type reports its reason codes and its field and template maps when it commits, through the structure report of `engine/change-stream#S2.5`, and a commit that makes such a change without one is refused (`StructureNotReported`, S2.5's guard; K19.2; U34, U35, U121, U128). The apply of an incoming generation sets `scm` as G1 says (`cloudkit_structure_applied`).

The precise normal-transfer aggregate `layouts` exception and its CloudKit authority/recovery safeguards are `engine/change-stream#S2.5` “Normal-transfer aggregates”. It exempts no `removed` entry or `type_changes` pair and changes no schema-stamp or transfer-acceptance rule.

## G8. Scenarios

Scenario ops that are schema changes run as the user accepted them: the oracle never asks, Meguri has nothing to ask (G4), and `scm` is observed with `db_exec` (`specs/conformance/notetype-ops.md` T9).

| Scenario | Covers |
|---|---|
| `notetypes/gate/after-sync` | AE39: after a sync (`ls` = `scm`), a field rename leaves `scm`; a field add sets it; a template reorder sets it again |
| `notetypes/gate/structure` | G2, G3, G5: each structural change of U34 sets `scm`, again when already modified; a field rename and a template text change do not; a move to the same position does not |
| `notetypes/records/stamps`, `notetypes/records/names`, `notetypes/records/clone` | the non-schema ops of G2 leave `scm` |

Both gate scenarios have their goldens. On Meguri they need `setup.notes` (U27), the `card` observation and U34's `update_notetype` edits (`specs/conformance/notetype-ops.md` T10.1), so they are `pending` until U34, which makes them active.

Meguri's own rules are tested in `core/crates/meguri-engine/tests/schema_gate.rs`, with test-only ops until U34's ops exist: G4 (with `scm <= ls`, a schema op commits at once, sets `scm`, and takes no confirmation input; no op returns `NeedsSchemaConfirmation`) and the guard (a non-schema op that would change a field count while `scm <= ls` is refused; undo and redo of a template add both commit). The safety backup of each structural edit is tested by its unit (`package/backup-triggers` T10). U114's tests cover `cloudkit_structure_applied`: an applied generation that adds a field sets `scm` and passes the guard; one that only renames a field leaves `scm`.

## G9. Differences from Anki (R1889)

R1889 references: `conformance/dsl#D13`; `conformance/deviations.yaml` header (link requested). This is a production-only register: each row names the test that asserts Meguri's behavior. Tests marked "to add" are required before the owning unit is accepted.

| Situation | Anki 26.09.3 | Meguri | Test |
|---|---|---|---|
| A schema change while `scm <= ls` (the first one since a sync) | the desktop warns that the next sync must upload the whole collection and that unsynced changes on other devices will be lost; the change runs only when the user accepts | the change commits without a question (G4); the next manual Anki transfer's preview shows the one-way consequence (R1476) | `core/crates/meguri-engine/tests/schema_gate.rs` `a_schema_op_commits_at_once_while_scm_is_at_ls` (U29: commits, sets `scm`, takes no confirmation input); for preset removal, `core/crates/meguri-engine/tests/deck_options.rs` `removing_a_preset_with_scm_at_ls_commits_without_confirmation` (U49, the deck options save) and `core/crates/meguri-engine/tests/deck_config.rs` `removal_commits_without_confirmation_and_undo_restores_schema_and_rows` (the preset operation, `decks/deck-presets#P13`) |
| A user says no to that warning | nothing changes | not applicable: no op takes a confirmation input or returns `NeedsSchemaConfirmation` | `schema_gate.rs` `no_op_takes_a_schema_confirmation_input` (U29) |
| A safety backup before a structural edit | none | one per structural edit (`package/backup-triggers#T6.1`, R2008) | `core/crates/meguri-engine/tests/maintenance_scheduler.rs` safety-class tests (`package/backup-triggers` T10), and each structural unit's ordering test (U28, U34, U35, U49, U120, U121); U120's is the T10 ordering test for an import that changes a sort field (`package/apkg-import` I18.3); U35's is `core/crates/meguri-engine/tests/change_notetype.rs` (no confirmation input, the safety backup's ordering and coalescing; `notetypes/change-notetype`) |

This table authorizes no scenario transform. The oracle runs schema ops as accepted (`conformance/notetype-ops` T9, `conformance/deck-ops` K9), so no golden holds the question, and `conformance/deviations.yaml` has no entry for it: its entries are golden transforms (`conformance/dsl#D13`).
