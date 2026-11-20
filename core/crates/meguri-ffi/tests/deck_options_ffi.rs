//! A separate binary owns the process runtime and exercises real U49 exports.
use meguri_ffi::{
    CoreError, CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    deck_options::*,
};
use std::{
    sync::{Arc, mpsc},
    time::Duration,
};
struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        1_894_701_600_000
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct Space;
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        DiskCapacity::Available { bytes: u64::MAX }
    }
}
struct Listener(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Listener {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}
fn finish(rx: &mpsc::Receiver<CoreEvent>, id: u64) -> JobOutcome {
    loop {
        let event = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        if let CoreEventKind::JobFinished { id: got, outcome } = event.kind
            && got == id
        {
            return outcome;
        }
    }
}
fn open(dir: &tempfile::TempDir) -> mpsc::Receiver<CoreEvent> {
    let collection = dir.path().join("collection");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    meguri_engine::bootstrap::create(
        &collection.join("collection.anki2"),
        &meguri_model::clock::FixedClock::new(1_894_701_600_000, meguri_model::clock::UtcOffset::UTC),
        &meguri_model::rng::SeededRandom::new(7),
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    let handle = collection::start_open(
        CollectionLocation {
            manual_transfer_directory: dir.path().join("ManualTransfers").to_string_lossy().into_owned(),
            collection_directory: collection.to_str().unwrap().into(),
            backups_directory: backups.to_str().unwrap().into(),
            app_group_snapshot: dir.path().join("widget.snapshot").to_str().unwrap().into(),
        },
        Arc::new(Clock),
        Arc::new(Space),
        7,
    )
    .unwrap();
    assert_eq!(finish(&rx, handle.id), JobOutcome::Succeeded);
    rx
}
fn read(rx: &mpsc::Receiver<CoreEvent>) -> Arc<DeckOptionsEditor> {
    let read = start_deck_options(1).unwrap();
    assert_eq!(finish(rx, read.handle.id), JobOutcome::Succeeded);
    read.result.result().unwrap().unwrap()
}
fn save(rx: &mpsc::Receiver<CoreEvent>, editor: &Arc<DeckOptionsEditor>) -> JobOutcome {
    let save = start_save_deck_options(editor.clone()).unwrap();
    finish(rx, save.handle.id)
}
fn value(snapshot: &DeckOptionsSnapshot, field: OptionField) -> OptionValue {
    snapshot
        .deck_values
        .iter()
        .chain(&snapshot.presets[0].values)
        .find(|v| v.field == field)
        .unwrap()
        .value
        .clone()
}
#[test]
fn keep_leaves_the_deck_values_as_stored_while_the_page_saves_a_preset_value() {
    let dir = tempfile::tempdir().unwrap();
    let rx = open(&dir);
    let editor = read(&rx);
    editor
        .edit(DeckOptionsEdit::Set {
            preset: None,
            field: OptionField::NewLimit,
            value: OptionValue::Number { value: 50. },
        })
        .unwrap();
    assert_eq!(save(&rx, &editor), JobOutcome::Succeeded);
    let deck_values = [
        OptionField::NewLimit,
        OptionField::ReviewLimit,
        OptionField::NewLimitToday,
        OptionField::ReviewLimitToday,
        OptionField::DeckDesiredRetention,
    ];
    let editor = read(&rx);
    assert!(editor.snapshot().unwrap().kept_deck_values.is_empty());
    assert!(matches!(
        editor.edit(DeckOptionsEdit::Keep {
            fields: vec![OptionField::NewPerDay],
        }),
        Err(CoreError::InvalidArgument { .. })
    ));
    let snapshot = editor
        .edit(DeckOptionsEdit::Keep {
            fields: deck_values.to_vec(),
        })
        .unwrap();
    assert_eq!(snapshot.kept_deck_values, deck_values);
    assert!(!snapshot.unsaved_changes);
    let snapshot = editor
        .edit(DeckOptionsEdit::Set {
            preset: Some(1),
            field: OptionField::NewPerDay,
            value: OptionValue::Number { value: 15. },
        })
        .unwrap();
    assert!(snapshot.unsaved_changes);
    // The kept This-deck limit of 50 plays no part in the warnings (O17.1).
    assert!(editor.validate().unwrap().is_empty());
    assert_eq!(save(&rx, &editor), JobOutcome::Succeeded);
    let snapshot = read(&rx).snapshot().unwrap();
    assert_eq!(value(&snapshot, OptionField::NewLimit), OptionValue::Number { value: 50. });
    assert_eq!(value(&snapshot, OptionField::NewPerDay), OptionValue::Number { value: 15. });
    meguri_ffi::lifecycle::close_collection().unwrap();
}
#[test]
fn runtime_jobs_expose_snapshot_validation_staging_save_stale_and_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let rx = open(&dir);
    let read = start_deck_options(1).unwrap();
    assert_eq!(finish(&rx, read.handle.id), JobOutcome::Succeeded);
    let editor = read.result.result().unwrap().unwrap();
    let snapshot = editor.snapshot().unwrap();
    assert_eq!(snapshot.selected_preset, 1);
    assert_eq!(snapshot.presets[0].use_count, 1);
    assert!(!snapshot.unsaved_changes);
    assert!(
        snapshot
            .metadata
            .iter()
            .any(|m| m.field == OptionField::LoadBalancerEnabled && m.global)
    );
    editor
        .edit(DeckOptionsEdit::Set {
            preset: Some(1),
            field: OptionField::DesiredRetention,
            value: OptionValue::Number { value: 0.68 },
        })
        .unwrap();
    assert!(
        editor
            .validate()
            .unwrap()
            .iter()
            .any(|i| i.code == OptionIssueCode::OutOfRange)
    );
    let save = start_save_deck_options(editor.clone()).unwrap();
    assert!(matches!(
        finish(&rx, save.handle.id),
        JobOutcome::Failed {
            error: CoreError::DeckOptionsInvalid { .. }
        }
    ));
    assert!(save.result.result().unwrap().is_none());
    editor
        .edit(DeckOptionsEdit::RestoreDefault {
            preset: Some(1),
            field: OptionField::DesiredRetention,
        })
        .unwrap();
    let snapshot = editor.edit(DeckOptionsEdit::Clone { name: "Staged".into() }).unwrap();
    assert!(snapshot.selected_preset < 0);
    assert!(snapshot.unsaved_changes);
    let save = start_save_deck_options(editor.clone()).unwrap();
    assert_eq!(finish(&rx, save.handle.id), JobOutcome::Succeeded);
    assert_eq!(
        save.result.result().unwrap().unwrap().summary.undo_name.as_deref(),
        Some("Deck Options")
    );
    let stale = start_save_deck_options(editor).unwrap();
    assert!(matches!(
        finish(&rx, stale.handle.id),
        JobOutcome::Failed {
            error: CoreError::Stale { .. }
        }
    ));
    let prefs = start_save_scheduling_preferences(SchedulingPreferencesChange {
        rollover: None,
        learn_ahead_minutes: Some(33.),
        load_balancer: Some(false),
    })
    .unwrap();
    assert_eq!(finish(&rx, prefs.handle.id), JobOutcome::Succeeded);
    assert_eq!(
        prefs.result.result().unwrap().unwrap().undo_name.as_deref(),
        Some("Preferences")
    );
    let read = start_scheduling_preferences().unwrap();
    assert_eq!(finish(&rx, read.handle.id), JobOutcome::Succeeded);
    assert_eq!(read.result.result().unwrap().unwrap().learn_ahead_minutes, 33);
    meguri_ffi::lifecycle::close_collection().unwrap();
    assert!(matches!(
        meguri_ffi::lifecycle::collection_lifecycle().unwrap(),
        meguri_ffi::lifecycle::CollectionLifecycle::NoCollection
    ));
    assert!(matches!(start_deck_options(1), Err(CoreError::Unavailable { .. })));
}
