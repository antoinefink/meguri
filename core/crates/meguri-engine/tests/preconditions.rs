//! Preconditions and the write-phase rule (`engine/op-model` O6, O7; R727
//! mechanism, AE27 core half): a write phase never writes a value computed on
//! a snapshot over a row that changed since.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use common::{
    Harness, MID, add_note_with_card, add_notes, col_mod, note, note_fields, query_int, revlog, set_first_field,
};
use meguri_engine::{EngineError, OpSpec, Plan, Precondition, plan_and_commit};
use meguri_model::changes::OpSource;
use meguri_model::ids::NoteId;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

#[test]
fn an_answer_planned_on_a_card_a_sync_changed_first_is_stale_and_writes_nothing() {
    let h = Harness::open();
    let (_, cid) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["q", "a"]).value;
    let key = RowKey::Id(cid.0);
    // The state token the answer buttons were computed from.
    let token = h
        .collection
        .reader(ReaderRole::Interactive)
        .unwrap()
        .row_digest(Table::Cards, &key)
        .unwrap();
    assert!(token.is_some());

    // A sync applies a change to that card from another device.
    h.clock.step_ms(10_000);
    h.collection
        .run(OpSpec::named("sync_merge", OpSource::SyncIncoming), move |ctx| {
            let mut incoming = ctx.scope().card(cid)?.unwrap();
            incoming.due = 77;
            incoming.usn = Usn(12);
            incoming.mtime = EpochSecs(1_894_700_000);
            ctx.scope().put(&incoming)?;
            Ok(())
        })
        .unwrap();
    let before_answer = col_mod(&h.collection);
    let revlog_rows = query_int(&h.collection, "SELECT count(*) FROM revlog");
    let card_row = query_int(&h.collection, &format!("SELECT due FROM cards WHERE id = {}", cid.0));

    h.clock.step_ms(10_000);
    let answer = |token| {
        h.collection.run(
            OpSpec::named("answer", OpSource::UserUndoable).precondition(Precondition::RowDigest {
                table: Table::Cards,
                key: RowKey::Id(cid.0),
                digest: token,
            }),
            move |ctx| {
                let mut answered = ctx.scope().card(cid)?.unwrap();
                answered.due = 1000;
                ctx.scope().put(&answered)?;
                ctx.scope().add_revlog(&revlog(cid))?;
                Ok(())
            },
        )
    };
    let stale = answer(token).unwrap_err();
    assert!(
        matches!(
            stale,
            EngineError::Stale {
                table: Table::Cards,
                ..
            }
        ),
        "{stale:?}"
    );
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM revlog"), revlog_rows);
    assert_eq!(
        query_int(&h.collection, &format!("SELECT due FROM cards WHERE id = {}", cid.0)),
        card_row
    );
    assert_eq!(col_mod(&h.collection), before_answer, "col.mod unchanged");

    // The fresh state answers.
    let fresh = h
        .collection
        .reader(ReaderRole::Interactive)
        .unwrap()
        .row_digest(Table::Cards, &RowKey::Id(cid.0))
        .unwrap();
    answer(fresh).unwrap();
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM revlog"), revlog_rows + 1);
}

fn first_field(fields: &str) -> &str {
    fields.split('\u{1f}').next().unwrap_or_default()
}

/// Upper-cases the first field of every note listed, as planned.
fn rewrite(ctx: &mut meguri_engine::OpContext<'_, '_>, writes: Vec<(NoteId, String)>) -> Result<(), EngineError> {
    for (id, first) in writes {
        let mut stored = ctx.scope().note(id)?.unwrap();
        let rest: Vec<&str> = stored.fields.split('\u{1f}').skip(1).collect();
        stored.fields = std::iter::once(first.as_str())
            .chain(rest)
            .collect::<Vec<_>>()
            .join("\u{1f}");
        ctx.scope().put(&stored)?;
    }
    Ok(())
}

