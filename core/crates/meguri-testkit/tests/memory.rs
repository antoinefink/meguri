//! Prefixes are test inputs only: mixed answer/save/check scenarios remain
//! pending until their complete product adapters land.
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::{
    exec::{ExecError, Executor, MeguriExecutor, Session},
    scenario,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

#[test]
fn memory_resolver_and_count_match_the_recorded_observations() {
    let mut checked = 0;
    for name in [
        "on-the-fly",
        "sm2-history",
        "no-history",
        "reset",
        "skipped-rows",
        "crafted-rows",
        "ignore-before",
        "ignore-before-boundary",
        "ignore-before-count",
    ] {
        let mut value = scenario::parse_yaml(
            &fs::read_to_string(root().join(format!("scenarios/sched/memory/{name}.yaml"))).unwrap(),
        )
        .unwrap();
        let steps = value["steps"].as_array_mut().unwrap();
        if let Some(index) = steps.iter().position(|step| step.get("answer").is_some()) {
            steps.truncate(index);
        }
        let count = steps.len();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scenario.json");
        fs::write(&path, value.to_string()).unwrap();
        let actual = MeguriExecutor::new().run(&scenario::load(&path).unwrap()).unwrap();
        let golden: Value = serde_json::from_str(
            &fs::read_to_string(root().join(format!("goldens/sched/memory/{name}.json"))).unwrap(),
        )
        .unwrap();
        let mut expected = golden["observations"].clone();
        expected["steps"].as_array_mut().unwrap().truncate(count);
        let comparison = meguri_testkit::diff::compare(name, &expected, &actual, &Default::default());
        assert!(
            comparison.differences.is_empty(),
            "{name}: {:?}",
            comparison.differences
        );
        checked += 1;
    }
    assert_eq!(checked, 9);
}

fn recompute_and_assert(session: &mut Session<'_>) -> Result<(), ExecError> {
    use meguri_engine::{
        OpSpec, UndoScope,
        sched::memory::{self, Target},
        tags::RunOptions,
    };
    use meguri_model::{changes::OpSource, ids::CardId};
    let collection = session.engine_collection()?;
    let words = meguri_model::ids::DeckId(session.decks["Words"]);
    // Set the input retention directly; U49 owns the full options save.
    collection
        .run(
            OpSpec::named("test_retention", OpSource::UserNonUndoable),
            move |context| {
                let card = context.scope().cards_of_deck(words)?.remove(0);
                let preset = meguri_sched::model::preset::PresetView::for_card(context.scope(), &card)?.unwrap();
                let edited = preset.edit(|fields| fields.desired_retention = 0.85).unwrap().unwrap();
                context.scope().put(&edited)?;
                Ok(())
            },
        )
        .unwrap();
    let dump = || {
        let rows = collection.reader(ReaderRole::Query).unwrap();
        (
            rows.execute_raw("SELECT * FROM cards ORDER BY id", &[]).unwrap().rows,
            rows.execute_raw("SELECT * FROM revlog ORDER BY id", &[]).unwrap().rows,
        )
    };
    let before = dump();
    let result = memory::recompute(
        &collection,
        Target::Decks(vec![words]),
        OpSource::UserUndoable,
        "Deck Options",
        &RunOptions::default(),
    )
    .unwrap();
    assert_eq!((result.value.processed, result.value.changed), (4, 4));
    assert!(result.changes.cards.iter().all(|card| card.memory_state_only));
    let after = dump();
    assert_eq!(before.1, after.1);
    let golden: Value =
        serde_json::from_str(&fs::read_to_string(root().join("goldens/sched/memory/recompute.json")).unwrap()).unwrap();
    let rows = collection.reader(ReaderRole::Query).unwrap();
    for (index, id) in session.cards.iter().enumerate() {
        let actual = rows.card(CardId(*id)).unwrap().unwrap();
        let expected = &golden["observations"]["steps"][20]["card"][format!("#c{}", index + 1)];
        let data: Value = serde_json::from_str(&actual.data.to_text()).unwrap();
        assert_eq!(data, expected["data"], "card {}", index + 1);
        for column in 0..18 {
            if ![4, 5, 17].contains(&column) {
                assert_eq!(before.0[index][column], after.0[index][column]);
            }
        }
    }
    drop(rows);
    let unchanged = memory::recompute(
        &collection,
        Target::Decks(vec![words]),
        OpSource::UserUndoable,
        "Deck Options",
        &RunOptions::default(),
    )
    .unwrap();
    assert_eq!(unchanged.value.changed, 0);
    assert_eq!(dump(), after);
    collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(), before);
    collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(), after);
    Ok(())
}

