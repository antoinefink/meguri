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
use meguri_engine::device_state::{PendingDecisionReason, PublicationDecision};
use meguri_sync::cloud::{
    BindingFacts, BindingId, BindingState, BuildScope, ConflictCopy, ConflictKind, ConflictReason, ContainerId, CopyId,
    Environment, EpochId, IsolationReason, OpaqueBytes, OpenOutcome, PayloadDigest, RecordKey, TrackingStore,
    adoption::{RecoveryKind, RecoveryStep},
    entity_codec as codec,
    publication::ConnectDecision,
    runtime::{KeyByteSource, KeySourceError},
    setup::{AskReason, OtherServiceHistory, SetupDecision, SetupQuestion, SetupRow, SilentStart},
};
use meguri_testkit::{
    cloud::{
        Config, Record, RequestKind, RequestedFields, Scope, Service,
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
    // Native adoption fetches ask for at most 32 records a page.
    let config = Config {
        page_size: 32,
        ..Config::default()
    };
    Service::open(root, config, SeededRandom::new(SEED)).unwrap()
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

const EVERY_HISTORY: [OtherServiceHistory; 3] = [
    OtherServiceHistory::Unknown,
    OtherServiceHistory::NoneSinceStop,
    OtherServiceHistory::ReadySinceStop,
];
/// Every record of every zone that iCloud holds for this scope.
fn server_records(service: &Service<SeededRandom>, scope: &Scope) -> std::collections::BTreeMap<String, Vec<Record>> {
    service
        .inspect(scope)
        .unwrap()
        .unwrap()
        .zones
        .into_iter()
        .map(|(name, zone)| (name, zone.records.into_values().collect()))
        .collect()
}
/// Another device publishes its collection as the active epoch, over any epoch there.
fn publish_from_another_device(
    root: &Path,
    name: &str,
    service: &mut Service<SeededRandom>,
    scope: &Scope,
) -> Vec<codec::Payload> {
    let mut publisher = open(&generated(root, name, true), scope);
    let preview = publisher.preview(service, STEPS).unwrap();
    publisher.confirm_publication(&preview).unwrap();
    publisher.drive_ready(service, STEPS).unwrap();
    assert!(publisher.facts().unwrap().recovery_ops.is_empty());
    let rows = publisher.driver().collection_rows().unwrap();
    publisher.close_clean().unwrap();
    rows
}
fn asks(decision: &SetupDecision) -> (SetupRow, SetupQuestion, Option<AskReason>, bool, bool) {
    (
        decision.row,
        decision.question,
        decision.reason,
        decision.keep_device_offered,
        decision.keep_cloud_offered,
    )
}

/// V27.6, "not after the other service", with V27.4: the sheet that replaces
/// Resume has two working buttons. "Keep the iCloud collection" is V14 with
/// its safety backup; "Keep this device's collection" is V15. Neither is
/// admitted for that preview while the decision is still Resume.
#[test]
fn after_the_other_service_both_buttons_of_the_sheet_work_over_a_binding_that_could_resume() {
    for keep_cloud in [true, false] {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("bothbuttons");
        let dir = generated(run.path(), "local", true);
        let bound = stopped(&dir, &scope, &mut service);
        let mut rig = open(&dir, &scope);
        let in_icloud = rig.driver().collection_rows().unwrap();
        let epoch = rig.facts().unwrap().bindings[0].facts.epoch;
        // An edit made while the service was Off: the two collections differ.
        let mid = notetype(&in_icloud);
        rig.driver().add_note(mid, 1, &["made while off", "back"]).unwrap();
        let on_device = rig.driver().collection_rows().unwrap();
        assert_eq!(notes(&on_device), notes(&in_icloud) + 1);
        let replaced = rig.facts().unwrap().decision.publication.identity;
        let preview = rig.preview(&mut service, STEPS).unwrap();
        assert!(preview.decision == ConnectDecision::ConfirmResume);
        assert_eq!(
            asks(&rig.setup_decision(&preview, OtherServiceHistory::ReadySinceStop).unwrap()),
            (SetupRow::KeepCollection, SetupQuestion::Keep, Some(AskReason::TwoCollections), true, true)
        );
        // While the decision is Resume, with or without its confirmation,
        // neither button of the sheet is offered and nothing changes. The
        // plain confirmation is the publication with the history unknown.
        let server = cursor(&service);
        assert!(rig.confirm_publication(&preview).is_err());
        for history in [OtherServiceHistory::Unknown, OtherServiceHistory::NoneSinceStop] {
            assert_eq!(rig.setup_decision(&preview, history).unwrap().row, SetupRow::Resume);
            assert!(rig.keep_cloud_over_resumable(&preview, history).is_err(), "{history:?}");
            assert!(rig.keep_device_over_active(&preview, history).is_err(), "{history:?}");
            let facts = rig.facts().unwrap();
            assert!(facts.recovery_ops.is_empty());
            assert_eq!(
                facts.bindings.iter().map(|b| (b.id, b.facts.state)).collect::<Vec<_>>(),
                [(bound, BindingState::Isolated(IsolationReason::Stopped))]
            );
        }
        assert_eq!(mutations_since(&service, server), 0);
        assert_eq!(rig.driver().collection_rows().unwrap(), on_device);
        let directory = run.path().join("local/collection.anki2");
        let evidence = || {
            meguri_engine::device_state::DeviceState::for_collection(&directory)
                .replacement_evidence_for(replaced)
                .unwrap()
        };
        assert!(evidence().is_none());

        if keep_cloud {
            let op = rig
                .keep_cloud_over_resumable(&preview, OtherServiceHistory::ReadySinceStop)
                .unwrap();
            let recorded = rig.facts().unwrap();
            assert_eq!(recorded.recovery_ops.len(), 1);
            assert_eq!(recorded.recovery_ops[0].id, op);
            assert_eq!(recorded.recovery_ops[0].epoch, epoch);
            let RecoveryKind::Adopt(adopt) = &recorded.recovery_ops[0].kind else {
                panic!("the button is V14")
            };
            // The stopped binding is the op's binding before, state unchanged.
            let before = adopt.binding_before.as_ref().unwrap();
            assert_eq!((before.id, before.state), (bound, BindingState::Isolated(IsolationReason::Stopped)));
            // The confirmation consumed the preview: the other button is gone.
            assert!(rig.confirm_publication(&preview).is_err());
            assert!(rig.setup_decision(&preview, OtherServiceHistory::ReadySinceStop).is_err());
            rig.drive_ready(&mut service, STEPS).unwrap();
            let facts = rig.facts().unwrap();
            assert!(facts.recovery_ops.is_empty());
            let current = facts.current.unwrap();
            assert_eq!((current.facts.state, current.facts.epoch), (BindingState::Active, epoch));
            assert_ne!(current.id, bound);
            assert_eq!(rig.driver().collection_rows().unwrap(), in_icloud);
            // V14.2 step 7: the collection that was on this device is backed up.
            let backup = evidence().unwrap().safety_backup.unwrap();
            assert_eq!(rig.driver().backup_collection_rows(&backup.name).unwrap(), on_device);
        } else {
            let before = server_records(&service, &scope);
            rig.keep_device_over_active(&preview, OtherServiceHistory::ReadySinceStop).unwrap();
            assert!(rig.keep_cloud_over_resumable(&preview, OtherServiceHistory::ReadySinceStop).is_err());
            rig.drive_ready(&mut service, STEPS).unwrap();
            let facts = rig.facts().unwrap();
            assert!(facts.recovery_ops.is_empty());
            let current = facts.current.unwrap();
            assert_eq!(current.facts.state, BindingState::Active);
            assert_ne!(current.facts.epoch, epoch);
            assert_eq!(rig.driver().collection_rows().unwrap(), on_device);
            // V22 invariant 6: the replaced epoch's zone stays in iCloud.
            let after = server_records(&service, &scope);
            let old_zone = meguri_sync::cloud::adapter::collection_zone(epoch);
            assert!(after[&old_zone] == before[&old_zone]);
        }
        rig.close_clean().unwrap();
    }
}

/// V27.4 lists an isolated binding under "shown for": a device proved to hold
/// no user content does not take row 5 over a binding whose epoch was replaced.
#[test]
fn row_5_is_not_taken_over_a_stopped_binding_whose_epoch_was_replaced() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("row5retained");
    let dir = fresh(run.path(), "local");
    let bound = stopped(&dir, &scope, &mut service);
    publish_from_another_device(run.path(), "remote", &mut service, &scope);
    let mut rig = open(&dir, &scope);
    let rows = rig.driver().collection_rows().unwrap();
    assert_eq!(notes(&rows), 0);
    let server = cursor(&service);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert!(preview.decision != ConnectDecision::ConfirmResume);
    for history in EVERY_HISTORY {
        assert_eq!(
            asks(&rig.setup_decision(&preview, history).unwrap()),
            (SetupRow::KeepCollection, SetupQuestion::Keep, Some(AskReason::RetainedBinding), true, true)
        );
        refuses_every_start(&mut rig, &service, &preview, history);
    }
    assert_eq!(mutations_since(&service, server), 0);
    assert!(rig.facts().unwrap().bindings.iter().any(|b| b.id == bound));
    // The sheet's button is the confirmation, bound to the same preview.
    let op = rig.adopt_cloud(&preview).unwrap();
    rig.cancel_recovery_op(op).unwrap();
    assert_eq!(rig.driver().collection_rows().unwrap(), rows);
    rig.close_clean().unwrap();
}

/// V27.8: an `MGActive` that appears after row 4's preview read none gives
/// V15.2 step 5. The other device's records are all as it left them.
#[test]
fn row_4_ends_in_a_switch_conflict_when_another_device_published_after_the_preview() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("row4race");
    let mut rig = open(&generated(run.path(), "local", true), &scope);
    let rows = rig.driver().collection_rows().unwrap();
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(rig.setup_decision(&preview, UNKNOWN).unwrap(), silent(SetupRow::Publish));
    publish_from_another_device(run.path(), "remote", &mut service, &scope);
    let theirs = server_records(&service, &scope);
    assert_eq!(theirs["meguri-root"].len(), 1);
    // The decision was true of what the preview read, so the start is admitted.
    let op = rig.start_setup(&preview, UNKNOWN).unwrap().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    let facts = rig.facts().unwrap();
    assert_eq!(facts.recovery_ops.len(), 1);
    assert_eq!(facts.recovery_ops[0].id, op);
    assert!(matches!(facts.recovery_ops[0].step, RecoveryStep::Publish(_)));
    assert!(facts.recovery_ops[0].switch_conflict.is_some(), "the conditional save lost");
    let after = server_records(&service, &scope);
    assert!(after["meguri-root"] == theirs["meguri-root"], "MGActive still names their epoch");
    for (zone, records) in &theirs {
        assert!(&after[zone] == records, "{zone} is as the other device left it");
    }
    assert_eq!(rig.driver().collection_rows().unwrap(), rows);
    // Nothing more is sent to the root zone while the conflict waits for the user.
    let stopped_at = cursor(&service);
    rig.advance_clock_ms(24 * 60 * 60 * 1000).unwrap();
    rig.sync_now_and_drain(&mut service, STEPS).unwrap();
    assert!(!service.request_log().unwrap().iter().any(|r| r.request_id > stopped_at
        && r.kind == RequestKind::Modify
        && r.zone.as_deref() == Some("meguri-root")));
    assert!(server_records(&service, &scope)["meguri-root"] == theirs["meguri-root"]);
    rig.close_clean().unwrap();
}

