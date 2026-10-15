# Schema changes and the confirmation gate

Spec: notetypes/schema-changes
Requirements: R949, R950, R951, R1475, R1476, R1477, R1590 (core side), R918, R948 (confirmation)
Anki: 26.09.3
Scenarios: conformance/scenarios/notetypes/gate/

Which operations mark the collection's schema as modified (forcing the next sync to be one-way), how Meguri asks before the first one since a sync, and the guard that keeps a changed note type structure out of a normal sync. The UI prompt is the caller's (U200, U201, U203, U210, U232); the sync side is U107 and U109. Columns are `model/row-records` R2 and `storage/ids-usn-mtime` S6.

## G1. The schema stamp

- `col.scm` is the instant of the last schema change and `col.ls` the instant of the last sync, both epoch milliseconds.
- The schema is **modified since the last sync** when `scm > ls`. A sync then has to be one-way (U106, U109). A collection never synced has `ls` 0, so its schema always counts as modified and no change asks for confirmation.
- After a one-way upload, `ls` equals `scm` (U109); after a normal sync, `ls` is the server's time, later than `scm`. Either way the schema is no longer modified: the next schema change asks again (R951).

## G2. Classification (R949, R950, R1475)

Each op that can change the schema declares a reason. The table is the whole list: no other op ever sets `scm` (R1590).

| Reason | Op (owner) | Sets `scm` | Asks first when `scm <= ls` |
|---|---|---|---|
| `fields_changed` | a note type save whose fields are added, removed or reordered (U34) | yes | yes |
| `sort_field_changed` | a note type save that makes another field the sort field (U34, R926) | yes | yes |
| `templates_changed` | a note type save whose templates are added, removed or reordered (U34) | yes | yes |
| `notetype_removed` | deleting a note type (U34, R918) | yes | yes |
| `notetype_restored` | Restore to Default when it changes the field or template structure (U34, R914) | yes | yes |
| `notes_notetype_changed` | Change Note Type (U35, R948) | yes | yes |
| `preset_removed` | deleting a deck preset (U28, U232) | yes | yes |
| `notetypes_merged` | an `.apkg` import that merges incoming note types into local ones (U121) | yes | yes |
| `collection_replaced` | a `.colpkg` import or a backup restore (U124, U127): set on the incoming collection before it replaces the current one | yes | no: the replace confirmation is the caller's |
| `check_database_repair` | Check Database, only for the repairs U128 and U129 list as schema changes (Anki 26.09.3: cards whose last-review time was filled, cards without a note removed, filtered-deck fields cleared on cards outside a filtered deck, duplicated card ordinals, field-count mismatches, missing templates or recovered note types, invalid review-log properties, ids in the future) | yes | no |
| `sync_sanity_failed` | a normal sync whose sanity check failed (U107, R1533) | yes | no |
| `full_upload` | a one-way upload, which then sets `ls` to `scm` (U109, R1541) | yes | no |
| `force_one_way` | the "Force a one-way sync on next sync" setting (U113/U160, R1547) | yes | no: the setting is the user's request |

Not schema changes (R950), never setting `scm`: renaming a note type, field or template; field options and the Add screen's pin; template text; styling; deck override; browser appearance; adding a note type (stock, from scratch or by import without a merge); cloning; the on-demand Image Occlusion add; and every note, card, deck, tag and config change.

## G3. Decided by the effect

- An op is schema-classified by what it actually changes, not by how it was asked. A U34 save is `fields_changed` when the list of stored field positions differs: a field added or removed, or a kept field at another position. Moving a field to the position it already has, or setting the sort field to the current one, changes nothing (`notetypes/records` N3.1) and is not a schema change.
- A rename alone is never one, even in the same save as a structural change; the save as a whole is then schema-classified.
- A save that is schema-classified for several reasons carries all of them (the gate reports one; any is enough).

## G4. The gate (R951, R1476)

