use meguri_engine::{OpSource, OpSpec};
use meguri_host::CoreHost;
use meguri_model::{
    ids::{DeckId, NoteId},
    rng::SeededRandom,
};
use meguri_storage::{
    ReadRows, ReaderRole,
    raw::{PlainDatabase, RawValue},
};
use meguri_sync::cloud::{
    self as cloud,
    entity_codec::Payload,
    runtime::{CloudInputs, IdentityProvider, IdentityStatus, KeyByteSource, KeySourceError},
};
use meguri_testkit::{
    cloud::{
        Config, Scope, Service,
        checks::{
            CheckRun, ComponentReport, Coverage,
            DeviceId::{self, A, B},
            Membership,
            ProofState::{self, Failed, Missing, Proven},
            REGISTRY, Timing,
        },
        coordinator::PublicationRig,
        devices::{IdentityReads, NativeFiles, options},
    },
    generate::{
        cloud_small,
        recipe::{Recipe, generate_recipe},
    },
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const NOW: i64 = 1_735_689_600_000;
const STEPS: usize = 10_000;

struct Keys(AtomicUsize);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Ok(std::array::from_fn(|i| {
            (i as u8).wrapping_mul(19).wrapping_add(n as u8)
        }))
    }
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.q5-component".into(),
        environment: "development".into(),
        account: "synthetic-q5".into(),
    }
}
fn open(directory: &Path, seed: u64) -> PublicationRig {
    PublicationRig::open(
        directory,
        &scope(),
        IdentityReads::Available(scope().account),
        false,
        NOW,
        seed,
        Arc::new(Keys(AtomicUsize::new(0))),
    )
    .unwrap()
}
fn generated(directory: &Path, notes: usize, seed: u64) {
    let recipe = Recipe::parse(&format!("recipe: shared-checks\ndescription: Public synthetic Q5 component\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Checks\n    notetype: basic\n    new: {notes}\n    fields: [\"generated front {{n}}\", \"generated back {{n}}\"]\n")).unwrap();
    generate_recipe(&recipe, seed, directory).unwrap();
}
fn publish(rig: &mut PublicationRig, service: &mut Service<SeededRandom>) {
    let preview = rig.preview(service, STEPS).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(service, STEPS).unwrap();
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
}
struct Pair {
    _root: tempfile::TempDir,
    receiver_dir: PathBuf,
    service: Service<SeededRandom>,
    sender: PublicationRig,
    receiver: PublicationRig,
}
impl Pair {
    fn adopted() -> Self {
        let root = tempfile::tempdir().unwrap();
        let sender_dir = root.path().join("sender");
        let receiver_dir = root.path().join("receiver");
        cloud_small::generate(&cloud_small::Request::new(73), &sender_dir).unwrap();
        generated(&receiver_dir, 1, 74);
        let mut service = Service::open(
            root.path(),
            Config {
                page_size: 32,
                ..Config::default()
            },
            SeededRandom::new(75),
        )
        .unwrap();
        let mut sender = open(&sender_dir, 76);
        publish(&mut sender, &mut service);
        let mut receiver = open(&receiver_dir, 77);
        let preview = receiver.preview(&mut service, STEPS).unwrap();
        receiver.adopt_cloud(&preview).unwrap();
        receiver.drive_ready(&mut service, STEPS).unwrap();
        assert!(receiver.facts().unwrap().recovery_ops.is_empty());
        Self {
            _root: root,
            receiver_dir,
            service,
            sender,
            receiver,
        }
    }
    fn check(&mut self, run: &mut CheckRun) -> ComponentReport {
        run.at_quiescence(
            &mut self.service,
            &mut [(A, self.sender.driver()), (B, self.receiver.driver())],
        )
    }
    fn close(&mut self) {
        self.receiver.close_clean().unwrap();
        self.sender.close_clean().unwrap();
    }
}
fn check<'a>(report: &'a ComponentReport, name: &str) -> &'a Coverage {
    report.checks.iter().find(|c| c.definition.name == name).unwrap()
}
fn fact(report: &ComponentReport, name: &str, subproof: &str) -> ProofState {
    check(report, name)
        .facts
        .iter()
        .find(|f| f.name == subproof)
        .unwrap()
        .state
}
fn canonical(report: &ComponentReport) -> ProofState {
    fact(report, "converged", "canonical-collection-payloads-kinds-1-through-8")
}
fn history(report: &ComponentReport) -> ProofState {
    fact(report, "history", "retained-observed-committed-review-rows")
}

