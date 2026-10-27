//! The first U127 slice: shared T6 safety admission, not periodic scheduling.
mod common;
use common::{Harness, NOW_MS};
use meguri_engine::{
    CancelToken, ClearCause, Collection, EngineError, OpSource, OpSpec, Ports, SchemaReason, UndoScope, deck_config,
    executor::{
        catalog,
        plan_commit::{Plan, plan_and_commit},
    },
    ports::{BackupProtection, BackupReceipt, BackupService, GuardedBackup},
    sched::deck_options,
    tags::RunOptions,
};
use meguri_model::{
    ids::{DeckConfigId, DeckId},
    time::EpochMillis,
};
use meguri_storage::{ReadRows, ReaderRole, Table};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;

type Hook = Box<dyn FnOnce(&Collection, &CancelToken) -> Result<(), EngineError> + Send>;
#[derive(Default)]
struct Backup {
    calls: AtomicUsize,
    hook: Mutex<Option<Hook>>,
}
impl BackupService for Backup {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        panic!("use the cancellable safety seam")
    }
    fn before_safety_operation(&self, c: &Collection, cancel: &CancelToken) -> Result<GuardedBackup, EngineError> {
        let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let hook = self.hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook(c, cancel)?;
        }
        cancel.check()?;
        Ok(GuardedBackup {
            receipt: Some(BackupReceipt {
                name: format!("witness-{count}"),
                size_bytes: 1,
                // A writer's instant precedes completion; never use it as reuse age.
                instant: EpochMillis(NOW_MS - 60_000),
                snapshot_modified: EpochMillis(0),
            }),
            protection: Some(Arc::new(())),
        })
    }
    fn protect_backup(&self, _: &str) -> Result<Option<Arc<dyn BackupProtection>>, EngineError> {
        Ok(Some(Arc::new(())))
    }
}
fn harness() -> (Harness, Arc<Backup>) {
    let backup = Arc::new(Backup::default());
    (Harness::open_with(Ports::new().backup_service(backup.clone())), backup)
}
fn structural(c: &Collection) {
    c.run(
        OpSpec::named("structural witness", OpSource::UserUndoable).schema_changes([SchemaReason::FieldsChanged]),
        |ctx| meguri_engine::mark_schema_change(ctx.scope(), &[SchemaReason::FieldsChanged]),
    )
    .unwrap();
}
fn settle(c: &Collection) {
    c.run(
        OpSpec::named("settle rollover", OpSource::Rollover).stamp(meguri_storage::StampMode::Raw),
        |_| Ok(()),
    )
    .unwrap();
}
fn remove_draft(h: &Harness) -> deck_options::Draft {
    let id = deck_config::add(&h.collection, "remove me".into()).unwrap().value;
    let mut draft = deck_options::Draft::new(deck_options::snapshot(&h.collection, DeckId(1)).unwrap());
    draft.remove(id.0).unwrap();
    draft
}

#[test]
fn structural_safety_reuses_at_nine_minutes_but_not_exactly_ten() {
    let (h, backup) = harness();
    structural(&h.collection);
    settle(&h.collection);
    h.clock.step_ms(9 * 60_000);
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 1);
    h.clock.step_ms(60_000);
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn completion_time_counts_and_backward_age_is_unprovable() {
    let (h, backup) = harness();
    let clock = h.clock.clone();
    *backup.hook.lock().unwrap() = Some(Box::new(move |_, _| {
        clock.step_ms(11 * 60_000);
        Ok(())
    }));
    structural(&h.collection);
    settle(&h.collection);
    h.clock.step_ms(9 * 60_000);
    structural(&h.collection);
    assert_eq!(
        backup.calls.load(Ordering::SeqCst),
        1,
        "age starts at successful completion"
    );
    h.clock.step_ms(-10 * 60_000);
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 2, "a negative age is not proof");
}

