//! `sync/cloudkit-recovery#V27.2`, V27.6 and V27.8 on the real Runtime, the
//! real adoption and publication operations and the testkit's CloudKit
//! service. Public synthetic collections only.
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_sync::cloud::{
    BindingFacts, BindingId, BindingState, BuildScope, ConflictCopy, ConflictKind, ConflictReason, ContainerId, CopyId,
    Environment, EpochId, IsolationReason, OpaqueBytes, OpenOutcome, PayloadDigest, RecordKey, TrackingStore,
    entity_codec as codec,
    runtime::{KeyByteSource, KeySourceError},
    setup::{AskReason, OtherServiceHistory, SetupDecision, SetupQuestion, SetupRow, SilentStart},
};
use meguri_testkit::{
    cloud::{
        Config, RequestKind, Scope, Service,
        coordinator::{Point, PublicationRig},
        devices::IdentityReads,
    },
    generate::recipe::{Recipe, generate_recipe},
};

const ORIGIN: i64 = 1_735_689_600_000;
const SEED: u64 = 0x2711_1902;
const STEPS: usize = 10_000;
const UNKNOWN: OtherServiceHistory = OtherServiceHistory::Unknown;

struct Keys(Mutex<VecDeque<[u8; 32]>>);
impl Keys {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new((1..=8_u8).map(|n| [n.wrapping_mul(29); 32]).collect())))
    }
}
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        self.0.lock().unwrap().pop_front().ok_or(KeySourceError::Unavailable)
    }
}
fn scope(build: &str) -> Scope {
    Scope {
        container: format!("test.meguri.setup.{build}"),
        environment: "development".into(),
        account: "synthetic-setup-account".into(),
    }
}
fn seed(dir: &Path) -> u64 {
    dir.file_name()
        .unwrap()
        .as_encoded_bytes()
        .iter()
        .fold(SEED, |seed, byte| seed.wrapping_mul(131).wrapping_add(u64::from(*byte)))
}
fn generated(root: &Path, name: &str, nonempty: bool) -> PathBuf {
    let dir = root.join(name);
    let decks = if nonempty {
        "decks:\n  - name: Generic::Setup\n    notetype: basic\n    new: 1\n    due: 1\n    fields: [\"generic {n}\", \"back {n}\"]\n"
    } else {
        "decks: []\n"
    };
    let recipe = Recipe::parse(&format!(
        "recipe: setup\ndescription: Public synthetic setup\nnow: \"2025-01-01T00:00:00Z\"\n{decks}"
    ))
    .unwrap();
    generate_recipe(&recipe, seed(&dir), &dir).unwrap();
    dir
}
/// A collection as the new-collection bootstrap writes it (`storage/new-collection#B7`):
/// the Default deck and the stock note types as created, so no user content.
/// A recipe's collection is not one: the generator saves its note types.
fn fresh(root: &Path, name: &str) -> PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    meguri_engine::bootstrap::create(
        &dir.join("collection.anki2"),
        &FixedClock::new(ORIGIN, UtcOffset::UTC),
        &SeededRandom::new(seed(&dir)),
    )
    .unwrap();
    dir
}
fn open(dir: &Path, scope: &Scope) -> PublicationRig {
    PublicationRig::open(
        dir,
        scope,
        IdentityReads::Available(scope.account.clone()),
        false,
        ORIGIN,
        seed(dir),
        Keys::new(),
    )
    .unwrap()
}
fn service(root: &Path) -> Service<SeededRandom> {
    Service::open(root, Config::default(), SeededRandom::new(SEED)).unwrap()
}
fn cursor(service: &Service<SeededRandom>) -> u64 {
    service.request_log().unwrap().last().map_or(0, |r| r.request_id)
}
fn mutations_since(service: &Service<SeededRandom>, after: u64) -> usize {
    service
        .request_log()
        .unwrap()
        .iter()
        .filter(|r| {
            r.request_id > after
                && matches!(
                    r.kind,
                    RequestKind::CreateZone | RequestKind::DeleteZone | RequestKind::Modify
                )
        })
        .count()
}
/// A complete epoch in iCloud, published by another device from `nonempty`.
fn published(root: &Path, service: &mut Service<SeededRandom>, scope: &Scope, nonempty: bool) -> Vec<codec::Payload> {
    let dir = generated(root, "remote", nonempty);
    let mut publisher = open(&dir, scope);
    let preview = publisher.preview(service, STEPS).unwrap();
    publisher.confirm_publication(&preview).unwrap();
    publisher.drive_ready(service, STEPS).unwrap();
    assert!(publisher.facts().unwrap().recovery_ops.is_empty());
    let rows = publisher.driver().collection_rows().unwrap();
    publisher.close_clean().unwrap();
    rows
}
fn silent(row: SetupRow) -> SetupDecision {
    SetupDecision {
        row,
        question: SetupQuestion::None,
        reason: None,
        keep_device_offered: false,
        keep_cloud_offered: false,
    }
}
fn notes(rows: &[codec::Payload]) -> usize {
    rows.iter().filter(|p| p.kind() == codec::Kind::Note).count()
}
fn notetype(rows: &[codec::Payload]) -> i64 {
    rows.iter()
        .find_map(|p| match p {
            codec::Payload::Notetype(notetype) => Some(notetype.id),
            _ => None,
        })
        .unwrap()
}