/// V14.3: a process killed while row 5's op waits at `ready` replaces
/// nothing, and no relaunch continues it. Content made afterwards is asked
/// about by row 7; the op's own condition is never what protects it then.
#[test]
fn a_killed_row_5_is_not_continued_after_relaunch_and_content_made_meanwhile_is_asked_about() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("row5kill");
    published(run.path(), &mut service, &scope, true);
    let dir = fresh(run.path(), "local");
    let mut rig = open(&dir, &scope);
    let before = rig.driver().collection_rows().unwrap();
    let mid = notetype(&before);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(
        rig.setup_decision(&preview, UNKNOWN).unwrap(),
        silent(SetupRow::AdoptOverNoContent)
    );
    rig.hold(Point::AdoptionBeforeActivate).unwrap();
    let op = rig.start_setup(&preview, UNKNOWN).unwrap().unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::AdoptionBeforeActivate));
    let ready = rig.facts().unwrap();
    assert_eq!(ready.recovery_ops.len(), 1);
    assert_eq!(ready.recovery_ops[0].id, op);
    assert!(!rig.kill().unwrap().clean_close);
    let server = cursor(&service);

    let mut rig = open(&dir, &scope);
    let relaunched = rig.facts().unwrap();
    assert!(relaunched.recovery_ops.is_empty(), "the op did not survive the relaunch");
    assert!(relaunched.current.is_none_or(|b| b.facts.state != BindingState::Active));
    assert_eq!(rig.driver().collection_rows().unwrap(), before);
    rig.driver().add_note(mid, 1, &["made after the relaunch", "back"]).unwrap();
    let with_note = rig.driver().collection_rows().unwrap();
    assert_eq!(notes(&with_note), 1);
    // Nothing is left to drive: the staged iCloud collection is never activated.
    let _ = rig.drive_ready(&mut service, STEPS);
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
    assert_eq!(rig.driver().collection_rows().unwrap(), with_note, "nothing was replaced");
    assert_eq!(mutations_since(&service, server), 0);
    let again = rig.preview(&mut service, STEPS).unwrap();
    let decision = rig.setup_decision(&again, UNKNOWN).unwrap();
    assert_eq!((decision.row, decision.question), (SetupRow::KeepCollection, SetupQuestion::Keep));
    assert!(decision.keep_device_offered && decision.keep_cloud_offered);
    refuses_every_start(&mut rig, &service, &again, UNKNOWN);
    assert_eq!(rig.driver().collection_rows().unwrap(), with_note);
    rig.close_clean().unwrap();
}

