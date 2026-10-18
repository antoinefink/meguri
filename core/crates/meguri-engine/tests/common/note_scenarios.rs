//! Recorded scenarios run the real U27 adapters. Test-only setup/auxiliary
//! adapters stand in for U28/U38/U44/U128; they are never T0 capabilities.
use meguri_engine::{notes, notetypes};
use meguri_model::ids::{CardId, DeckConfigId, DeckId};
use meguri_model::schema11;
use meguri_storage::{ConfigValue, ReadRows, StampMode, Table, WriteScope};
use meguri_testkit::exec::{ExecError, OpResult, Registry, Session};
use meguri_testkit::scenario::{Setup, Step};
use meguri_testkit::{
    diff::{self, CompareOptions},
    exec::{Executor, MeguriExecutor},
    scenario,
    suite::Suite,
};
use serde_json::Value;
use std::path::Path;

pub fn play(id: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance");
    let loaded = scenario::load(&root.join(format!("scenarios/{id}.yaml"))).unwrap();
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(format!("goldens/{id}.json"))).unwrap()).unwrap();
    assert_eq!(golden["scenario_sha256"].as_str(), Some(loaded.sha256.as_str()));
    let executor = executor();
    assert!(
        executor.missing(&loaded.scenario.needs()).is_empty(),
        "missing {:?}",
        executor.missing(&loaded.scenario.needs())
    );
    let actual = executor.run(&loaded).unwrap_or_else(|e| panic!("{id}: {e}"));
    let suite = Suite::open(&root).unwrap();
    let comparison = diff::compare(
        id,
        &golden["observations"],
        &actual,
        &CompareOptions {
            set: &loaded.scenario.compare.set,
            deviations: &suite.deviations,
            overlay: &[],
        },
    );
    assert!(comparison.differences.is_empty(), "{id}: {:#?}", comparison.differences);
}

