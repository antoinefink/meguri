//! Preset-only behavior (deck-presets P3, P7, P8, P10, P13).
mod common;
use common::{NOW_MS, col_mod, query_int};
use meguri_engine::{EngineError, OpSource, OpSpec, SchemaReason, UndoScope, deck_config, decks};
use meguri_model::{
    ids::{DeckConfigId, DeckId},
    proto::ProtoBlob,
};
use meguri_storage::{ReadRows, ReaderRole};

#[test]
fn removal_requires_confirmation_and_undo_restores_schema_and_rows() {
    let h = common::fresh();
    let a = decks::add(&h.collection, "A::B".into()).unwrap().value.id;
    let p = deck_config::add(&h.collection, "Custom".into()).unwrap().value;
    deck_config::assign(&h.collection, a, p, false).unwrap();
    h.collection
        .run(OpSpec::named("synced", OpSource::UserNonUndoable), |ctx| {
            let mut col = ctx.scope().col()?;
            col.last_sync = col.schema_modified;
            ctx.scope().put_col(&col)?;
            Ok(())
        })
        .unwrap();
    let before = col_mod(&h.collection);
    h.clock.step_ms(60_000);
    assert_eq!(
        deck_config::remove(&h.collection, p, DeckConfigId(1), false).unwrap_err(),
        EngineError::NeedsSchemaConfirmation {
            reasons: vec![SchemaReason::PresetRemoved]
        }
    );
    assert_eq!(col_mod(&h.collection), before);
    let done = deck_config::remove(&h.collection, p, DeckConfigId(1), true).unwrap();
    assert_eq!(done.changes.deck_configs.len(), 1);
    assert_eq!(done.changes.decks.len(), 1);
    assert_eq!(query_int(&h.collection, "SELECT scm FROM col"), NOW_MS + 60_000);
    h.collection.undo(UndoScope::Global).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.deck_config(p).unwrap().is_some());
    assert_eq!(deck_config::for_deck(&*r, a).unwrap().unwrap().id, p);
}

#[test]
fn patch_clone_keep_unknown_fields_and_all_other_parameter_lists() {
    let h = common::fresh();
    let p = deck_config::add(&h.collection, "X".into()).unwrap().value;
    h.collection
        .run(OpSpec::named("future_preset", OpSource::UserUndoable), move |ctx| {
            let mut record = ctx.scope().deck_config(p)?.unwrap();
            let mut bytes = record.config.to_bytes().into_owned();
            // field 3 (FSRS-4) packed [1], field 5 (FSRS-5) packed [2],
            // future field 99 varint 7, future list field 7 packed [3].
            bytes.extend([
                26, 4, 0, 0, 128, 63, 42, 4, 0, 0, 0, 64, 0x98, 6, 7, 58, 4, 0, 0, 64, 64,
            ]);
            record.config = ProtoBlob::decode(&bytes).unwrap();
            ctx.scope().put(&record)?;
            Ok(())
        })
        .unwrap();
    let before = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck_config(p)
        .unwrap()
        .unwrap();
    let cloned = deck_config::clone(&h.collection, p, "Clone".into()).unwrap().value;
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck_config(cloned)
            .unwrap()
            .unwrap()
            .config
            .to_bytes(),
        before.config.to_bytes()
    );
    h.collection
        .run(OpSpec::named("optimizer", OpSource::UserUndoable), move |ctx| {
            deck_config::update_in(ctx.scope(), p, |m| m.set_fsrs_params_6(vec![0.5; 21]))
        })
        .unwrap();
    let mut expected = before.config.clone();
    expected.set_fsrs_params_6(vec![0.5; 21]);
    let after = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck_config(p)
        .unwrap()
        .unwrap();
    assert_eq!(after.config.to_bytes(), expected.to_bytes());
    assert_eq!(after.config.fsrs_params_4(), [1.0]);
    assert_eq!(after.config.fsrs_params_5(), [2.0]);
}

#[test]
fn fallback_reads_and_menu_names_write_nothing() {
    let h = common::fresh();
    let id = decks::add(&h.collection, "A".into()).unwrap().value.id;
    h.collection
        .run(OpSpec::named("missing_preset", OpSource::UserNonUndoable), move |ctx| {
            let mut deck = ctx.scope().deck(id)?.unwrap();
            if let meguri_model::proto::deck::DeckKind::Normal(n) = &mut deck.kind.kind {
                n.config_id = 42;
            }
            ctx.scope().put(&deck)?;
            ctx.scope().remove_deck_config(DeckConfigId(1))?;
            Ok(())
        })
        .unwrap();
    let before = col_mod(&h.collection);
    assert_eq!(
        deck_config::for_deck(&*h.collection.reader(ReaderRole::Query).unwrap(), id)
            .unwrap()
            .unwrap()
            .config
            .new_per_day,
        20
    );
    assert_eq!(col_mod(&h.collection), before);
    assert_eq!(
        deck_config::menu_name(" Spanish ", &["Spanish".into()], h.clock.as_ref(), true),
        Some("Spanish1894701600".into())
    );
    assert_eq!(deck_config::menu_name("  ", &[], h.clock.as_ref(), true), None);
    assert_eq!(
        deck_config::menu_name(" Spanish ", &["Spanish".into()], h.clock.as_ref(), false),
        Some(" Spanish ".into())
    );
    assert!(matches!(
        deck_config::assign(&h.collection, DeckId(99), DeckConfigId(42), false),
        Err(EngineError::NotFound { .. })
    ));
}
