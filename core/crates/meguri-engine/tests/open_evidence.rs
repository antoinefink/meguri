//! Immutable opening facts for privileged steps and each reopened writer.
mod common;

use std::os::unix::fs::MetadataExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meguri_engine::{
    Collection, Engine, EngineConfig, EngineError, LongOp, OpSource, OpSpec, OpenEvidence, OpenStep, Ports, WeakEngine,
};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_storage::{ConfigValue, Initializers, ReadRows, ReaderFactory, ReaderRole};

// Only inspect closed files here: recovery's private copy closes a main-file
// descriptor, which would release this process's POSIX locks on a live writer.
fn stored_facts(path: &std::path::Path) -> (i64, i64, (u64, u64)) {
    let col = ReaderFactory::recovery(path, &Initializers::default())
        .open(ReaderRole::Query)
        .unwrap()
        .col()
        .unwrap();
    let native = std::fs::metadata(path).unwrap();
    (col.modified.0, col.schema_modified.0, (native.dev(), native.ino()))
}

fn live_facts(collection: &Collection) -> (i64, i64, (u64, u64)) {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = reader.snapshot().unwrap();
    let col = snapshot.reader().col().unwrap();
    let native = std::fs::metadata(collection.path()).unwrap();
    let facts = (col.modified.0, col.schema_modified.0, (native.dev(), native.ino()));
    drop(snapshot);
    drop(reader);
    facts
}

struct CaptureStep {
    engine: WeakEngine,
    facts: (i64, i64, (u64, u64)),
    clean: bool,
    seen: Mutex<Option<OpenEvidence>>,
}
impl OpenStep for CaptureStep {
    fn name(&self) -> &str {
        "capture-open-evidence"
    }
    fn run(&self, collection: &Collection) -> Result<(), EngineError> {
        // Current has not been installed, including on the second open.
        assert_eq!(self.engine.upgrade().unwrap().previous_clean_shutdown(), None);
        let evidence = collection.open_evidence()?;
        assert_eq!(evidence.generation, collection.generation());
        assert_eq!(evidence.previous_clean_shutdown, self.clean);
        assert_eq!(
            (evidence.opened_mod, evidence.opened_scm, evidence.native_file),
            self.facts
        );
        assert!(
            !meguri_storage::sidecar::Sidecar::load(&collection.path().parent().unwrap().join("meguri-sidecar.json"))?
                .clean_shutdown
        );
        collection.run(OpSpec::named("opening write", OpSource::SyncIncoming), |ctx| {
            ctx.scope().set_config("open-evidence", &ConfigValue::Integer(7))?;
            Ok(())
        })?;
        assert_eq!(collection.open_evidence()?, evidence);
        *self.seen.lock().unwrap() = Some(evidence);
        Ok(())
    }
}

#[test]
fn initial_step_sees_preclear_facts_and_clean_reopen_refreshes_and_rejects_old_handles() {
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let clock = Arc::new(StepClock::new(common::NOW_MS, UtcOffset::UTC, "UTC"));
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let mut old: Option<Collection> = None;
    for clean in [false, true] {
        let step = Arc::new(CaptureStep {
            engine: engine.downgrade(),
            facts: stored_facts(&path),
            clean,
            seen: Mutex::new(None),
        });
        let c = engine
            .open_raw(
                &path,
                common::open_config(&clock).ports(Ports::new().open_step(step.clone())),
            )
            .unwrap();
        let captured = step.seen.lock().unwrap().unwrap();
        assert_eq!(c.open_evidence().unwrap(), captured);
        if let Some(old) = old.take() {
            assert!(matches!(old.open_evidence(), Err(EngineError::StaleGeneration { .. })));
        }
        clock.step_ms(2000);
        common::add_note_with_card(&c, OpSource::UserNonUndoable, &["later", "back"]);
        assert_ne!(common::col_mod(&c), captured.opened_mod);
        assert_eq!(c.open_evidence().unwrap(), captured);
        engine.close().unwrap();
        assert!(matches!(
            c.open_evidence(),
            Err(EngineError::CollectionUnavailable { .. })
        ));
        old = Some(c);
    }
}

#[test]
fn suspend_resume_and_replacement_capture_fresh_facts_before_any_new_ops() {
    let h = common::Harness::open();
    let initial = h.collection.open_evidence().unwrap();
    h.clock.step_ms(2000);
    common::add_note_with_card(&h.collection, OpSource::UserNonUndoable, &["updated", "back"]);
    let facts = live_facts(&h.collection);
    assert_eq!(h.collection.open_evidence().unwrap(), initial);
    h.engine.suspend(Duration::from_secs(5)).unwrap();
    h.engine.resume().unwrap();
    let resumed = h.collection.open_evidence().unwrap();
    assert_eq!(resumed.generation, initial.generation);
    assert!(resumed.previous_clean_shutdown);
    assert_eq!((resumed.opened_mod, resumed.opened_scm, resumed.native_file), facts);
    assert_ne!(resumed.opened_mod, initial.opened_mod);

    let (_candidate_dir, candidate) = common::copy_fixture("schema-18.anki2");
    let facts = stored_facts(&candidate);
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let replaced = exclusive
        .replace(move |live| {
            std::fs::rename(candidate, live).map_err(|e| EngineError::Internal { message: e.to_string() })
        })
        .unwrap();
    let evidence = replaced.open_evidence().unwrap();
    assert_eq!(evidence.generation, replaced.generation());
    assert_ne!(evidence.generation, resumed.generation);
    assert_eq!((evidence.opened_mod, evidence.opened_scm, evidence.native_file), facts);
    assert!(matches!(
        h.collection.open_evidence(),
        Err(EngineError::StaleGeneration { .. })
    ));
    drop(exclusive);
    drop(slot);
    assert_eq!(replaced.open_evidence().unwrap(), evidence);
}

#[test]
fn failed_swap_reopening_the_same_generation_refreshes_the_incarnation() {
    let h = common::Harness::open();
    let initial = h.collection.open_evidence().unwrap();
    h.clock.step_ms(2000);
    common::add_note_with_card(&h.collection, OpSource::UserNonUndoable, &["changed", "back"]);
    let facts = live_facts(&h.collection);
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let error = EngineError::InvalidArgument {
        message: "swap refused".into(),
    };
    let returned = error.clone();
    assert_eq!(exclusive.replace(move |_| Err(returned)).unwrap_err(), error);
    let refreshed = exclusive.collection().open_evidence().unwrap();
    assert_eq!(refreshed.generation, initial.generation);
    assert_eq!(
        (refreshed.opened_mod, refreshed.opened_scm, refreshed.native_file),
        facts
    );
    assert_ne!(refreshed.opened_mod, initial.opened_mod);
    assert!(refreshed.previous_clean_shutdown);
}