/// V27.1 at the replace itself: a media file added between row 5's start and
/// its exclusive phase is user content, so the op is cancelled and the file stays.
#[test]
fn row_5_a_media_file_that_appears_before_the_replacement_cancels_it_and_asks() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("row5media");
    published(run.path(), &mut service, &scope, true);
    let mut rig = open(&fresh(run.path(), "local"), &scope);
    let before = rig.driver().collection_rows().unwrap();
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert_eq!(
        rig.setup_decision(&preview, UNKNOWN).unwrap(),
        silent(SetupRow::AdoptOverNoContent)
    );
    rig.hold(Point::AdoptionBeforeActivate).unwrap();
    assert!(rig.start_setup(&preview, UNKNOWN).unwrap().is_some());
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::AdoptionBeforeActivate));
    rig.driver().put_media("made-meanwhile.bin", b"media made meanwhile").unwrap();
    let server = cursor(&service);
    rig.release(Point::AdoptionBeforeActivate).unwrap();
    let _ = rig.drive_ready(&mut service, STEPS);
    let facts = rig.facts().unwrap();
    assert!(facts.recovery_ops.is_empty(), "the op is cancelled, not waiting");
    assert!(facts.current.is_none_or(|b| b.facts.state != BindingState::Active));
    assert_eq!(rig.driver().collection_rows().unwrap(), before, "nothing was replaced");
    assert_eq!(
        rig.driver().media_files().unwrap()["made-meanwhile.bin"],
        b"media made meanwhile"
    );
    assert_eq!(mutations_since(&service, server), 0);
    let again = rig.preview(&mut service, STEPS).unwrap();
    let decision = rig.setup_decision(&again, UNKNOWN).unwrap();
    assert_eq!((decision.row, decision.question), (SetupRow::KeepCollection, SetupQuestion::Keep));
    assert!(rig.start_setup(&again, UNKNOWN).is_err());
    assert_eq!(
        rig.driver().media_files().unwrap()["made-meanwhile.bin"],
        b"media made meanwhile"
    );
    rig.close_clean().unwrap();
}