/// Row 4: a collection, and no `MGActive`. No confirmation call is made.
#[test]
fn row_4_publishes_this_devices_collection_with_no_question() {
    for nonempty in [false, true] {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("row4");
        let mut rig = open(&generated(run.path(), "local", nonempty), &scope);
        let rows = rig.driver().collection_rows().unwrap();
        let preview = rig.preview(&mut service, STEPS).unwrap();
        assert_eq!(rig.setup_decision(&preview, UNKNOWN).unwrap(), silent(SetupRow::Publish));
        let op = rig.start_setup(&preview, UNKNOWN).unwrap();
        assert!(op.is_some());
        rig.drive_ready(&mut service, STEPS).unwrap();
        let facts = rig.facts().unwrap();
        assert!(facts.recovery_ops.is_empty());
        assert_eq!(facts.current.unwrap().facts.state, BindingState::Active);
        assert_eq!(rig.driver().collection_rows().unwrap(), rows);
        rig.close_clean().unwrap();
    }
}

/// Row 5: a local collection with no user content adopts the iCloud
/// collection with no confirmation call.
#[test]
fn row_5_replaces_a_collection_with_no_user_content_with_no_question() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("row5");
    let remote = published(run.path(), &mut service, &scope, true);
    let mut rig = open(&fresh(run.path(), "local"), &scope);
    let before = rig.driver().collection_rows().unwrap();
    assert_eq!(notes(&before), 0);
    let replaced = rig.facts().unwrap().decision.publication.identity;
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(
        rig.setup_decision(&preview, UNKNOWN).unwrap(),
        silent(SetupRow::AdoptOverNoContent)
    );
    assert!(rig.start_setup(&preview, UNKNOWN).unwrap().is_some());
    rig.drive_ready(&mut service, STEPS).unwrap();
    let facts = rig.facts().unwrap();
    assert!(facts.recovery_ops.is_empty());
    assert_eq!(facts.current.unwrap().facts.state, BindingState::Active);
    assert_eq!(rig.driver().collection_rows().unwrap(), remote);
    // V27.3 row 5, V22 invariant 6: the replaced collection is in the backups.
    let evidence = meguri_engine::device_state::DeviceState::for_collection(&run.path().join("local/collection.anki2"))
        .replacement_evidence_for(replaced)
        .unwrap()
        .unwrap();
    let backup = evidence.safety_backup.unwrap();
    assert_eq!(rig.driver().backup_collection_rows(&backup.name).unwrap(), before);
    rig.close_clean().unwrap();
}

