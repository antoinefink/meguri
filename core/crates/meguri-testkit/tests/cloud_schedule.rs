//! Q3.6/Q4.4/Q4.5 component evidence only. Real publication/adoption and core
//! exchanges; no Q6 catalog, whole-Q5, fake lane, fault, soak or unit acceptance.
use meguri_model::rng::{ChaCha20Random, RandomSource, SeededRandom};
use meguri_sync::cloud::entity_codec::Payload;
use meguri_testkit::cloud::{
    Config,
    adapter::{FetchScope, OrdinaryAction},
    checks::{
        CheckRun, ComponentReport,
        DeviceId::{self, A, B, C, D},
        ProofState,
    },
    schedule::{
        ActionKey, Blocker, DrainOutcome, ProgramKind, RequestReceipt, Runner, Step, Trace, Unsupported,
        UnsupportedProgram,
    },
};

fn setup(seed: u64, devices: &[DeviceId]) -> (tempfile::TempDir, Runner) {
    let root = tempfile::tempdir().unwrap();
    let runner = Runner::setup(
        root.path(),
        devices,
        seed,
        Config {
            page_size: 16,
            ..Config::default()
        },
    )
    .unwrap();
    (root, runner)
}
fn notes(runner: &mut Runner) -> Vec<i64> {
    let mut ids: Vec<_> = runner
        .collection_rows(A)
        .unwrap()
        .into_iter()
        .filter_map(|p| {
            if let Payload::Note(note) = p {
                Some(note.id)
            } else {
                None
            }
        })
        .collect();
    ids.sort();
    ids
}
fn edit(runner: &mut Runner, device: DeviceId, note: i64, front: &str) {
    runner
        .enqueue(
            device,
            Step::Edit {
                note,
                fields: vec![front.into(), "synthetic answer".into()],
            },
        )
        .unwrap();
}
fn until(runner: &mut Runner, predicate: impl Fn(&Trace) -> bool) -> Trace {
    for _ in 0..10_000 {
        let trace = runner.grant().unwrap().expect("implemented work remains");
        if predicate(&trace) {
            return trace;
        }
    }
    panic!("bounded grant target did not occur");
}
fn finite(runner: &mut Runner) {
    for _ in 0..10_000 {
        if runner.grant().unwrap().is_none() {
            return;
        }
    }
    panic!("ordinary program did not finish within grant bound");
}
fn fact(report: &ComponentReport, name: &str, part: &str) -> ProofState {
    report
        .checks
        .iter()
        .find(|c| c.definition.name == name)
        .unwrap()
        .facts
        .iter()
        .find(|f| f.name == part)
        .unwrap()
        .state
}
fn available(report: &ComponentReport) {
    assert!(report.input_complete);
    assert_eq!(
        fact(report, "converged", "canonical-collection-payloads-kinds-1-through-8"),
        ProofState::Proven
    );
    assert_eq!(
        fact(report, "history", "retained-observed-committed-review-rows"),
        ProofState::Proven
    );
    assert_eq!(fact(report, "integrity", "local-sqlite-integrity"), ProofState::Proven);
    assert_eq!(fact(report, "coherent", "local-field-counts"), ProofState::Proven);
    assert!(
        report
            .checks
            .iter()
            .all(|c| c.state == ProofState::Missing && !c.missing.is_empty())
    );
}
fn settled_but_not_proved(runner: &mut Runner) {
    let DrainOutcome::NotProved { facts, .. } = runner.drain().unwrap() else {
        panic!("implemented exchanges must finish");
    };
    for member in facts {
        for missing in [
            Blocker::OnlineNotObserved,
            Blocker::GateNotObserved,
            Blocker::CompleteHeldSetNotObserved,
            Blocker::PeerResolvableInboxNotObserved,
            Blocker::ServeableMediaNotObserved,
            Blocker::DueOwnOperationsNotObserved,
        ] {
            assert!(member.blockers.contains(&missing));
        }
        for dynamic in [
            Blocker::ProgramPending,
            Blocker::EngineWork,
            Blocker::RetainedCallback,
            Blocker::StaleRootDrain,
            Blocker::StaleCollectionDrain,
            Blocker::PendingEntity,
            Blocker::InFlightEntity,
            Blocker::InboxEntries,
            Blocker::FactsUnavailable,
        ] {
            assert!(!member.blockers.contains(&dynamic), "{dynamic:?}");
        }
        assert_eq!(member.root_read_revision, Some(runner.revision()));
        assert_eq!(member.collection_read_revision, Some(runner.revision()));
    }
}

