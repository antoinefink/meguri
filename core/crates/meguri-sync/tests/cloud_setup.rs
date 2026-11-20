//! `sync/cloudkit-recovery#V27.2`, V27.6 and V27.8 on the real Runtime, the
//! real adoption and publication operations and the testkit's CloudKit
//! service. Public synthetic collections only.
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    BindingState, IsolationReason, entity_codec as codec,
    runtime::{KeyByteSource, KeySourceError},
    setup::{AskReason, OtherServiceHistory, SetupDecision, SetupQuestion, SetupRow},
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
    let mut rig = open(&generated(run.path(), "local", false), &scope);
    let before = rig.driver().collection_rows().unwrap();
    assert_eq!(notes(&before), 0);
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
    let mut rig = open(&generated(run.path(), "local", false), &scope);
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
    let mut rig = open(&generated(run.path(), "local", false), &scope);
    rig.driver().put_media("only-media.bin", b"abc").unwrap();
    let preview = rig.preview(&mut service, STEPS).unwrap();
    let decision = rig.setup_decision(&preview, UNKNOWN).unwrap();
    assert_eq!((decision.row, decision.question), (SetupRow::KeepCollection, SetupQuestion::Keep));
    assert!(rig.start_setup(&preview, UNKNOWN).is_err());
    assert_eq!(rig.driver().media_files().unwrap()["only-media.bin"], b"abc");
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