/// V27.4 and V12.2 through the runtime: a pending fence shows the sheet
/// whatever else holds, and no start runs before a button.
#[test]
fn a_pending_fence_shows_the_sheet_and_starts_nothing() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("fence");
    let mut rig = open(&generated(run.path(), "local", true), &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.start_setup(&preview, UNKNOWN).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(rig.facts().unwrap().current.unwrap().facts.state, BindingState::Active);
    // A restore sets the fence (V12.1).
    let backup = rig.driver().backup_now().unwrap();
    rig.driver().restore_backup(&backup).unwrap();
    let pending = rig.facts().unwrap().decision;
    assert!(pending.publication.decision == PublicationDecision::Pending);
    assert_eq!(pending.pending_reason, Some(PendingDecisionReason::Restore));
    let theirs = server_records(&service, &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    for history in EVERY_HISTORY {
        assert_eq!(
            asks(&rig.setup_decision(&preview, history).unwrap()),
            (SetupRow::KeepCollection, SetupQuestion::Keep, Some(AskReason::Fence), true, true)
        );
        let decision = rig.setup_decision(&preview, history).unwrap();
        assert_eq!(decision.silent_start(), None);
        let server = cursor(&service);
        for start in EVERY_START {
            assert!(rig.force_setup_start(&preview, history, start).is_err(), "{start:?} {history:?}");
        }
        assert!(rig.start_setup(&preview, history).is_err());
        assert!(rig.facts().unwrap().recovery_ops.is_empty());
        assert_eq!(mutations_since(&service, server), 0);
    }
    assert!(server_records(&service, &scope) == theirs);
    assert!(rig.facts().unwrap().decision.publication.decision == PublicationDecision::Pending);
    rig.close_clean().unwrap();
}

/// V27.6, "another account" (R2055): with a binding of another account and
/// nothing in the new account, nothing is published before the sheet.
#[test]
fn a_binding_of_another_account_is_asked_before_anything_reaches_the_new_account() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let first = scope("account");
    let dir = generated(run.path(), "local", true);
    stopped(&dir, &first, &mut service);
    let second = Scope {
        account: "synthetic-setup-other-account".into(),
        ..first.clone()
    };
    let mut rig = PublicationRig::open(
        &dir,
        &second,
        IdentityReads::Available(second.account.clone()),
        false,
        ORIGIN,
        seed(&dir),
        Keys::new(),
    )
    .unwrap();
    let server = cursor(&service);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    // The new account holds no collection: without the binding this is row 4.
    assert!(preview.cloud.is_none());
    for history in EVERY_HISTORY {
        assert_eq!(
            asks(&rig.setup_decision(&preview, history).unwrap()),
            (SetupRow::KeepCollection, SetupQuestion::Keep, Some(AskReason::OtherAccount), true, false)
        );
        refuses_every_start(&mut rig, &service, &preview, history);
    }
    assert_eq!(mutations_since(&service, server), 0);
    assert!(service.inspect(&second).unwrap().is_none_or(|account| account.zones.is_empty()));
    rig.close_clean().unwrap();
}