/// Q3.6 uses the dependency's ChaCha20 through modelrng, retaining legacy RNG.
#[test]
fn chacha_scheduler_stream_replays_without_replacing_legacy_rng() {
    let a = ChaCha20Random::new(23);
    let b = ChaCha20Random::new(23);
    let legacy = SeededRandom::new(23);
    let actual: Vec<_> = (0..32).map(|_| a.next_u64()).collect();
    assert_eq!(actual, (0..32).map(|_| b.next_u64()).collect::<Vec<_>>());
    assert_ne!(actual[0], legacy.next_u64());
}

fn ordinary_program(seed: u64) -> (Vec<Trace>, Vec<RequestReceipt>, Vec<Vec<u8>>, String) {
    let (_root, mut runner) = setup(seed, &[A, B]);
    let ids = notes(&mut runner);
    runner.enqueue(A, Step::HoldSent).unwrap();
    edit(&mut runner, A, ids[0], "first synthetic edit");
    runner.enqueue(A, Step::Sync).unwrap();
    let first = until(&mut runner, |t| {
        t.device == A && t.action == ActionKey::Engine(OrdinaryAction::BatchSave)
    });
    let request = first.receipt.request.unwrap();
    // A's real save has reached Service, while the original callback is held.
    edit(&mut runner, B, ids[1], "peer interleaved edit");
    runner.enqueue(B, Step::Sync).unwrap();
    until(&mut runner, |t| {
        t.device == B && t.action == ActionKey::Engine(OrdinaryAction::BatchSave)
    });
    assert!(
        !runner
            .trace()
            .iter()
            .any(|t| t.device == A && t.action == ActionKey::Engine(OrdinaryAction::Sent))
    );
    assert!(
        runner
            .drain_facts()
            .iter()
            .find(|m| m.device == A)
            .unwrap()
            .blockers
            .contains(&Blocker::RetainedCallback)
    );
    // The retained older ACK must preserve this actual newer local write.
    edit(&mut runner, A, ids[0], "newer synthetic edit");
    runner.enqueue(A, Step::ReleaseSent).unwrap();
    finite(&mut runner);
    assert_eq!(
        runner
            .trace()
            .iter()
            .filter(|t| t.device == A
                && t.action == ActionKey::Engine(OrdinaryAction::Sent)
                && t.receipt.request == Some(request))
            .count(),
        1
    );
    runner.enqueue(B, Step::StartReview { note: ids[2] }).unwrap();
    runner.enqueue(B, Step::ShowAnswer).unwrap();
    runner.enqueue(B, Step::AnswerGood).unwrap();
    runner.enqueue(B, Step::EndReview).unwrap();
    runner
        .enqueue(
            B,
            Step::PutMedia {
                name: "schedule-media.bin".into(),
                bytes: b"seeded ordinary media bytes".to_vec(),
            },
        )
        .unwrap();
    runner.enqueue(B, Step::Sync).unwrap();
    finite(&mut runner);
    settled_but_not_proved(&mut runner);
    let report = runner.collect_components(&mut CheckRun::new([A, B]));
    available(&report);
    let rows = runner.collection_rows(A).unwrap();
    assert!(
        rows.iter()
            .any(|p| matches!(p, Payload::Note(n) if n.id == ids[0] && n.flds.split(|byte| *byte == 0x1f).next() == Some(b"newer synthetic edit".as_slice())))
    );
    for device in [A, B] {
        assert!(
            runner
                .media_files(device)
                .unwrap()
                .values()
                .any(|bytes| bytes == b"seeded ordinary media bytes")
        );
    }
    assert_eq!(runner.collection_rows(A).unwrap(), runner.collection_rows(B).unwrap());
    let canonical = rows.into_iter().map(|p| p.encode().unwrap()).collect();
    let trace = runner.trace().to_vec();
    let order = runner.request_order().unwrap();
    let report = serde_json::to_string(&report).unwrap();
    let diagnostics = format!("{runner:?} {trace:?} {order:?} {report}");
    for marker in [
        "newer synthetic edit",
        "schedule-media.bin",
        "synthetic-seeded",
        "test.meguri.seeded-ordinary",
    ] {
        assert!(!diagnostics.contains(marker));
    }
    runner.close().unwrap();
    (trace, order, canonical, report)
}