#[test]
fn loss_of_undo_coverage_and_nonundoable_commits_force_fresh_backups() {
    let (h, backup) = harness();
    structural(&h.collection);
    h.collection.clear_undo(ClearCause::CheckDatabase).unwrap();
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 2);
    h.collection
        .run(
            OpSpec::named("nonundoable", OpSource::UserNonUndoable).skip_undo(),
            |ctx| {
                ctx.scope().set_config("coverage witness", &true.into())?;
                Ok(())
            },
        )
        .unwrap();
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 3);
    // Each distinct Other step consumes history: eviction breaks the proof even
    // though peek_undo still has an available top step.
    for i in 0..31 {
        h.collection
            .run(OpSpec::named("history", OpSource::UserUndoable), move |ctx| {
                ctx.scope().set_config(&format!("history {i}"), &true.into())?;
                Ok(())
            })
            .unwrap();
    }
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 4);
}

#[test]
fn undo_redo_rollover_and_reopen_discard_reuse() {
    let (h, backup) = harness();
    structural(&h.collection);
    settle(&h.collection);
    h.collection.undo(UndoScope::Global).unwrap();
    h.collection.redo(UndoScope::Global).unwrap();
    h.clock.step_ms(1_000);
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 2);
    h.clock.step_ms(86_400_000);
    h.collection.check_rollover().unwrap();
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 3);
    h.engine.close().unwrap();
    let c = h
        .engine
        .open_raw(
            &h.path,
            common::open_config(&h.clock).ports(Ports::new().backup_service(backup.clone())),
        )
        .unwrap();
    structural(&c);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 4);
}

#[test]
fn destructive_nonundoable_operations_always_take_their_own_backup() {
    let (h, backup) = harness();
    structural(&h.collection);
    for _ in 0..2 {
        h.collection
            .run(OpSpec::new(catalog::CHECK_DATABASE, OpSource::UserNonUndoable), |_| {
                Ok(())
            })
            .unwrap();
    }
    assert_eq!(backup.calls.load(Ordering::SeqCst), 3);
}

#[test]
fn refusal_and_cancel_precede_rollover_and_all_guarded_writes() {
    for cancel in [false, true] {
        let (h, backup) = harness();
        let draft = remove_draft(&h);
        settle(&h.collection);
        h.clock.step_ms(86_400_000);
        let before = common::steps::dump(&h.collection);
        let seq = h.collection.last_seq();
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        let events = h.engine.stream().subscribe();
        *backup.hook.lock().unwrap() = Some(Box::new(move |_, token| {
            if cancel {
                token.cancel();
                return token.check();
            }
            Err(EngineError::InvalidArgument {
                message: "provider refused".into(),
            })
        }));
        let result = deck_options::save(&h.collection, draft, &RunOptions::default());
        if cancel {
            assert!(matches!(result, Err(EngineError::Cancelled)));
        } else {
            assert!(matches!(result, Err(EngineError::SafetyBackupFailed(_))));
        }
        assert_eq!(common::steps::dump(&h.collection), before);
        assert_eq!(h.collection.last_seq(), seq);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        assert!(common::drain(&events, Duration::from_millis(20)).is_empty());
    }
}

#[test]
fn answers_commit_while_safety_waits_and_the_guarded_operation_replans_once() {
    let (h, backup) = harness();
    let id = deck_config::add(&h.collection, "remove me".into()).unwrap().value;
    let (_, card) = common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["front", "back"]).value;
    settle(&h.collection);
    let (entered, waiting) = mpsc::sync_channel(1);
    let (release, released) = mpsc::sync_channel(1);
    *backup.hook.lock().unwrap() = Some(Box::new(move |_, _| {
        entered.send(()).unwrap();
        released.recv().unwrap();
        Ok(())
    }));
    let c = h.collection.clone();
    let worker = std::thread::spawn(move || deck_config::remove(&c, id, DeckConfigId(1)));
    waiting.recv_timeout(Duration::from_secs(5)).unwrap();
    // The real answer path can run while the provider waits off the writer.
    let prediction = meguri_engine::sched::answer::predict(&h.collection, card, &mut Default::default(), None).unwrap();
    meguri_engine::sched::answer::answer(
        &h.collection,
        prediction.answer(meguri_sched::answer::Rating::Good, 1000),
    )
    .unwrap();
    deck_config::rename(&h.collection, DeckConfigId(1), "concurrent preset".into()).unwrap();
    release.send(()).unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(backup.calls.load(Ordering::SeqCst), 1);
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck_config(id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(card)
            .unwrap()
            .unwrap()
            .reps,
        1
    );
    // Exercise the shared plan seam with an observable attempt count too.
    h.collection.clear_undo(ClearCause::CheckDatabase).unwrap();
    let c = h.collection.clone();
    *backup.hook.lock().unwrap() = Some(Box::new(move |_, _| {
        deck_config::rename(&c, DeckConfigId(1), "force one replan".into())?;
        Ok(())
    }));
    let mut plans = 0;
    plan_and_commit(
        &h.collection,
        ReaderRole::Maintenance,
        OpSpec::named("planned structural witness", OpSource::UserUndoable)
            .schema_changes([SchemaReason::PresetRemoved]),
        |p| {
            plans += 1;
            Ok(Plan {
                value: (),
                preconditions: vec![meguri_engine::Precondition::TableUntouched {
                    table: Table::DeckConfig,
                    since: p.seq(),
                }],
            })
        },
        |ctx, ()| meguri_engine::mark_schema_change(ctx.scope(), &[SchemaReason::PresetRemoved]),
    )
    .unwrap();
    assert_eq!(plans, 2);
    assert_eq!(
        backup.calls.load(Ordering::SeqCst),
        2,
        "one backup across the stale retry"
    );
}