/// Q5/K21.3: actual publication + adoption; no prebound fixture or seeded ack.
#[test]
fn published_and_adopted_small_proves_local_facts_but_preserves_missing_requirements() {
    let mut pair = Pair::adopted();
    let report = pair.check(&mut CheckRun::new([A, B]));
    assert!(report.input_complete);
    assert_eq!(
        (report.compared_devices, report.compared_groups, report.observed_reviews),
        (2, 1, 120)
    );
    assert_eq!(canonical(&report), Proven);
    assert_eq!(fact(&report, "converged", "equal-col-crt"), Proven);
    assert_eq!(history(&report), Proven);
    for name in [
        "local-sqlite-integrity",
        "local-schema-18-expected-objects",
        "local-references",
    ] {
        assert_eq!(fact(&report, "integrity", name), Proven);
    }
    assert_eq!(fact(&report, "coherent", "local-field-counts"), Proven);
    assert!(
        report
            .checks
            .iter()
            .all(|c| c.state == Missing && !c.missing.is_empty())
    );
    assert!(
        check(&report, "integrity")
            .missing
            .contains(&"oracle-check-database-on-copy-plus-wal-schema18-frozen-clock-unchanged-scm")
    );
    assert!(
        check(&report, "converged")
            .missing
            .contains(&"actual-media-file-hashes-and-media-missing-lists")
    );
    assert_eq!(report.checks.iter().map(|c| c.definition).collect::<Vec<_>>(), REGISTRY);
    assert_eq!(REGISTRY[0].when, Timing::QuiescenceRelaunchAndFinalAfterOtherChecks);
    // Actual marker text and account/settings must not escape through reports.
    let text = serde_json::to_string(&report).unwrap();
    assert!(!text.contains(&cloud_small::Request::new(73).marker()));
    assert!(!text.contains(&scope().account));
    assert!(!format!("{report:?}").contains("generated front"));
    pair.close();
}

/// Q5/K21.3: the reader sees an unsent production edit, not cached ack rows.
#[test]
fn a_real_note_edit_breaks_canonical_convergence() {
    let mut pair = Pair::adopted();
    let mut run = CheckRun::new([A, B]);
    assert_eq!(canonical(&pair.check(&mut run)), Proven);
    let note = pair
        .sender
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .find_map(|p| if let Payload::Note(n) = p { Some(n) } else { None })
        .unwrap();
    pair.sender
        .driver()
        .edit_note_fields(note.id, &["private-looking generated sentinel", "edited answer"])
        .unwrap();
    let report = pair.check(&mut run);
    assert_eq!(canonical(&report), Failed);
    assert_eq!(check(&report, "converged").state, Failed);
    assert_eq!(history(&report), Proven);
    assert!(!serde_json::to_string(&report).unwrap().contains("private-looking"));
    pair.close();
}

/// Q5/K21.7: removal from every current peer must still fail retained history.
#[test]
fn committed_review_rows_remain_required_after_both_devices_remove_them() {
    let mut pair = Pair::adopted();
    let mut run = CheckRun::new([A, B]);
    assert_eq!(history(&pair.check(&mut run)), Proven);
    let review = pair
        .sender
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .find_map(|p| if let Payload::Revlog(r) = p { Some(r.id) } else { None })
        .unwrap();
    pair.sender.driver().remove_revlog(review).unwrap();
    assert_eq!(history(&pair.check(&mut run)), Failed);
    pair.receiver.driver().remove_revlog(review).unwrap();
    let report = pair.check(&mut run);
    assert_eq!(history(&report), Failed);
    assert_eq!(report.observed_reviews, 118);
    // Current snapshots agree; their equality does not erase committed history.
    assert_eq!(canonical(&report), Proven);
    pair.close();
}

/// Q5: col.crt is a separate comparison, excluded collection metadata is not.
#[test]
fn real_durable_crt_difference_is_detected_after_reopen() {
    let mut pair = Pair::adopted();
    pair.receiver.close_clean().unwrap();
    {
        let db = PlainDatabase::open(&pair.receiver_dir.join("collection.anki2")).unwrap();
        db.execute("UPDATE col SET crt=crt+1", &[]).unwrap();
    }
    pair.receiver = open(&pair.receiver_dir, 77);
    let report = pair.check(&mut CheckRun::new([A, B]));
    assert_eq!(fact(&report, "converged", "equal-col-crt"), Failed);
    assert_eq!(check(&report, "converged").state, Failed);
    pair.close();
}