/// Q3.6/Q4.5: actual two-child Service/core work, exact replay including final
/// canonical rows and available Q5 facts, with an interleaved retained callback.
#[test]
fn actual_two_device_seeded_replay_retains_sent_callback_and_available_q5_facts() {
    let first = ordinary_program(0x117);
    let second = ordinary_program(0x117);
    assert_eq!(first, second);
    let different = ordinary_program(0x118);
    assert_ne!(
        first.0.iter().map(|t| (t.device, t.action)).collect::<Vec<_>>(),
        different.0.iter().map(|t| (t.device, t.action)).collect::<Vec<_>>()
    );
}

/// Q4.4: a page read before a peer write cannot become a fresh drain when its
/// retained callback finishes. Pagination continues using original Service pages.
#[test]
fn paginated_fetch_interleaved_write_retains_page_and_stale_drain_blocks() {
    let (_root, mut runner) = setup(0x119, &[A, B]);
    let ids = notes(&mut runner);
    runner.enqueue(A, Step::HoldFetch).unwrap();
    runner.enqueue(A, Step::Sync).unwrap();
    let original = until(&mut runner, |t| {
        t.device == A && t.receipt.read_started == Some(FetchScope::Collection)
    });
    let original_request = original.receipt.request.unwrap();
    edit(&mut runner, B, ids[3], "written between pages");
    runner.enqueue(B, Step::Sync).unwrap();
    until(&mut runner, |t| {
        t.device == B && t.action == ActionKey::Engine(OrdinaryAction::BatchSave)
    });
    let held = runner.drain_facts();
    let a = held.iter().find(|m| m.device == A).unwrap();
    assert!(a.blockers.contains(&Blocker::RetainedCallback));
    assert!(a.blockers.contains(&Blocker::StaleCollectionDrain));
    assert!(
        !runner
            .trace()
            .iter()
            .any(|t| t.receipt.callback_requests.contains(&original_request))
    );
    // Zero budget is a typed no-quiescence outcome, preserving original work.
    assert!(matches!(
        runner.drain_with_limit(0).unwrap(),
        DrainOutcome::NoQuiescence { grants: 0, .. }
    ));
    runner.enqueue(A, Step::ReleaseFetch).unwrap();
    let completion = until(&mut runner, |t| {
        t.device == A && t.receipt.drained == Some(FetchScope::Collection)
    });
    assert!(completion.revision_after > original.revision_before);
    let facts = runner.drain_facts();
    let a = facts.iter().find(|m| m.device == A).unwrap();
    assert_eq!(a.collection_read_revision, Some(original.revision_before));
    assert!(a.blockers.contains(&Blocker::StaleCollectionDrain));
    let callback_count = runner
        .trace()
        .iter()
        .filter(|t| t.receipt.callback_requests.contains(&original_request))
        .count();
    assert_eq!(callback_count, 1);
    let pages = runner
        .trace()
        .iter()
        .filter(|t| t.device == A && t.action == ActionKey::Engine(OrdinaryAction::FetchRequest))
        .count();
    assert!(pages > 2, "actual paginated collection plus root");
    finite(&mut runner);
    settled_but_not_proved(&mut runner);
    available(&runner.collect_components(&mut CheckRun::new([A, B])));
    assert_eq!(runner.collection_rows(A).unwrap(), runner.collection_rows(B).unwrap());
    runner.close().unwrap();
}

/// Stable 4-member publication/adoption, independent ordinary user edits and
/// seeded grants. Unsupported programs cannot be labeled completed.
#[test]
fn four_actual_devices_exchange_and_unsupported_programs_block_explicitly() {
    let (_root, mut runner) = setup(0x120, &[D, B, A, C]);
    let ids = notes(&mut runner);
    for (index, device) in [A, B, C, D].into_iter().enumerate() {
        edit(&mut runner, device, ids[index], "four member synthetic edit");
        runner.enqueue(device, Step::Sync).unwrap();
    }
    finite(&mut runner);
    settled_but_not_proved(&mut runner);
    available(&runner.collect_components(&mut CheckRun::new([A, B, C, D])));
    for device in [B, C, D] {
        assert_eq!(
            runner.collection_rows(A).unwrap(),
            runner.collection_rows(device).unwrap()
        );
    }
    assert!(
        runner
            .enqueue(A, Step::Unsupported(Unsupported::Account))
            .unwrap_err()
            .is::<UnsupportedProgram>()
    );
    assert!(
        !runner
            .trace()
            .iter()
            .any(|t| t.action == ActionKey::Program(ProgramKind::Unsupported))
    );
    runner.close().unwrap();
}

// IPC entry only; every child-backed target owns this exact entry name.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