#[test]
fn nonundoable_commit_during_publication_does_not_publish_reuse_proof() {
    let (h, backup) = harness();
    *backup.hook.lock().unwrap() = Some(Box::new(|c, _| {
        c.run(OpSpec::named("during publication", OpSource::Maintenance), |ctx| {
            ctx.scope().set_config("nonundoable publication", &true.into())?;
            Ok(())
        })?;
        Ok(())
    }));
    structural(&h.collection);
    h.clock.step_ms(1_000);
    structural(&h.collection);
    assert_eq!(backup.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn staged_removal_noop_and_nonstructural_saves_take_no_safety_backup() {
    let (h, backup) = harness();
    let mut draft = deck_options::Draft::new(deck_options::snapshot(&h.collection, DeckId(1)).unwrap());
    let staged = meguri_engine::sched::deck_options::stage_add(&h.collection, &mut draft, "staged", false)
        .unwrap()
        .unwrap();
    draft.remove(staged).unwrap();
    assert!(draft.schema_reasons().is_empty());
    deck_options::save(&h.collection, draft, &RunOptions::default()).unwrap();
    let mut draft = deck_options::Draft::new(deck_options::snapshot(&h.collection, DeckId(1)).unwrap());
    draft
        .edit(
            Some(1),
            meguri_sched::options::Field::LearnSteps,
            meguri_sched::options::Value::Text("3m 30m".into()),
        )
        .unwrap();
    deck_options::save(&h.collection, draft, &RunOptions::default()).unwrap();
    assert_eq!(backup.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn structural_save_backs_up_before_rollover_unburies_cards() {
    let (h, backup) = harness();
    let (_, card) = common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["front", "back"]).value;
    let draft = remove_draft(&h);
    h.collection
        .run(
            OpSpec::named("bury before rollover", OpSource::UserUndoable),
            move |ctx| {
                let mut row = ctx.scope().card(card)?.unwrap();
                row.queue = meguri_model::card::CardQueue::BuriedByUser;
                ctx.scope().put(&row)?;
                Ok(())
            },
        )
        .unwrap();
    settle(&h.collection);
    h.clock.step_ms(86_400_000);
    let before = common::steps::dump(&h.collection);
    *backup.hook.lock().unwrap() = Some(Box::new(move |c, _| {
        assert_eq!(common::steps::dump(c), before, "even rollover must wait for admission");
        Ok(())
    }));
    let result = deck_options::save(&h.collection, draft, &RunOptions::default());
    // Rollover may invalidate the draft's Config precondition. That Stale is
    // expected; preparation must still wait until safety admission succeeds.
    assert!(matches!(
        result,
        Err(EngineError::Stale {
            table: Table::Config,
            ..
        })
    ));
    assert_eq!(backup.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(card)
            .unwrap()
            .unwrap()
            .queue,
        meguri_model::card::CardQueue::New
    );
}