/// An `MGActive` that cannot be decoded proves nothing about iCloud: the
/// sheet, with no iCloud collection to keep, for either kind of device.
#[test]
fn an_unreadable_active_record_is_asked_and_offers_no_icloud_collection() {
    for local_has_content in [false, true] {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("unreadable");
        published(run.path(), &mut service, &scope, true);
        let dir = if local_has_content {
            generated(run.path(), "local", true)
        } else {
            fresh(run.path(), "local")
        };
        let mut rig = open(&dir, &scope);
        let theirs = server_records(&service, &scope);
        // The actual named-field read: the record is found, its `e` is not.
        let preview = rig
            .preview_with_active_fields(&mut service, RequestedFields::Named(vec![]), STEPS)
            .unwrap();
        assert!(preview.decision == ConnectDecision::CloudUnreadable);
        for history in EVERY_HISTORY {
            assert_eq!(
                asks(&rig.setup_decision(&preview, history).unwrap()),
                (SetupRow::KeepCollection, SetupQuestion::Keep, Some(AskReason::CloudUnreadable), true, false)
            );
            refuses_every_start(&mut rig, &service, &preview, history);
        }
        // No iCloud collection is offered, and none can be kept.
        assert!(rig.adopt_cloud(&preview).is_err());
        assert!(rig.facts().unwrap().recovery_ops.is_empty());
        assert!(server_records(&service, &scope) == theirs);
        // This device's collection is offered, so its confirmation records
        // the publication that may replace the unreadable record (V27.4).
        let op = rig.confirm_publication(&preview).unwrap();
        assert_eq!(rig.facts().unwrap().recovery_ops[0].id, op);
        // The confirmation records the op; it sends nothing.
        assert!(server_records(&service, &scope) == theirs);
        rig.cancel_recovery_op(op).unwrap();
        rig.close_clean().unwrap();
    }
}