#[test]
fn an_edit_committed_between_plan_and_write_is_planned_again_never_overwritten() {
    let h = Harness::open();
    let ids = add_notes(&h.collection, 20, "n");
    let target = ids[3];
    let plans = AtomicU32::new(0);
    let collection = h.collection.clone();
    let result = plan_and_commit(
        &h.collection,
        ReaderRole::Maintenance,
        OpSpec::named("uppercase_fields", OpSource::UserNonUndoable),
        |plan| {
            plans.fetch_add(1, Ordering::SeqCst);
            let notes = plan.snapshot().notes_after(None, 1_000)?;
            if plan.attempt() == 0 {
                // A server edit commits after the snapshot, before the write.
                set_first_field(&collection, OpSource::LocalApi, target, "edited by api")?;
            }
            Ok(Plan {
                preconditions: notes
                    .iter()
                    .map(|n| Precondition::Untouched {
                        table: Table::Notes,
                        key: RowKey::Id(n.id.0),
                        since: plan.seq(),
                    })
                    .collect(),
                value: notes
                    .iter()
                    .map(|n| (n.id, first_field(&n.fields).to_uppercase()))
                    .collect(),
            })
        },
        rewrite,
    )
    .unwrap();
    assert_eq!(plans.load(Ordering::SeqCst), 2, "planned again after Stale");
    assert!(result.timing.plan.is_some());
    assert_eq!(first_field(&note_fields(&h.collection, target)), "EDITED BY API");
    assert_eq!(first_field(&note_fields(&h.collection, ids[0])), "N0");
}

#[test]
fn a_write_phase_that_re_derives_from_the_live_row_keeps_the_edit() {
    let h = Harness::open();
    let ids = add_notes(&h.collection, 5, "n");
    let target = ids[2];
    let collection = h.collection.clone();
    plan_and_commit(
        &h.collection,
        ReaderRole::Maintenance,
        OpSpec::named("uppercase_fields", OpSource::UserNonUndoable),
        |plan| {
            let notes = plan.snapshot().notes_after(None, 1_000)?;
            set_first_field(&collection, OpSource::LocalApi, target, "live edit")?;
            Ok(Plan {
                value: notes.iter().map(|n| n.id).collect::<Vec<_>>(),
                preconditions: Vec::new(),
            })
        },
        |ctx, ids: Vec<NoteId>| {
            // Re-derived inside the write transaction from the live row.
            let writes = ids
                .iter()
                .map(|id| Ok((*id, first_field(&ctx.scope().note(*id)?.unwrap().fields).to_uppercase())))
                .collect::<Result<Vec<_>, EngineError>>()?;
            rewrite(ctx, writes)
        },
    )
    .unwrap();
    assert_eq!(first_field(&note_fields(&h.collection, target)), "LIVE EDIT");
}

#[test]
fn a_same_second_edit_after_the_snapshot_is_caught_although_mtime_and_usn_match() {
    let h = Harness::open();
    let ids = add_notes(&h.collection, 3, "n");
    let target = ids[1];
    // The clock stays frozen from here: every edit lands in one second.
    set_first_field(&h.collection, OpSource::LocalApi, target, "first").unwrap();
    let after_first = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .note(target)
        .unwrap()
        .unwrap();
    let collection = h.collection.clone();
    let mut after_second = None;
    plan_and_commit(
        &h.collection,
        ReaderRole::Maintenance,
        OpSpec::named("uppercase_fields", OpSource::UserNonUndoable),
        |plan| {
            let notes = plan.snapshot().notes_after(None, 1_000)?;
            if plan.attempt() == 0 {
                set_first_field(&collection, OpSource::LocalApi, target, "second")?;
                after_second = collection.reader(ReaderRole::Query)?.note(target)?;
            }
            Ok(Plan {
                preconditions: notes
                    .iter()
                    .map(|n| Precondition::Untouched {
                        table: Table::Notes,
                        key: RowKey::Id(n.id.0),
                        since: plan.seq(),
                    })
                    .collect(),
                value: notes
                    .iter()
                    .map(|n| (n.id, first_field(&n.fields).to_uppercase()))
                    .collect(),
            })
        },
        rewrite,
    )
    .unwrap();
    let after_second = after_second.unwrap();
    assert_eq!(
        (after_second.mtime, after_second.usn),
        (after_first.mtime, after_first.usn),
        "the stamps cannot tell the two edits apart"
    );
    assert_eq!(first_field(&note_fields(&h.collection, target)), "SECOND");
}