/// V27.8: a note added between row 5's start and its exclusive phase cancels
/// the op with the collection unchanged and gives row 7.
#[test]
fn row_5_content_that_appears_before_the_replacement_cancels_it_and_asks() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("row5race");
    published(run.path(), &mut service, &scope, true);
    let mut rig = open(&fresh(run.path(), "local"), &scope);
    let mid = notetype(&rig.driver().collection_rows().unwrap());
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(
        rig.setup_decision(&preview, UNKNOWN).unwrap(),
        silent(SetupRow::AdoptOverNoContent)
    );
    rig.hold(Point::AdoptionBeforeActivate).unwrap();
    assert!(rig.start_setup(&preview, UNKNOWN).unwrap().is_some());
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::AdoptionBeforeActivate));
    // The proof was true when the op started; it is no longer.
    rig.driver().add_note(mid, 1, &["made meanwhile", "back"]).unwrap();
    let with_note = rig.driver().collection_rows().unwrap();
    assert_eq!(notes(&with_note), 1);
    let server = cursor(&service);
    rig.release(Point::AdoptionBeforeActivate).unwrap();
    let _ = rig.drive_ready(&mut service, STEPS);
    let facts = rig.facts().unwrap();
    assert!(facts.recovery_ops.is_empty(), "the op is cancelled, not waiting");
    assert!(facts.current.is_none_or(|b| b.facts.state != BindingState::Active));
    assert_eq!(rig.driver().collection_rows().unwrap(), with_note, "nothing was replaced");
    assert_eq!(mutations_since(&service, server), 0);
    // The flow starts again at the table, where row 7 now asks.
    let again = rig.preview(&mut service, STEPS).unwrap();
    let decision = rig.setup_decision(&again, UNKNOWN).unwrap();
    assert_eq!((decision.row, decision.question), (SetupRow::KeepCollection, SetupQuestion::Keep));
    assert!(decision.keep_device_offered && decision.keep_cloud_offered);
    assert!(rig.start_setup(&again, UNKNOWN).is_err());
    rig.close_clean().unwrap();
}

/// Row 7, and V27.8's "an epoch known only by `MGEpoch.counts` of zero is not
/// proved empty": two collections ask, whatever the manifest counts say.
#[test]
fn row_7_asks_and_manifest_counts_of_zero_never_stand_for_an_empty_icloud_collection() {
    for remote_nonempty in [true, false] {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("row7");
        published(run.path(), &mut service, &scope, remote_nonempty);
        let mut rig = open(&generated(run.path(), "local", true), &scope);
        let rows = rig.driver().collection_rows().unwrap();
        let server = cursor(&service);
        let preview = rig.preview(&mut service, STEPS).unwrap();
        if !remote_nonempty {
            assert_eq!(preview.cloud.as_ref().unwrap().counts[&codec::Kind::Note], 0);
        }
        assert_eq!(
            rig.setup_decision(&preview, UNKNOWN).unwrap(),
            SetupDecision {
                row: SetupRow::KeepCollection,
                question: SetupQuestion::Keep,
                reason: Some(AskReason::TwoCollections),
                keep_device_offered: true,
                keep_cloud_offered: true,
            }
        );
        // No start runs without the user's choice, and the preview stays for it.
        assert!(rig.start_setup(&preview, UNKNOWN).is_err());
        let facts = rig.facts().unwrap();
        assert!(facts.recovery_ops.is_empty() && facts.bindings.is_empty());
        assert_eq!(rig.driver().collection_rows().unwrap(), rows);
        assert_eq!(mutations_since(&service, server), 0);
        // The sheet's button is the confirmation, bound to the same preview.
        rig.adopt_cloud(&preview).unwrap();
        let op = rig.facts().unwrap().recovery_ops[0].id;
        rig.cancel_recovery_op(op).unwrap();
        rig.close_clean().unwrap();
    }
}

/// V27.1: a tracked media file is user content, so the device is asked.
#[test]
fn a_media_file_alone_makes_this_device_hold_user_content() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("media");
    published(run.path(), &mut service, &scope, true);
    let mut rig = open(&fresh(run.path(), "local"), &scope);
    // Without the file this device takes row 5.
    let empty = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(rig.setup_decision(&empty, UNKNOWN).unwrap(), silent(SetupRow::AdoptOverNoContent));
    rig.cancel_preview().unwrap();
    rig.driver().put_media("only-media.bin", b"abc").unwrap();
    let preview = rig.preview(&mut service, STEPS).unwrap();
    let decision = rig.setup_decision(&preview, UNKNOWN).unwrap();
    assert_eq!((decision.row, decision.question), (SetupRow::KeepCollection, SetupQuestion::Keep));
    assert!(rig.start_setup(&preview, UNKNOWN).is_err());
    assert_eq!(rig.driver().media_files().unwrap()["only-media.bin"], b"abc");
    rig.close_clean().unwrap();
}