/// Q5 integrity/coherent local subproofs over one Host WAL view. Intentional
/// malformed references are durable engine commits, not supplied predicates.
#[test]
fn local_integrity_and_coherence_fail_on_real_durable_references_and_field_counts() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("device");
    generated(&directory, 1, 81);
    let host = CoreHost::open(options(&directory, NOW, 82)).unwrap();
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(83)).unwrap();
    let mut run = CheckRun::new([A]);
    let initial = run.at_quiescence_hosts(&mut service, &[(A, &host)]);
    assert_eq!(fact(&initial, "integrity", "local-references"), Proven);
    assert_eq!(canonical(&initial), Missing); // local-only is never compared
    assert_eq!(initial.members[0].membership, Membership::Unbound);
    let collection = host.collection().unwrap();
    let card = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        reader.cards_after(None, 1).unwrap().remove(0)
    };
    let mut orphan = card.clone();
    orphan.note_id = NoteId(i64::MAX - 1);
    orphan.deck_id = DeckId(i64::MAX - 2);
    collection
        .run(
            OpSpec::named("synthetic orphan references", OpSource::LocalApi),
            move |ctx| {
                ctx.scope().put(&orphan)?;
                Ok(())
            },
        )
        .unwrap();
    let report = run.at_quiescence_hosts(&mut service, &[(A, &host)]);
    assert_eq!(fact(&report, "integrity", "local-sqlite-integrity"), Proven);
    assert_eq!(fact(&report, "integrity", "local-references"), Failed);
    assert_eq!(check(&report, "integrity").state, Failed);
    assert_eq!(check(&report, "coherent").state, Failed);
    collection
        .run(
            OpSpec::named("restore synthetic references", OpSource::LocalApi),
            move |ctx| {
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    host.close().unwrap();
    // Exercise field counts through the production reader after a durable raw
    // fixture mutation made while the Host is closed.
    {
        let db = PlainDatabase::open(&directory.join("collection.anki2")).unwrap();
        db.execute("UPDATE notes SET flds=?1", &[RawValue::text("one field")])
            .unwrap();
    }
    let host = CoreHost::open(options(&directory, NOW, 82)).unwrap();
    let report = run.at_quiescence_hosts(&mut service, &[(A, &host)]);
    assert_eq!(fact(&report, "coherent", "local-field-counts"), Failed);
    host.close().unwrap();
}

/// Q5: never silently remove a missing/duplicate member or accept an empty set.
#[test]
fn absent_and_duplicate_observations_and_zero_compared_devices_fail_closed() {
    let mut pair = Pair::adopted();
    let report = CheckRun::new([A, B]).at_quiescence(&mut pair.service, &mut [(A, pair.sender.driver())]);
    assert!(!report.input_complete);
    assert_eq!(report.members[1].membership, Membership::Unavailable);
    assert_eq!(canonical(&report), Missing);
    assert_eq!(history(&report), Missing);
    let duplicate = CheckRun::new([A, B]).at_quiescence(
        &mut pair.service,
        &mut [(A, pair.sender.driver()), (A, pair.receiver.driver())],
    );
    assert!(!duplicate.input_complete);
    assert_eq!(canonical(&duplicate), Missing);
    let empty = CheckRun::new([]).at_quiescence(&mut pair.service, &mut []);
    assert!(!empty.input_complete);
    assert_eq!(empty.compared_devices, 0);
    assert_eq!(canonical(&empty), Missing);
    assert_eq!(history(&empty), Missing);
    pair.close();
}

#[test]
fn one_published_host_under_two_labels_cannot_prove_multi_device_facts() {
    struct Account;
    impl IdentityProvider for Account {
        fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
            IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes()))
        }
    }
    let mut pair = Pair::adopted();
    pair.close();
    let reopen = |directory: &Path, seed| {
        let mut config = options(directory, NOW, seed);
        config.cloud = Some(CloudInputs {
            scope: cloud::BuildScope {
                container: cloud::ContainerId::new(scope().container).unwrap(),
                environment: cloud::Environment::Development,
            },
            identity: Arc::new(Account),
            files: Arc::new(NativeFiles),
        });
        let host = CoreHost::open(config).unwrap();
        host.cloud_runtime().unwrap().refresh_identity().unwrap();
        host
    };
    let sender = reopen(&pair._root.path().join("sender"), 76);
    let receiver = reopen(&pair.receiver_dir, 77);
    let distinct = CheckRun::new([A, B]).at_quiescence_hosts(&mut pair.service, &[(A, &sender), (B, &receiver)]);
    assert!(distinct.input_complete);
    assert_eq!(distinct.compared_devices, 2);
    assert_eq!(canonical(&distinct), Proven);
    assert_eq!(history(&distinct), Proven);

    let aliased = CheckRun::new([A, B]).at_quiescence_hosts(&mut pair.service, &[(A, &sender), (B, &sender)]);
    assert!(!aliased.input_complete);
    assert_eq!(aliased.compared_devices, 1);
    assert_eq!(aliased.members[1].membership, Membership::Unavailable);
    assert_eq!(canonical(&aliased), Missing);
    assert_eq!(history(&aliased), Missing);
    assert!(aliased.checks.iter().flat_map(|c| &c.facts).all(|f| f.state != Proven));
    receiver.close().unwrap();
    sender.close().unwrap();
}