fn executor() -> MeguriExecutor {
    let mut registry: Registry = MeguriExecutor::core_registry();
    registry.setup("decks", decks);
    registry.setup("presets", presets);
    registry.op_rejecting("legacy_update", &["invalid_json"], legacy);
    registry.op("answer", answer);
    registry.op_rejecting("check_db", &["corrupt"], check_db);
    MeguriExecutor::with(registry)
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("test_standin", e.to_string())
}
fn scope<T>(
    session: &Session<'_>,
    mode: StampMode,
    f: impl FnOnce(&mut WriteScope<'_>) -> Result<T, ExecError>,
) -> Result<T, ExecError> {
    let mut scope = session
        .storage
        .collection()
        .unwrap()
        .writer()
        .begin_write(mode, session.clock.as_ref(), None)
        .map_err(core)?;
    let value = f(&mut scope)?;
    scope.commit().map_err(core)?;
    Ok(value)
}
fn presets(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    scope(session, StampMode::Local, |scope| {
        for spec in setup.presets.iter().flatten() {
            let mut preset = scope.deck_config(DeckConfigId(1)).map_err(core)?.unwrap();
            preset.name = spec.name.clone();
            scope.add_deck_config(&preset).map_err(core)?;
        }
        Ok(())
    })
}
fn decks(session: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    scope(session, StampMode::Local, |scope| {
        // U28's missing bootstrap key, needed before U27 draws positions.
        scope.set_config("nextPos", &ConfigValue::from(1_i64)).map_err(core)?;
        for spec in setup.decks.iter().flatten() {
            let mut deck = scope.deck(DeckId(1)).map_err(core)?.unwrap();
            deck.name = spec.name().replace("::", "\u{1f}");
            if let Some(name) = spec.preset() {
                let preset = scope
                    .ids_after(Table::DeckConfig, None, 100)
                    .map_err(core)?
                    .into_iter()
                    .map(|id| scope.deck_config(DeckConfigId(id)).unwrap().unwrap())
                    .find(|p| p.name == name)
                    .unwrap();
                if let meguri_model::proto::deck::DeckKind::Normal(normal) = &mut deck.kind.kind {
                    normal.config_id = preset.id.0;
                }
            }
            scope.add_deck(&deck).map_err(core)?;
        }
        Ok(())
    })
}
fn merge(value: &mut Value, patch: &Value) {
    if let Value::Object(patch) = patch {
        if !value.is_object() {
            *value = serde_json::json!({});
        }
        for (key, patch) in patch {
            if patch.is_null() {
                value.as_object_mut().unwrap().remove(key);
            } else {
                merge(&mut value[key], patch);
            }
        }
    } else {
        *value = patch.clone();
    }
}
fn legacy(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::LegacyUpdate(args) = step else { unreachable!() };
    scope(
        session,
        if args.preset.is_some() {
            StampMode::Local
        } else {
            StampMode::CallerSupplied
        },
        |scope| {
            if let Some(name) = &args.notetype {
                let nt = notetypes::read::by_name(scope, name).map_err(core)?.unwrap();
                let mut object: Value = serde_json::from_str(&schema11::notetype_to_json(&nt)).unwrap();
                merge(&mut object, &Value::Object(args.patch.clone()));
                let mut nt = schema11::notetype_from_json(&object.to_string()).map_err(core)?;
                nt.config.reqs = notetypes::write::card_requirements(&nt);
                scope.put_notetype(&nt).map_err(core)?;
            } else if let Some(name) = &args.preset {
                let preset = scope
                    .ids_after(Table::DeckConfig, None, 100)
                    .map_err(core)?
                    .into_iter()
                    .map(|id| scope.deck_config(DeckConfigId(id)).unwrap().unwrap())
                    .find(|p| p.name == *name)
                    .unwrap();
                let mut object: Value = serde_json::from_str(&schema11::deck_config_to_json(&preset)).unwrap();
                merge(&mut object, &Value::Object(args.patch.clone()));
                scope
                    .put(&schema11::deck_config_from_json(&object.to_string()).map_err(core)?)
                    .map_err(core)?;
            } else {
                return Err(core("U27 test stand-in only supports note types and presets"));
            }
            Ok(())
        },
    )?;
    Ok(OpResult::Done)
}
fn answer(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Answer(args) = step else { unreachable!() };
    let id = CardId(session.cards[args.card.0.index()]);
    // U44's two recorded answers: only type/queue/due are observed here.
    let due = match session.scenario.scenario.id.as_str() {
        "cardgen/position/counter" => 1894702270,
        "cardgen/position/random" => 1894705895,
        other => return Err(core(format!("no answer stand-in for {other}"))),
    };
    scope(session, StampMode::Local, |scope| {
        let mut card = scope.card(id).map_err(core)?.unwrap();
        card.card_type = meguri_model::card::CardType::Learning;
        card.queue = meguri_model::card::CardQueue::Learning;
        card.due = due;
        scope.put(&card).map_err(core)?;
        Ok(())
    })?;
    Ok(OpResult::Done)
}
fn check_db(session: &mut Session<'_>, _step: &Step) -> Result<OpResult, ExecError> {
    let count = scope(session, StampMode::CallerSupplied, |scope| {
        let mut count = 0;
        for mut note in scope.notes_after(None, 100).map_err(core)? {
            let nt = scope.notetype(note.notetype_id).map_err(core)?.unwrap();
            let mut fields: Vec<String> = note.field_values().map(str::to_owned).collect();
            if fields.len() != nt.fields.len() {
                notes::fields::repair(&mut fields, nt.fields.len()).map_err(core)?;
                note.fields = fields.join("\u{1f}");
                note.tags = " db-check ".to_owned();
                scope.put(&note).map_err(core)?;
                count += 1;
            }
        }
        Ok(count)
    })?;
    Ok(OpResult::Output(
        serde_json::json!({"card_last_review_time":0,"card_properties":0,"cards_without_note":0,"cards_without_template":0,"decks_missing":0,"duplicate_card_ords":0,"field_counts":count,"future_ids":0,"new_positions_too_high":0,"notes_invalid_utf8":0,"notetypes_recovered":0,"revlog_properties":0}),
    ))
}