/// Nothing was recorded or sent, and the collection and its binding states
/// are as they were.
fn assert_unchanged(
    rig: &mut PublicationRig,
    service: &Service<SeededRandom>,
    server: u64,
    rows: &[codec::Payload],
    states: &[(BindingId, BindingState)],
) {
    let facts = rig.facts().unwrap();
    assert!(facts.recovery_ops.is_empty(), "no operation was recorded");
    assert_eq!(facts.bindings.iter().map(|b| (b.id, b.facts.state)).collect::<Vec<_>>(), states);
    assert_eq!(mutations_since(service, server), 0);
    assert_eq!(rig.driver().collection_rows().unwrap(), rows);
}

/// V27.6 with V13.3: while the core's decision is Resume and its
/// confirmation, no sheet offers "Keep this device's collection", so the
/// publication confirmation over that `MGActive` is refused and records
/// nothing. The preview stays for the question that was asked.
#[test]
fn the_publication_is_not_offered_while_the_decision_is_resume() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("noresumepublish");
    let dir = generated(run.path(), "local", true);
    let bound = stopped(&dir, &scope, &mut service);
    let mut rig = open(&dir, &scope);
    let rows = rig.driver().collection_rows().unwrap();
    let identity = rig.facts().unwrap().decision.publication.identity;
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert!(preview.decision == ConnectDecision::ConfirmResume);
    let decision = rig.setup_decision(&preview, UNKNOWN).unwrap();
    assert_eq!((decision.row, decision.question), (SetupRow::Resume, SetupQuestion::ConfirmResume));
    let server = cursor(&service);
    let stopped_state = [(bound, BindingState::Isolated(IsolationReason::Stopped))];
    assert!(rig.confirm_publication(&preview).is_err());
    assert_unchanged(&mut rig, &service, server, &rows, &stopped_state);
    for history in [OtherServiceHistory::Unknown, OtherServiceHistory::NoneSinceStop] {
        assert!(rig.keep_device_over_active(&preview, history).is_err(), "{history:?}");
        assert_unchanged(&mut rig, &service, server, &rows, &stopped_state);
    }
    assert_eq!(rig.facts().unwrap().decision.publication.identity, identity);
    // The refusals consumed nothing: the same preview continues the binding.
    assert_eq!(rig.start_setup(&preview, OtherServiceHistory::NoneSinceStop).unwrap(), None);
    rig.drive_ready(&mut service, STEPS).unwrap();
    let resumed = rig.facts().unwrap().current.unwrap();
    assert_eq!((resumed.id, resumed.facts.state), (bound, BindingState::Active));
    rig.close_clean().unwrap();
}

