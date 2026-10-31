//! Device/core Rust peak heap, not full RSS or native R1852 acceptance.
//! Each child executes the actual CoreHost runtime. The parent alone owns
//! Service, request/response assets and fixture generation; none of those
//! allocations are subtracted from a same-process measurement.
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::entity_codec as codec;
use meguri_testkit::{
    bench::peak_alloc::PeakAlloc,
    cloud::{adapter::SDKRig, devices::PreboundDevice, *},
    generate::recipe::{Recipe, generate_recipe},
};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;
const NOW: i64 = 1_735_689_600_000;
const SEED: u64 = 7103;
const BYTES: usize = 40 * 1024 * 1024;
const RUST_HEAP_LIMIT: usize = 300_000_000;

#[test]
fn four_40_mib_ordinary_payloads_build_persist_and_ack_with_bounded_child_heap() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    let recipe = Recipe::parse("recipe: ordinary-pa-memory\ndescription: Generated public Notes\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic\n    notetype: basic\n    new: 4\n    fields: [\"front\", \"back\"]\n").unwrap();
    generate_recipe(&recipe, SEED, &directory).unwrap();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(SEED)).unwrap();
    let files = PreboundDevice::arrange(&directory, &mut service, SEED).unwrap();
    let mut sdk = SDKRig::open(&files, false, NOW, SEED).unwrap();
    sdk.reconcile_and_confirm_root(&mut service, 10_000).unwrap();
    let mut expected = std::collections::BTreeMap::new();
    for (index, note) in files.notes().iter().enumerate() {
        let proof = sdk
            .driver()
            .edit_generated_payload(note.id, BYTES, SEED + index as u64)
            .unwrap();
        assert!(proof.dirty && proof.local_rev > proof.acked_rev);
        expected.insert(proof.name.clone(), proof);
    }
    assert_eq!(expected.len(), 4);
    // Exercise the real record budget and pending tail across two requests.
    // One preparation window owns one image; this workload needs four windows.
    // Defaults, including the preexisting 400-Note throughput assertions, stay intact.
    sdk.driver().payload_record_limit(2).unwrap();
    let baseline = sdk.driver().payload_memory_reset().unwrap();
    let mut delivered = std::collections::BTreeSet::new();
    let mut calls = 0;
    while delivered.len() < expected.len() {
        calls += 1;
        assert!(calls <= 2, "budgeted tail failed to deliver");
        let attempt = sdk.request_collection_batch(&mut service).unwrap();
        assert_eq!(attempt.mutations.len(), 2);
        for mutation in &attempt.mutations {
            let Mutation::Save { record, .. } = mutation else {
                panic!("Save")
            };
            let Field::Asset(asset) = &record.fields["pa"] else {
                panic!("real pa")
            };
            assert!(asset.bytes().len() >= BYTES && asset.bytes().len() as u64 <= codec::MAX_PAYLOAD_BYTES);
            assert_eq!(codec::sha256(asset.bytes()), expected[&record.name].payload_digest);
            assert!(delivered.insert(record.name.clone()));
        }
        let response = sdk.sent_request(attempt.request_ids[0]).unwrap();
        let Reply::Modified(items) = response.response.result.as_ref().unwrap() else {
            panic!("Modified")
        };
        for item in items {
            let MutationSuccess::Saved(record) = item.result.as_ref().unwrap() else {
                panic!("Saved")
            };
            let proof = sdk.driver().payload_proof(&record.name).unwrap();
            let before = &expected[&record.name];
            assert_eq!(proof.acked_rev, before.local_rev);
            assert_eq!(proof.local_rev, before.local_rev);
            assert_eq!(proof.server_erev, before.server_erev + 1);
            assert_eq!(proof.payload_digest, before.payload_digest);
            assert_eq!(proof.acked_payload_digest, before.payload_digest);
            assert_eq!(proof.system_fields, record.system_fields.bytes());
            let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
                panic!("e")
            };
            assert_eq!(&proof.envelope, envelope);
            assert!(!proof.dirty && proof.flights == 0);
        }
        let pending = sdk.core_tracking_sendables().unwrap();
        for name in expected.keys() {
            assert_eq!(pending.contains_key(name), !delivered.contains(name));
        }
    }
    assert!(sdk.core_tracking_sendables().unwrap().is_empty());
    let (peak, live) = sdk.driver().payload_memory_peak().unwrap();
    eprintln!(
        "device/core Rust heap: baseline={baseline} peak={peak} live={live}; parent Service excluded; RSS/native R1852 unmeasured"
    );
    // Absolute child heap, not a fabricated logical byte count or baseline delta.
    assert!(
        peak < RUST_HEAP_LIMIT,
        "device/core Rust peak heap {peak}; baseline {baseline}"
    );
    sdk.close_clean().unwrap();
}