#[test]
fn direct_recompute_matches_golden_cards_and_is_one_reversible_memory_only_step() {
    let mut value =
        scenario::parse_yaml(&fs::read_to_string(root().join("scenarios/sched/memory/recompute.yaml")).unwrap())
            .unwrap();
    let steps = value["steps"].as_array_mut().unwrap();
    let end = steps
        .iter()
        .position(|step| step.get("save_deck_options").is_some())
        .unwrap();
    steps.truncate(end);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scenario.json");
    fs::write(&path, value.to_string()).unwrap();
    let mut registry = MeguriExecutor::core_registry();
    registry.finish(recompute_and_assert);
    MeguriExecutor::with(registry)
        .run(&scenario::load(&path).unwrap())
        .unwrap();
}

fn repair_and_assert(session: &mut Session<'_>) -> Result<(), ExecError> {
    use meguri_engine::OpSpec;
    use meguri_model::changes::OpSource;
    use meguri_storage::{StampMode, queries::revlog_fsrs::HistorySelection};
    let collection = session.engine_collection()?;
    let ids = session.cards.iter().map(|id| meguri_model::ids::CardId(*id)).collect();
    let count = collection
        .run(
            OpSpec::named("test_lrt_rule", OpSource::Maintenance).stamp(StampMode::Raw),
            move |context| {
                let mut updates = Vec::new();
                context.scope().visit_fsrs_histories(
                    &HistorySelection::Cards(ids),
                    |mut card, rows| -> Result<(), meguri_storage::StorageError> {
                        if meguri_sched::memory::resolve::repair_last_review_time(&mut card, rows) {
                            updates.push(card);
                        }
                        Ok(())
                    },
                )?;
                let count = updates.len();
                for card in updates {
                    context.scope().put_card_data(card.id, &card.data)?;
                }
                Ok(count)
            },
        )
        .unwrap()
        .value;
    assert_eq!(count, 8);
    let rows = collection.reader(ReaderRole::Query).unwrap();
    let actual = rows
        .execute_raw("SELECT data,mod,usn FROM cards ORDER BY id", &[])
        .unwrap();
    let golden: Value =
        serde_json::from_str(&fs::read_to_string(root().join("goldens/sched/memory/lrt-repair.json")).unwrap())
            .unwrap();
    for (index, row) in actual.rows.iter().enumerate() {
        let expected = &golden["observations"]["steps"][24]["db_exec"]["rows"][index];
        assert_eq!(
            row[0],
            meguri_storage::raw::SqlValue::Text(expected[0].as_str().unwrap().into())
        );
        assert_eq!(
            row[1],
            meguri_storage::raw::SqlValue::Integer(expected[1].as_i64().unwrap())
        );
        assert_eq!(
            row[2],
            meguri_storage::raw::SqlValue::Integer(expected[2].as_i64().unwrap())
        );
    }
    Ok(())
}

#[test]
fn direct_lrt_repair_matches_all_golden_data_and_preserves_stamps() {
    let mut value =
        scenario::parse_yaml(&fs::read_to_string(root().join("scenarios/sched/memory/lrt-repair.yaml")).unwrap())
            .unwrap();
    let steps = value["steps"].as_array_mut().unwrap();
    let end = steps.iter().position(|step| step.get("check_db").is_some()).unwrap();
    steps.truncate(end);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scenario.json");
    fs::write(&path, value.to_string()).unwrap();
    let mut registry = MeguriExecutor::core_registry();
    registry.finish(repair_and_assert);
    MeguriExecutor::with(registry)
        .run(&scenario::load(&path).unwrap())
        .unwrap();
}