/// V27.1: a note type that was saved (`mtime_secs` above 0) is user content
/// with no note at all. The recipe generator saves its note types.
#[test]
fn a_saved_note_type_alone_makes_this_device_hold_user_content() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("notetype");
    published(run.path(), &mut service, &scope, true);
    let mut rig = open(&generated(run.path(), "local", false), &scope);
    let rows = rig.driver().collection_rows().unwrap();
    assert_eq!(notes(&rows), 0);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    let decision = rig.setup_decision(&preview, UNKNOWN).unwrap();
    assert_eq!(
        (decision.row, decision.question, decision.reason),
        (SetupRow::KeepCollection, SetupQuestion::Keep, Some(AskReason::TwoCollections))
    );
    assert!(rig.start_setup(&preview, UNKNOWN).is_err());
    assert_eq!(rig.driver().collection_rows().unwrap(), rows);
    rig.close_clean().unwrap();
}

/// Row 3 and V27.6: a `stopped` binding resumes with no confirmation only
/// with a durable record that no other service was ready since the stop.
#[test]
fn row_3_resumes_a_stopped_binding_and_never_after_the_other_service() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("row3");
    let mut rig = open(&generated(run.path(), "local", true), &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.start_setup(&preview, UNKNOWN).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    let bound = rig.facts().unwrap().current.unwrap();
    rig.stop_using_icloud().unwrap();
    assert_eq!(
        rig.facts().unwrap().bindings[0].facts.state,
        BindingState::Isolated(IsolationReason::Stopped)
    );
    let server = cursor(&service);

    // After Meguri Sync was ready: not offered. Two collections, so it asks.
    let preview = rig.preview(&mut service, STEPS).unwrap();
    let after_other = rig.setup_decision(&preview, OtherServiceHistory::ReadySinceStop).unwrap();
    assert_eq!((after_other.row, after_other.question), (SetupRow::KeepCollection, SetupQuestion::Keep));
    assert!(rig.start_setup(&preview, OtherServiceHistory::ReadySinceStop).is_err());

    // No durable record: Resume keeps its confirmation.
    let unknown = rig.setup_decision(&preview, UNKNOWN).unwrap();
    assert_eq!((unknown.row, unknown.question), (SetupRow::Resume, SetupQuestion::ConfirmResume));
    assert!(rig.start_setup(&preview, UNKNOWN).is_err());
    assert_eq!(mutations_since(&service, server), 0);
    assert_eq!(
        rig.facts().unwrap().bindings[0].facts.state,
        BindingState::Isolated(IsolationReason::Stopped)
    );

    // Known history: the same binding continues, with no confirmation call.
    assert_eq!(
        rig.setup_decision(&preview, OtherServiceHistory::NoneSinceStop).unwrap(),
        silent(SetupRow::Resume)
    );
    assert_eq!(rig.start_setup(&preview, OtherServiceHistory::NoneSinceStop).unwrap(), None);
    rig.drive_ready(&mut service, STEPS).unwrap();
    let resumed = rig.facts().unwrap().current.unwrap();
    assert_eq!((resumed.id, resumed.facts.state), (bound.id, BindingState::Active));
    assert_eq!(resumed.facts.epoch, bound.facts.epoch);
    rig.close_clean().unwrap();
}

