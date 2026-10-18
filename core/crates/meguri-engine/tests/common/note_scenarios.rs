//! Recorded scenarios run the real U27 adapters. Test-only setup/auxiliary
//! adapters stand in only for U44/U128; they are never T0 capabilities.
use meguri_engine::notes;
use meguri_model::ids::CardId;
use meguri_storage::{ReadRows, StampMode, WriteScope};
use meguri_testkit::exec::{ExecError, OpResult, Registry, Session};
use meguri_testkit::scenario::Step;
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
    registry.op("answer", answer);
    registry.op_rejecting("check_db", &["corrupt"], check_db);
    MeguriExecutor::with(registry)
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("test_standin", e.to_string())
}
fn scope<T: Send + 'static>(
    session: &mut Session<'_>,
    mode: StampMode,
    f: impl FnOnce(&mut WriteScope<'_>) -> Result<T, ExecError> + Send + 'static,
) -> Result<T, ExecError> {
    let collection = session.engine_collection()?;
    collection
        .run(
            meguri_engine::OpSpec::named("test_standin", meguri_engine::OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(mode),
            move |ctx| {
                f(ctx.scope()).map_err(|error| meguri_engine::EngineError::Internal {
                    message: error.to_string(),
                })
            },
        )
        .map(|result| result.value)
        .map_err(core)
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
    scope(session, StampMode::Local, move |scope| {
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