/// Q5 ancestry scope: a replacement publishes another epoch. The old member
/// remains in the report and local checks; it is not globally compared with it.
#[test]
fn a_real_replacement_epoch_does_not_compare_the_old_bound_device() {
    let mut pair = Pair::adopted();
    let mut run = CheckRun::new([A, B]);
    assert_eq!(canonical(&pair.check(&mut run)), Proven);
    let note = pair
        .receiver
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .find_map(|p| if let Payload::Note(n) = p { Some(n.id) } else { None })
        .unwrap();
    pair.receiver
        .driver()
        .edit_note_fields(note, &["replacement front", "replacement back"])
        .unwrap();
    publish(&mut pair.receiver, &mut pair.service);
    let report = pair.check(&mut run);
    assert_eq!(report.members[0].membership, Membership::OtherEpoch);
    assert_eq!(report.members[1].membership, Membership::Compared);
    assert_eq!(report.compared_devices, 1);
    assert_eq!(canonical(&report), Proven);
    assert_eq!(check(&report, "converged").state, Missing);
    pair.sender.driver().sign_out_for_testing().unwrap();
    let report = pair.check(&mut run);
    assert_eq!(report.members[0].membership, Membership::Isolated);
    assert_eq!(fact(&report, "integrity", "local-references"), Proven);
    pair.close();
}

/// Boundary-only fixture: just over the legacy 2048-row inspection cap. This
/// is not a 150k/large-asset/soak workload. The new collector must read all pages.
#[test]
fn canonical_observation_is_not_truncated_by_the_existing_fixture_inspection_cap() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("device");
    generated(&directory, 1025, 91);
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(92)).unwrap();
    let mut rig = open(&directory, 93);
    publish(&mut rig, &mut service);
    assert!(rig.driver().collection_rows().is_err()); // documented legacy cap
    let report = CheckRun::new([DeviceId::A]).at_quiescence(&mut service, &mut [(A, rig.driver())]);
    assert!(report.observed_entities > 2048);
    assert_eq!(canonical(&report), Proven);
    rig.close_clean().unwrap();
}

/// Independent scoped active epochs are separate comparisons even when both
/// members are supplied in the same drain roster and their payloads differ.
#[test]
fn two_real_scoped_publications_are_never_compared_globally() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("first");
    let second = root.path().join("second");
    generated(&first, 1, 101);
    generated(&second, 2, 102);
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(103)).unwrap();
    let mut sender = open(&first, 104);
    publish(&mut sender, &mut service);
    let other_scope = Scope {
        account: "synthetic-q5-other-account".into(),
        ..scope()
    };
    let mut receiver = PublicationRig::open(
        &second,
        &other_scope,
        IdentityReads::Available(other_scope.account.clone()),
        false,
        NOW,
        105,
        Arc::new(Keys(AtomicUsize::new(0))),
    )
    .unwrap();
    publish(&mut receiver, &mut service);
    let report = CheckRun::new([A, B]).at_quiescence(&mut service, &mut [(A, sender.driver()), (B, receiver.driver())]);
    assert_eq!((report.compared_devices, report.compared_groups), (2, 2));
    assert_eq!(canonical(&report), Proven);
    assert_eq!(check(&report, "converged").state, Missing);
    receiver.close_clean().unwrap();
    sender.close_clean().unwrap();
}

// Each test executable owns its child entry; this is IPC, not behavior evidence.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