/// V27.6 as wired: a retained binding whose epoch `MGActive` no longer names
/// never leads to an upload without the sheet.
#[test]
fn a_retained_binding_with_no_active_epoch_is_asked_before_any_upload() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("retained");
    let mut rig = open(&generated(run.path(), "local", true), &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.start_setup(&preview, UNKNOWN).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    rig.stop_using_icloud().unwrap();
    rig.close_clean().unwrap();
    // The iCloud data is gone: a fresh service has no `MGActive`.
    let lost = tempfile::tempdir().unwrap();
    let mut service = self::service(lost.path());
    let mut rig = open(&run.path().join("local"), &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    for history in [UNKNOWN, OtherServiceHistory::NoneSinceStop, OtherServiceHistory::ReadySinceStop] {
        let decision = rig.setup_decision(&preview, history).unwrap();
        assert_eq!(
            (decision.row, decision.reason, decision.keep_device_offered, decision.keep_cloud_offered),
            (SetupRow::KeepCollection, Some(AskReason::RetainedBinding), true, false)
        );
        assert!(rig.start_setup(&preview, history).is_err());
    }
    assert_eq!(mutations_since(&service, 0), 0);
    rig.close_clean().unwrap();
}

const TRASHED: &[u8] = b"synthetic replaced media bytes";
const EVERY_START: [SilentStart; 4] = [
    SilentStart::Publication,
    SilentStart::AdoptionInitial,
    SilentStart::AdoptionOverNoContent,
    SilentStart::Resume,
];

fn tracking(dir: &Path, container: &str) -> TrackingStore {
    let scope = BuildScope {
        container: ContainerId::new(container).unwrap(),
        environment: Environment::Development,
    };
    match TrackingStore::open(dir, scope).unwrap() {
        OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
        _ => panic!("expected a supported tracking file"),
    }
}
fn unresolved_copy() -> ConflictCopy {
    ConflictCopy {
        id: CopyId([71; 16]),
        kind: ConflictKind::Note,
        key: OpaqueBytes(vec![71]),
        reason: ConflictReason::FieldConflict,
        losing_envelope: OpaqueBytes(vec![0, 255, 71]),
        losing_payload_digest: PayloadDigest([71; 32]),
        winning_payload_digest: PayloadDigest([99; 32]),
        field_names: Some(vec!["Front".into()]),
        template_names: None,
        note_deck: Some(1),
        staged_path: None,
        created_at_ms: ORIGIN,
        resolved_at_ms: None,
    }
}
fn media_store(dir: &Path) -> meguri_storage::media::MediaStore {
    meguri_storage::media::MediaStore::open(
        &dir.join("collection.anki2"),
        Arc::new(FixedClock::new(ORIGIN, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(SEED)),
    )
    .unwrap()
}
/// One media file that this device moved to the media trash before any setup.
fn plant_trash(dir: &Path, scope: &Scope) {
    let mut rig = open(dir, scope);
    rig.driver().put_media("replaced.bin", TRASHED).unwrap();
    rig.close_clean().unwrap();
    media_store(dir).trash(&["replaced.bin".into()]).unwrap();
    assert_trash_kept(dir);
    let mut rig = open(dir, scope);
    assert!(rig.driver().media_files().unwrap().is_empty());
    rig.close_clean().unwrap();
}
/// One unresolved conflict copy. With no binding named, it belongs to a
/// stopped binding of another build's container, which no setup row reads.
fn plant_conflict(dir: &Path, scope: &Scope, binding: Option<BindingId>) {
    match binding {
        Some(binding) => tracking(dir, &scope.container).store_conflict(binding, &unresolved_copy()).unwrap(),
        None => {
            let mut other = tracking(dir, "test.meguri.setup.another-build");
            let binding = other
                .create_binding(
                    BindingFacts {
                        account: OpaqueBytes(b"synthetic-other-build-account".to_vec()),
                        epoch: EpochId([72; 16]),
                        record_key: RecordKey([73; 32]),
                        publication: serde_json::from_str("[17,29]").unwrap(),
                        state: BindingState::Isolated(IsolationReason::Stopped),
                    },
                    &FixedClock::new(ORIGIN, UtcOffset::UTC),
                    &SeededRandom::new(SEED),
                )
                .unwrap();
            other.store_conflict(binding.id, &unresolved_copy()).unwrap()
        }
    };
    assert_conflict_kept(dir, scope);
}
fn assert_trash_kept(dir: &Path) {
    let media = media_store(dir);
    let entries = media.trash_entries().unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "replaced.bin" && entry.size == TRASHED.len() as u64),
        "the media trash entry is listed"
    );
    fn holds(directory: &Path) -> bool {
        std::fs::read_dir(directory).unwrap().any(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() { holds(&path) } else { std::fs::read(path).unwrap() == TRASHED }
        })
    }
    assert!(holds(media.trash_dir()), "the trashed bytes are on disk");
}
fn assert_conflict_kept(dir: &Path, scope: &Scope) {
    let copies = tracking(dir, &scope.container).conflict_copies().unwrap();
    assert_eq!(copies.len(), 1);
    assert!(copies[0].copy == unresolved_copy(), "the unresolved copy is unchanged");
}
/// A collection bound to iCloud by row 4 and then switched Off, closed.
fn stopped(dir: &Path, scope: &Scope, service: &mut Service<SeededRandom>) -> BindingId {
    let mut rig = open(dir, scope);
    let preview = rig.preview(service, STEPS).unwrap();
    rig.start_setup(&preview, UNKNOWN).unwrap();
    rig.drive_ready(service, STEPS).unwrap();
    let bound = rig.facts().unwrap().current.unwrap().id;
    rig.stop_using_icloud().unwrap();
    rig.close_clean().unwrap();
    bound
}