- Each op whose reason G2 marks "asks first" carries `schema_confirmed`: true only when the caller asked the user and the user accepted. Ops whose reason does not ask never consult the gate.
- The check runs inside the op's write transaction, on the live `col` row, after the op has computed its effect and before it writes anything: when `scm <= ls` and the op needs confirmation (G2) and `schema_confirmed` is false, the op ends with `NeedsSchemaConfirmation { reasons }`, writes nothing, takes no undo step and publishes nothing.
- The caller shows the warning (R951: the next sync must upload the whole collection, unsynced changes on other devices will be lost) and, on acceptance, submits the same op again with `schema_confirmed` true. On refusal nothing changes (R1476).
- Once a schema change has committed, `scm > ls`, so later schema ops until the next sync go through without asking (R951; AE39).
- The engine answers, for the UI, whether a schema change now would ask: `scm <= ls` read from the live `col` row. The caller may ask before building a long draft, but the op's own check is the one that counts (a sync can finish in between).
- Spec-flow Q9's offer to sync first when online is not adopted (plan, Open Questions).

## G5. Setting `scm`

- Every schema-classified op that commits sets `scm` to `now_ms` inside its own transaction: confirmed or not, and also when `scm > ls` already (Anki sets it again on each such change; scenario `notetypes/gate/structure`).
- The write is part of the op: it moves `col.mod` with the op (`storage/ids-usn-mtime` S5), it is in the op's change set (`collection_row`, `engine/change-stream` S1), and it is in the op's undo step: undo sets `scm` back to the value it had before the op, redo sets it to the value the op set (Anki does the same).
- `collection_replaced` sets it on the incoming collection (U124, U127); `full_upload` sets it and then `ls` to the same value (U109).

## G6. The storage guard (R1477)

A safety net under G2–G5: a note type whose field or template count changed must never reach a normal sync with `scm <= ls`.

- At the commit of every write transaction on an open collection, for every note type that exists both before and after the transaction and whose number of `fields` rows or `templates` rows differs between the two, the commit proceeds only if `scm > ls` holds in the state the transaction leaves. Otherwise the transaction rolls back and the op fails with `SchemaNotMarked { notetype_id }`, an internal error (a bug in the op): nothing is written.
- Note types added or removed in the transaction do not count (adding is not a schema change; removing is `notetype_removed`, which sets `scm`).
- The one exception: the undo of a step that set `scm` and changed the counts, which restores `scm` to the step's before-image together with the counts. That leaves `scm <= ls` and the counts as they were at the last sync, which a normal sync can send.
- Redo gets no exception: redoing such a step sets `scm` to the value the step set, which passes while no sync happened in between. If a sync ran between the undo and the redo, the redo would leave `scm <= ls` with changed counts, and the guard refuses it; U25 drops redo entries a sync makes unsafe (KTD7), so this case is a bug report, not a user-visible flow.
- Collection replacement (one-way download, `.colpkg` import, backup restore) is not a write transaction on the open collection and is not guarded.
- Anki has no such guard; its own ops set `scm` as G2 says, and its sync refuses a note type whose counts changed (U107, R1525).

## G7. Sync interplay (summary, owned by U106, U107, U109)

- A normal sync is possible only while `scm <= ls`; with `scm > ls` the status check reports a one-way sync.
- An incoming note type whose field or template count differs from the local one stops a normal sync (R1525).
- A sanity-check failure sets `scm` (`sync_sanity_failed`).

## G8. Scenarios

Scenario ops that are schema changes run confirmed (`specs/conformance/notetype-ops.md` T9): the oracle never asks, and `scm` is observed with `db_exec`.

| Scenario | Covers |
|---|---|
| `notetypes/gate/after-sync` | AE39: after a sync (`ls` = `scm`), a field rename leaves `scm`; a field add sets it; a template reorder sets it again |
| `notetypes/gate/structure` | G2, G3, G5: each structural change of U34 sets `scm`, again when already modified; a field rename and a template text change do not; a move to the same position does not |
| `notetypes/records/stamps`, `notetypes/records/names`, `notetypes/records/clone` | the non-schema ops of G2 leave `scm` |

The gate itself (`NeedsSchemaConfirmation`, nothing written, the confirmed resubmission, no second request) and the guard (a non-schema op that would change a field count while `scm <= ls` is refused; undo and redo of a template add both commit) are Meguri's alone and tested in `core/crates/meguri-engine/tests/schema_gate.rs`, with test-only ops until U34's ops exist.