/// Row 5 downloads with no question, so no sheet offers this device's
/// collection either: publishing a collection proved to hold no user
/// content over the iCloud collection is refused, whatever the history.
#[test]
fn the_publication_is_not_offered_where_row_5_downloads_with_no_question() {
    let run = tempfile::tempdir().unwrap();
    let mut service = service(run.path());
    let scope = scope("norow5publish");
    published(run.path(), &mut service, &scope, true);
    let mut rig = open(&fresh(run.path(), "local"), &scope);
    let rows = rig.driver().collection_rows().unwrap();
    let theirs = server_records(&service, &scope);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert!(preview.decision == ConnectDecision::ConfirmAdoption);
    let server = cursor(&service);
    assert!(rig.confirm_publication(&preview).is_err());
    assert_unchanged(&mut rig, &service, server, &rows, &[]);
    for history in EVERY_HISTORY {
        assert_eq!(rig.setup_decision(&preview, history).unwrap(), silent(SetupRow::AdoptOverNoContent));
        assert!(rig.keep_device_over_active(&preview, history).is_err(), "{history:?}");
        assert_unchanged(&mut rig, &service, server, &rows, &[]);
    }
    assert!(server_records(&service, &scope) == theirs);
    // The preview is still the row's: its no-question start runs.
    let op = rig.start_setup(&preview, UNKNOWN).unwrap().unwrap();
    assert_eq!(rig.facts().unwrap().recovery_ops[0].id, op);
    rig.cancel_recovery_op(op).unwrap();
    rig.close_clean().unwrap();
}

/// The confirmed flows the app uses still publish: the connect sheet with no
/// `MGActive` (`ConfirmUpload`) and "Replace iCloud" on the publication
/// choice (`ChooseCollection`), where the core's decision is the sheet with
/// this device's collection offered.
#[test]
fn the_confirmed_publication_of_the_existing_sheets_still_publishes() {
    for active in [false, true] {
        let run = tempfile::tempdir().unwrap();
        let mut service = service(run.path());
        let scope = scope("confirmedpublish");
        if active {
            published(run.path(), &mut service, &scope, true);
        }
        let mut rig = open(&generated(run.path(), "local", true), &scope);
        let rows = rig.driver().collection_rows().unwrap();
        // A scope nothing was published to holds no zone yet.
        let before = if active { server_records(&service, &scope) } else { Default::default() };
        let preview = rig.preview(&mut service, STEPS).unwrap();
        let decision = rig.setup_decision(&preview, UNKNOWN).unwrap();
        if active {
            assert!(preview.decision == ConnectDecision::ChooseCollection);
            assert_eq!(
                asks(&decision),
                (SetupRow::KeepCollection, SetupQuestion::Keep, Some(AskReason::TwoCollections), true, true)
            );
        } else {
            assert!(preview.decision == ConnectDecision::ConfirmUpload);
            assert_eq!(decision, silent(SetupRow::Publish));
        }
        rig.confirm_publication(&preview).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        let facts = rig.facts().unwrap();
        assert!(facts.recovery_ops.is_empty());
        let current = facts.current.unwrap();
        assert_eq!(current.facts.state, BindingState::Active);
        // A new epoch is active; a replaced one stays in iCloud (V22 invariant 6).
        let after = server_records(&service, &scope);
        let zone = meguri_sync::cloud::adapter::collection_zone(current.facts.epoch);
        assert!(!before.contains_key(&zone) && after.contains_key(&zone));
        for (name, records) in before.iter().filter(|(name, _)| name.starts_with("c-")) {
            assert!(after.get(name) == Some(records), "the replaced zone {name} changed");
        }
        assert_eq!(rig.driver().collection_rows().unwrap(), rows);
        rig.close_clean().unwrap();
    }
}

/// The rig's child process is this test binary, started at this test.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