/// V22 invariant 8 and V27.7 through row 4: the no-question publication
/// deletes neither a media trash entry nor an unresolved conflict copy.
#[test]
fn row_4_keeps_a_media_trash_entry_and_an_unresolved_conflict_copy() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("keep4");
    let dir = generated(run.path(), "local", true);
    plant_trash(&dir, &scope);
    plant_conflict(&dir, &scope, None);
    let mut rig = open(&dir, &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(rig.setup_decision(&preview, UNKNOWN).unwrap(), silent(SetupRow::Publish));
    assert!(rig.start_setup(&preview, UNKNOWN).unwrap().is_some());
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(rig.facts().unwrap().current.unwrap().facts.state, BindingState::Active);
    rig.close_clean().unwrap();
    assert_trash_kept(&dir);
    assert_conflict_kept(&dir, &scope);
}

/// The same through row 5, where this device's collection is replaced.
#[test]
fn row_5_keeps_a_media_trash_entry_and_an_unresolved_conflict_copy() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("keep5");
    let remote = published(run.path(), &mut service, &scope, true);
    let dir = fresh(run.path(), "local");
    plant_trash(&dir, &scope);
    plant_conflict(&dir, &scope, None);
    let mut rig = open(&dir, &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(
        rig.setup_decision(&preview, UNKNOWN).unwrap(),
        silent(SetupRow::AdoptOverNoContent)
    );
    assert!(rig.start_setup(&preview, UNKNOWN).unwrap().is_some());
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(rig.facts().unwrap().current.unwrap().facts.state, BindingState::Active);
    assert_eq!(rig.driver().collection_rows().unwrap(), remote);
    rig.close_clean().unwrap();
    assert_trash_kept(&dir);
    assert_conflict_kept(&dir, &scope);
}

/// The same when row 5's re-test cancels the replacement.
#[test]
fn a_cancelled_row_5_keeps_a_media_trash_entry_and_an_unresolved_conflict_copy() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("keep5cancel");
    published(run.path(), &mut service, &scope, true);
    let dir = fresh(run.path(), "local");
    plant_trash(&dir, &scope);
    plant_conflict(&dir, &scope, None);
    let mut rig = open(&dir, &scope);
    let mid = notetype(&rig.driver().collection_rows().unwrap());
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::AdoptionBeforeActivate).unwrap();
    assert!(rig.start_setup(&preview, UNKNOWN).unwrap().is_some());
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::AdoptionBeforeActivate));
    rig.driver().add_note(mid, 1, &["made meanwhile", "back"]).unwrap();
    rig.release(Point::AdoptionBeforeActivate).unwrap();
    let _ = rig.drive_ready(&mut service, STEPS);
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
    assert_eq!(notes(&rig.driver().collection_rows().unwrap()), 1);
    rig.close_clean().unwrap();
    assert_trash_kept(&dir);
    assert_conflict_kept(&dir, &scope);
}

/// The same through row 3, with the copy on the binding that resumes.
#[test]
fn row_3_keeps_a_media_trash_entry_and_an_unresolved_conflict_copy() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("keep3");
    let dir = generated(run.path(), "local", true);
    plant_trash(&dir, &scope);
    let bound = stopped(&dir, &scope, &mut service);
    plant_conflict(&dir, &scope, Some(bound));
    let mut rig = open(&dir, &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(
        rig.setup_decision(&preview, OtherServiceHistory::NoneSinceStop).unwrap(),
        silent(SetupRow::Resume)
    );
    assert_eq!(rig.start_setup(&preview, OtherServiceHistory::NoneSinceStop).unwrap(), None);
    rig.drive_ready(&mut service, STEPS).unwrap();
    let resumed = rig.facts().unwrap().current.unwrap();
    assert_eq!((resumed.id, resumed.facts.state), (bound, BindingState::Active));
    rig.close_clean().unwrap();
    assert_trash_kept(&dir);
    assert_conflict_kept(&dir, &scope);
}