#[test]
fn a_note_added_to_a_predicate_targeted_set_between_plan_and_write_is_covered() {
    let h = Harness::open();
    add_notes(&h.collection, 4, "n");
    let collection = h.collection.clone();
    let mut added = None;
    let result = plan_and_commit(
        &h.collection,
        ReaderRole::Maintenance,
        OpSpec::named("uppercase_notetype", OpSource::UserNonUndoable),
        |plan| {
            // The target set: every note of note type MID.
            let notes: Vec<_> = plan
                .snapshot()
                .notes_after(None, 1_000)?
                .into_iter()
                .filter(|n| n.notetype_id == MID)
                .collect();
            if plan.attempt() == 0 {
                let new = collection.run(OpSpec::named("addNote", OpSource::LocalApi), |ctx| {
                    Ok(ctx.scope().add_note(&note(&["late", "x"]))?)
                })?;
                added = Some(new.value);
            }
            Ok(Plan {
                preconditions: vec![Precondition::TableUntouched {
                    table: Table::Notes,
                    since: plan.seq(),
                }],
                value: notes
                    .iter()
                    .map(|n| (n.id, first_field(&n.fields).to_uppercase()))
                    .collect(),
            })
        },
        rewrite,
    )
    .unwrap();
    let added = added.unwrap();
    assert_eq!(first_field(&note_fields(&h.collection, added)), "LATE");
    assert!(result.changes.note(added).is_some());
}

#[test]
fn an_op_that_stays_stale_gives_up_after_three_more_plans() {
    let h = Harness::open();
    let (nid, _) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["x", "y"]).value;
    let plans = AtomicU32::new(0);
    let error = plan_and_commit(
        &h.collection,
        ReaderRole::Maintenance,
        OpSpec::named("never", OpSource::UserNonUndoable),
        |_| {
            plans.fetch_add(1, Ordering::SeqCst);
            Ok(Plan {
                value: (),
                preconditions: vec![Precondition::RowDigest {
                    table: Table::Notes,
                    key: RowKey::Id(nid.0),
                    digest: Some([0; 32]),
                }],
            })
        },
        |_, ()| Ok(()),
    )
    .unwrap_err();
    assert!(matches!(error, EngineError::Stale { .. }));
    assert_eq!(plans.load(Ordering::SeqCst), 4);
}

#[test]
fn untouched_and_absent_row_preconditions_hold_when_nothing_changed() {
    let h = Harness::open();
    let (nid, _) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["x", "y"]).value;
    let since = h.collection.last_seq();
    add_note_with_card(&h.collection, OpSource::UserUndoable, &["other", "y"]);
    h.collection
        .run(
            OpSpec::named("guarded", OpSource::UserUndoable)
                .precondition(Precondition::Untouched {
                    table: Table::Notes,
                    key: RowKey::Id(nid.0),
                    since,
                })
                .precondition(Precondition::RowDigest {
                    table: Table::Notes,
                    key: RowKey::Id(nid.0 + 999_999),
                    digest: None,
                })
                .precondition(Precondition::TableUntouched {
                    table: Table::Decks,
                    since,
                }),
            |_| Ok(()),
        )
        .unwrap();
    let stale = h
        .collection
        .run(
            OpSpec::named("guarded", OpSource::UserUndoable).precondition(Precondition::TableUntouched {
                table: Table::Notes,
                since,
            }),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(matches!(
        stale,
        EngineError::Stale {
            table: Table::Notes,
            ..
        }
    ));
}
