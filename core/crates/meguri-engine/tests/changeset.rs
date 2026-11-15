//! The change set derived from an op's journal (`engine/change-stream` S1).

mod common;

use common::{Harness, add_note_with_card, card, note, revlog};
use meguri_engine::OpSpec;
use meguri_model::card_data::CardData;
use meguri_model::changes::{ChangeKind, OpSource};
use meguri_model::deck::Deck;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_model::tag::Tag;
use meguri_storage::journal::{LayoutReport as AuthoredLayoutReport, StructureReport as AuthoredStructureReport};
use meguri_storage::{ConfigValue, ReadRows};

/// Adds a note whose card has an FSRS memory state.
fn reviewed_card(h: &Harness) -> (NoteId, CardId) {
    h.collection
        .run(OpSpec::named("add", OpSource::UserUndoable), |ctx| {
            let nid = ctx.scope().add_note(&note(&["memory", "x"]))?;
            let mut new = card(nid, 0);
            new.data = CardData::parse(r#"{"s":1.5,"d":5.0,"dr":0.9,"zz":1}"#);
            new.due = 100;
            let cid = ctx.scope().add_card(&new)?;
            Ok((nid, cid))
        })
        .unwrap()
        .value
}

/// An op that changes the card with `edit`.
fn edit_card(
    h: &Harness,
    cid: CardId,
    edit: impl FnOnce(&mut meguri_model::card::Card) + Send + 'static,
) -> meguri_engine::OpResult<()> {
    h.clock.step_ms(60_000);
    h.collection
        .run(OpSpec::named("edit_card", OpSource::UserUndoable), move |ctx| {
            let mut stored = ctx.scope().card(cid)?.unwrap();
            edit(&mut stored);
            ctx.scope().put(&stored)?;
            Ok(())
        })
        .unwrap()
}

#[test]
fn a_change_of_only_fsrs_values_in_data_is_memory_state_only_and_carries_the_note() {
    let h = Harness::open();
    let (nid, cid) = reviewed_card(&h);
    let result = edit_card(&h, cid, |card| {
        card.data = CardData::parse(r#"{"s":2.25,"d":4.5,"dr":0.85,"decay":0.2,"lrt":1894701600,"zz":1}"#);
    });
    let change = result.changes.card(cid).copied().expect("the card is listed");
    assert_eq!(change.note_id, nid);
    assert_eq!(change.kind, ChangeKind::Updated);
    assert!(change.memory_state_only);
    assert!(!change.scheduling_relevant());
}

#[test]
fn a_due_change_is_scheduling_relevant_and_carries_the_note() {
    let h = Harness::open();
    let (nid, cid) = reviewed_card(&h);
    let result = edit_card(&h, cid, |card| card.due = 101);
    let change = result.changes.card(cid).copied().unwrap();
    assert_eq!(change.note_id, nid);
    assert!(change.scheduling_relevant());
}

#[test]
fn other_data_keys_and_other_columns_are_scheduling_relevant() {
    let h = Harness::open();
    let (_, cid) = reviewed_card(&h);
    let unknown_key = edit_card(&h, cid, |card| {
        card.data = CardData::parse(r#"{"s":1.5,"d":5.0,"dr":0.9,"zz":2}"#);
    });
    assert!(unknown_key.changes.card(cid).unwrap().scheduling_relevant());
    let position = edit_card(&h, cid, |card| {
        card.data = CardData::parse(r#"{"pos":3,"s":1.5,"d":5.0,"dr":0.9,"zz":2}"#);
    });
    assert!(position.changes.card(cid).unwrap().scheduling_relevant());
    let flags = edit_card(&h, cid, |card| card.flags = 2);
    assert!(flags.changes.card(cid).unwrap().scheduling_relevant());
}

#[test]
fn added_and_removed_rows_list_their_kinds_and_review_log_ids() {
    let h = Harness::open();
    let added = h
        .collection
        .run(OpSpec::named("answer", OpSource::UserUndoable), |ctx| {
            let nid = ctx.scope().add_note(&note(&["n", "x"]))?;
            let cid = ctx.scope().add_card(&card(nid, 0))?;
            let rid = ctx.scope().add_revlog(&revlog(cid))?;
            Ok((nid, cid, rid))
        })
        .unwrap();
    let (nid, cid, rid) = added.value;
    assert_eq!(added.changes.source, OpSource::UserUndoable);
    assert_eq!(added.changes.note(nid).map(|n| n.kind), Some(ChangeKind::Added));
    let card_change = added.changes.card(cid).copied().unwrap();
    assert_eq!((card_change.kind, card_change.note_id), (ChangeKind::Added, nid));
    assert_eq!(added.changes.revlog_added, [rid]);
    assert!(added.changes.revlog_removed.is_empty());
    assert!(
        !added.changes.collection_row,
        "the commit's mod stamp is not a col change"
    );

    h.clock.step_ms(1_000);
    let removed = h
        .collection
        .run(OpSpec::named("forget", OpSource::UserUndoable), move |ctx| {
            ctx.scope().remove_revlog(rid)?;
            ctx.scope().remove_note_and_cards(nid)?;
            Ok(())
        })
        .unwrap();
    let card_change = removed.changes.card(cid).copied().unwrap();
    assert_eq!((card_change.kind, card_change.note_id), (ChangeKind::Removed, nid));
    assert_eq!(removed.changes.note(nid).map(|n| n.kind), Some(ChangeKind::Removed));
    assert_eq!(removed.changes.revlog_removed, [rid]);
    assert!(removed.changes.revlog_added.is_empty());
}

#[test]
fn a_row_created_and_deleted_in_one_op_is_not_listed() {
    let h = Harness::open();
    let result = h
        .collection
        .run(OpSpec::named("churn", OpSource::UserUndoable), |ctx| {
            let gone = ctx.scope().add_note(&note(&["gone", "x"]))?;
            ctx.scope().remove_note(gone)?;
            let kept = ctx.scope().add_note(&note(&["kept", "x"]))?;
            Ok(kept)
        })
        .unwrap();
    let ids: Vec<NoteId> = result.changes.notes.iter().map(|n| n.id).collect();
    assert_eq!(ids, [result.value]);
}

#[test]
fn decks_presets_config_keys_tags_and_media_are_listed_sorted_once() {
    let h = Harness::open();
    let result = h
        .collection
        .run(OpSpec::named("misc", OpSource::UserNonUndoable), |ctx| {
            let preset = ctx.scope().deck_config(meguri_model::ids::DeckConfigId(1))?.unwrap();
            let preset_id = ctx.scope().add_deck_config(&preset)?;
            let mut deck: Deck = ctx.scope().deck(DeckId(1))?.unwrap();
            deck.name = "Second".to_owned();
            let deck_id = ctx.scope().add_deck(&deck)?;
            ctx.scope().set_config("zKey", &ConfigValue::Integer(2))?;
            ctx.scope().set_config("aKey", &ConfigValue::String("x".into()))?;
            ctx.scope().put(&Tag {
                name: "Vocab::Verbs".to_owned(),
                ..Tag::default()
            })?;
            ctx.media_added("b.jpg");
            ctx.media_added("a.jpg");
            ctx.media_added("a.jpg");
            ctx.media_removed("old.mp3");
            Ok((deck_id, preset_id))
        })
        .unwrap();
    let (deck_id, preset_id) = result.value;
    let changes = &result.changes;
    assert_eq!(
        changes.decks.iter().map(|d| (d.id, d.kind)).collect::<Vec<_>>(),
        [(deck_id, ChangeKind::Added)]
    );
    assert_eq!(
        changes.deck_configs.iter().map(|d| (d.id, d.kind)).collect::<Vec<_>>(),
        [(preset_id, ChangeKind::Added)]
    );
    let keys: Vec<&str> = changes.config_keys.iter().map(|k| k.key.as_str()).collect();
    assert_eq!(keys, ["aKey", "zKey"]);
    assert_eq!(changes.tags.len(), 1);
    assert_eq!(changes.tags[0].name, "Vocab::Verbs");
    assert_eq!(changes.media_added, ["a.jpg", "b.jpg"]);
    assert_eq!(changes.media_removed, ["old.mp3"]);
}

#[test]
fn note_type_changes_flag_schema_changes_but_not_template_text() {
    let h = Harness::open();
    let ntid = NotetypeId(common::query_int(
        &h.collection,
        "SELECT id FROM notetypes ORDER BY id LIMIT 1",
    ));

    h.clock.step_ms(1_000);
    let template_text = h
        .collection
        .run(OpSpec::named("edit_template", OpSource::UserNonUndoable), move |ctx| {
            let mut notetype = ctx.scope().notetype(ntid)?.unwrap();
            notetype.templates[0].config.q_format.push_str("<hr>");
            ctx.scope().put_notetype(&notetype)?;
            Ok(())
        })
        .unwrap();
    let change = template_text.changes.notetypes[0];
    assert_eq!(
        (change.id, change.kind, change.schema_changed),
        (ntid, ChangeKind::Updated, false)
    );

    h.clock.step_ms(1_000);
    let renamed = h
        .collection
        .run(OpSpec::named("rename_field", OpSource::UserNonUndoable), move |ctx| {
            let mut notetype = ctx.scope().notetype(ntid)?.unwrap();
            let mut report = AuthoredStructureReport::default();
            // This fixture renames one field without moving any ordered position.
            report.layouts.insert(ntid.0, AuthoredLayoutReport::identity(&notetype)?);
            let token = ctx.scope().start_structure_report(report)?;
            notetype.fields[0].name = "Renamed".to_owned();
            ctx.scope().put_notetype(&notetype)?;
            ctx.scope().complete_structure_report(token)?;
            Ok(())
        })
        .unwrap();
    let change = renamed.changes.notetypes[0];
    assert_eq!((change.kind, change.schema_changed), (ChangeKind::Updated, true));

    h.clock.step_ms(1_000);
    let added = h
        .collection
        .run(OpSpec::named("add_notetype", OpSource::UserNonUndoable), move |ctx| {
            let mut copy = ctx.scope().notetype(ntid)?.unwrap();
            copy.name = "Copy".to_owned();
            Ok(ctx.scope().add_notetype(&copy)?)
        })
        .unwrap();
    let change = added.changes.notetypes[0];
    assert_eq!(
        (change.id, change.kind, change.schema_changed),
        (added.value, ChangeKind::Added, true)
    );
}

#[test]
fn a_change_to_the_col_row_beyond_its_mod_stamp_is_flagged() {
    let h = Harness::open();
    let ordinary = add_note_with_card(&h.collection, OpSource::UserUndoable, &["a", "b"]);
    assert!(!ordinary.changes.collection_row);
    h.clock.step_ms(1_000);
    let schema = h
        .collection
        .run(OpSpec::named("schema_change", OpSource::UserNonUndoable), |ctx| {
            let mut col = ctx.scope().col()?;
            col.schema_modified = ctx.now();
            ctx.scope().put_col(&col)?;
            Ok(())
        })
        .unwrap();
    assert!(schema.changes.collection_row);
    assert!(!schema.changes.is_empty());
}