/// Each no-question start, called directly, starts nothing while the core's
/// decision for that preview is a question. The preview stays for the sheet.
fn refuses_every_start(
    rig: &mut PublicationRig,
    service: &Service<SeededRandom>,
    preview: &meguri_testkit::cloud::coordinator::ConnectionPreview,
    history: OtherServiceHistory,
) {
    let decision = rig.setup_decision(preview, history).unwrap();
    assert_ne!(decision.question, SetupQuestion::None);
    assert_eq!(decision.silent_start(), None);
    let rows = rig.driver().collection_rows().unwrap();
    let states = |rig: &PublicationRig| -> Vec<_> {
        let facts = rig.facts().unwrap();
        assert!(facts.recovery_ops.is_empty(), "no operation was recorded");
        facts.bindings.iter().map(|b| (b.id, b.facts.state)).collect()
    };
    let before = states(rig);
    let server = cursor(service);
    for start in EVERY_START {
        assert!(rig.force_setup_start(preview, history, start).is_err(), "{start:?} for {decision:?}");
        assert_eq!(states(rig), before, "{start:?}");
    }
    assert!(rig.start_setup(preview, history).is_err());
    assert_eq!(states(rig), before);
    assert_eq!(mutations_since(service, server), 0);
    assert_eq!(rig.driver().collection_rows().unwrap(), rows);
    // Nothing consumed the preview: the same question is still its answer.
    assert_eq!(rig.setup_decision(preview, history).unwrap(), decision);
}

#[test]
fn no_start_runs_while_the_decision_is_a_question() {
    // Row 7: two collections.
    {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("ask7");
        published(run.path(), &mut service, &scope, true);
        let mut rig = open(&generated(run.path(), "local", true), &scope);
        let preview = rig.preview(&mut service, STEPS).unwrap();
        for history in [UNKNOWN, OtherServiceHistory::NoneSinceStop, OtherServiceHistory::ReadySinceStop] {
            refuses_every_start(&mut rig, &service, &preview, history);
        }
        rig.close_clean().unwrap();
    }
    // Row 3 with its confirmation kept, and Resume not offered after the other service.
    {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("ask3");
        let dir = generated(run.path(), "local", true);
        stopped(&dir, &scope, &mut service);
        let mut rig = open(&dir, &scope);
        let preview = rig.preview(&mut service, STEPS).unwrap();
        for history in [UNKNOWN, OtherServiceHistory::ReadySinceStop] {
            refuses_every_start(&mut rig, &service, &preview, history);
        }
        rig.close_clean().unwrap();
    }
    // A retained binding whose iCloud data is gone.
    {
        let run = tempfile::tempdir().unwrap();
        let mut first = service(run.path());
        let scope = scope("asklost");
        let dir = generated(run.path(), "local", true);
        stopped(&dir, &scope, &mut first);
        let lost = tempfile::tempdir().unwrap();
        let mut service = service(lost.path());
        let mut rig = open(&dir, &scope);
        let preview = rig.preview(&mut service, STEPS).unwrap();
        for history in [UNKNOWN, OtherServiceHistory::NoneSinceStop, OtherServiceHistory::ReadySinceStop] {
            refuses_every_start(&mut rig, &service, &preview, history);
        }
        rig.close_clean().unwrap();
    }
    // A device whose only content is one media file, and an iCloud collection.
    {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("askmedia");
        published(run.path(), &mut service, &scope, true);
        let mut rig = open(&fresh(run.path(), "local"), &scope);
        rig.driver().put_media("only-media.bin", b"abc").unwrap();
        let preview = rig.preview(&mut service, STEPS).unwrap();
        refuses_every_start(&mut rig, &service, &preview, UNKNOWN);
        rig.close_clean().unwrap();
    }
}

/// The rig's child process is this test binary, started at this test.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