#[test]
fn eight_lost_40_mib_revisions_of_one_note_edit_reopen_and_recover_exact_own_save() {
    fn sample(sdk: &mut SDKRig, phase: &str) {
        let (peak, live) = sdk.driver().payload_memory_peak().unwrap();
        eprintln!("device/core Rust heap phase={phase} cumulative_peak={peak} live={live}");
    }

    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    let recipe = Recipe::parse("recipe: retained-pa-memory\ndescription: Generated public Note\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic\n    notetype: basic\n    new: 1\n    fields: [\"front\", \"back\"]\n").unwrap();
    generate_recipe(&recipe, SEED, &directory).unwrap();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(SEED)).unwrap();
    let files = PreboundDevice::arrange(&directory, &mut service, SEED).unwrap();
    let mut sdk = SDKRig::open(&files, true, NOW, SEED).unwrap();
    sdk.reconcile_and_confirm_root(&mut service, 10_000).unwrap();
    let note = files.notes()[0].id;
    let baseline = sdk.driver().payload_memory_reset().unwrap();
    let mut digests = std::collections::BTreeSet::new();
    let mut landed = None;
    for revision in 0..8 {
        // Lose seven quota refusals and one successful reply. Without callback
        // delivery or a fetch, recovery must still find all eight flights.
        let mut config = Config::default();
        if revision < 7 {
            config.limits.asset_bytes = (BYTES - 1) as u64;
        }
        service
            .request(&files.scope, Caller::default(), Operation::Configure(config))
            .unwrap()
            .result
            .unwrap();
        let proof = sdk
            .driver()
            .edit_generated_payload(note, BYTES, SEED + revision)
            .unwrap();
        assert!(digests.insert(proof.payload_digest));
        assert_eq!(proof.flights, revision as usize);
        let response = sdk.lose_collection_batch_reply(&mut service).unwrap();
        assert_eq!(response.mutations.len(), 1);
        let Mutation::Save { record, .. } = &response.mutations[0] else {
            panic!("Save")
        };
        let Field::Asset(asset) = &record.fields["pa"] else {
            panic!("pa")
        };
        assert!(asset.bytes().len() >= BYTES && asset.bytes().len() as u64 <= codec::MAX_PAYLOAD_BYTES);
        assert_eq!(codec::sha256(asset.bytes()), proof.payload_digest);
        let Reply::Modified(results) = response.response.result.as_ref().unwrap() else {
            panic!("Modified")
        };
        assert_eq!(results.len(), 1);
        if revision < 7 {
            let Err(error) = &results[0].result else {
                panic!("real quota failure");
            };
            assert_eq!(error.code, ErrorCode::LimitExceeded);
        } else {
            let MutationSuccess::Saved(saved) = results[0].result.as_ref().unwrap() else {
                panic!("Saved")
            };
            let Field::EncryptedBytes(envelope) = &saved.fields["e"] else {
                panic!("e")
            };
            landed = Some((proof.clone(), saved.system_fields.bytes().to_vec(), envelope.clone()));
        }
        let after = sdk.driver().payload_proof(&proof.name).unwrap();
        assert_eq!(after.flights, revision as usize + 1);
        assert_eq!(after.acked_rev, proof.acked_rev);
        assert_eq!(after.acked_payload_digest, proof.acked_payload_digest);
        assert!(after.dirty);
        assert!(sdk.send_changes_calls().last().unwrap().returned);
        sample(&mut sdk, "after generated edit and lost reply");
    }
    let (saved, system_fields, envelope) = landed.unwrap();
    let newer = sdk.driver().edit_generated_payload(note, BYTES, SEED + 8).unwrap();
    assert_eq!(newer.flights, 8);
    assert!(newer.local_rev > saved.local_rev);
    sample(&mut sdk, "after newer edit");
    sdk.reopen_after_lost_replies().unwrap();
    let reopened = sdk.driver().payload_proof(&newer.name).unwrap();
    assert_eq!(reopened.flights, 8);
    assert_eq!(reopened.payload_digest, newer.payload_digest);
    assert_eq!(reopened.acked_rev, newer.acked_rev);
    sample(&mut sdk, "after reopen");
    sdk.reconcile_register_and_confirm_root(&mut service, 10_000).unwrap();
    sample(&mut sdk, "after reconcile register and root");
    sdk.fetch_recovery_exchange(&mut service, 10_000).unwrap();
    sample(&mut sdk, "after fetch");
    assert!(matches!(
        sdk.driver().drain_incoming(10_000).unwrap(),
        meguri_testkit::cloud::child_driver::IncomingProgress::Completed(_)
    ));
    sample(&mut sdk, "after final drain");
    let recovered = sdk.driver().payload_proof(&newer.name).unwrap();
    assert_eq!(recovered.acked_rev, saved.local_rev);
    assert_eq!(recovered.server_erev, saved.server_erev + 1);
    assert_eq!(recovered.acked_payload_digest, saved.payload_digest);
    assert_eq!(recovered.system_fields, system_fields);
    assert_eq!(recovered.envelope, envelope);
    assert_eq!(recovered.local_rev, newer.local_rev);
    assert_eq!(recovered.payload_digest, newer.payload_digest);
    assert!(recovered.dirty);
    assert_eq!(recovered.flights, 0);
    let (peak, live) = sdk.driver().payload_memory_peak().unwrap();
    eprintln!(
        "device/core Rust heap: baseline={baseline} peak={peak} live={live}; parent Service excluded; RSS/native R1852 unmeasured"
    );
    assert!(
        peak < RUST_HEAP_LIMIT,
        "device/core Rust peak heap {peak}; baseline {baseline}"
    );
    sdk.close_clean().unwrap();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
