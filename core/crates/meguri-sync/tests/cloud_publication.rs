//! Tests FIRST: enabled missing-API RED for the first production publication phase.
//! Master starting commit: efcf26cabdcb42653e6adff2811249aff1d97340.
//! Role: fixer. Root intake metadata is authoritative.
//! Source-only delivery; independent review, compile RED and commit belong to root.
//!
//! Required seam (not implemented here): coordinator::PublicationRig opens an
//! UNBOUND generated directory through the existing child_driver/CoreHost IPC.
//! It extends the shared SDKRig scheduler, not a second publication model.
//! Runtime owns discovery, decisions, authority/entropy, recovery_ops, sidecar
//! writes, all-nine-kind snapshot/WAL scan, pending actions, completion and GC.
//! confirm_publication is explicit user confirmation of Runtime's fresh preview;
//! adopt_cloud invokes the real CloudAdopt replacement, never fixture binding.
//! facts/decision/recovery_ops/intents are typed reads of actual Runtime,
//! TrackingStore and DeviceState; collection_rows projects actual reader rows
//! with row_payload/codec; media_rows separately verifies MediaStore journal/files
//! for kind 9, independently of tracking/expected/acked payloads.
//! Secret-bearing facts/errors/Debug must be opaque. No helper seeds tracking,
//! acks, keys, decisions, server baselines, traces or successful outcomes.
//!
//! Point gates pause REAL work at the named boundary, before the next child grant
//! or SDK dispatch. drive_ready has the existing finite step bound and no new
//! user/policy input; an explicit gate ends a drive/drain without pretending
//! quiescence or completing an exchange. held_request is the original request;
//! submitting it directly to Service and killing the child loses its real reply.
//! The three CreateZone requests in Service represent ONE own modify-zones
//! operation (one own_operation attribution), never SDK database pending work.
//! preview_with_active_fields delivers the actual Service named-field read;
//! its absent e exercises the unreadable-Active branch without inventing an epoch.
//! Driver backup/restore/media methods call the real CoreHost services; backup
//! retention uses normal configured limits. Opening/reopening never clears an
//! isolation or changes a sidecar decision as fixture setup.
//! restore_backup returns the existing maintenance::RestoredBackup outcome,
//! including its actual safety_backup name. backup_path(name) resolves that
//! basename in the host's configured backup directory; backup_collection_rows(name)
//! inspects that real package and projects its saved rows through row_payload,
//! never tracking or the current collection. Retained/expired BackupNotice values
//! carry the backup name from durable replacement provenance, not driver input.
//!
//! runtime::KeyByteSource is a PLATFORM key-byte provider: each draw returns
//! exactly 32 bytes or KeySourceError::Unavailable. Runtime requests independent
//! record/media draws outside engine/tracking locks, before ANY publication
//! commit or cloud mutation, and has no SeededRandom/u64 key fallback. The script
//! below is public synthetic provider input only. Collection-id randomness keeps
//! its existing source. Reopen must use persisted authority without another draw.
//!
//! Empty physical media is intentional in the baseline cases. Nonempty media,
//! adoption/incoming delivery, K19, full Q4/Q5, signed SDK evidence and budgets
//! remain required later. The intent test explicitly depends on that missing
//! production media path; it is enabled RED, not an implemented media subsystem
//! or a full delivery/U113/U114/P24 completion claim.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use meguri_engine::device_state::{PendingDecisionReason, PublicationDecision};
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    BindingState, IsolationReason,
    control_codec::{self, Epoch, EpochReason, EpochState, MediaRoot},
    entity_codec as codec, row_payload,
    runtime::{KeyByteSource, KeySourceError},
};
use meguri_testkit::{
    cloud::{
        Caller, Config, Field, LogOutcome, Mutation, Operation, Record, RemovalCause, ReplyReference, RequestKind,
        RequestedFields, Scope, Service,
        adapter::Observation,
        coordinator::{BackupNotice, ConnectDecision, Point, PublicationRig, PublishStep},
        devices::IdentityReads,
    },
    generate::recipe::{Recipe, generate_recipe},
};

const ORIGIN: i64 = 1_735_689_600_000; // exactly 2025-01-01T00:00:00Z
const DAY: i64 = 86_400_000;
const SEED: u64 = 0x2411_9011;
const STEPS: usize = 10_000; // the existing scheduler bound, never raised on failure

// Every byte position participates; the two independent 256-bit draws differ
// also beyond their first eight bytes. Debug is deliberately not derived.
struct ScriptedKeys {
    script: Mutex<VecDeque<Result<[u8; 32], KeySourceError>>>,
    calls: Mutex<usize>,
}
impl ScriptedKeys {
    fn new(fail_at: Option<usize>) -> Arc<Self> {
        let mut script = VecDeque::new();
        for publication in 0..4_u8 {
            script.push_back(Ok(record_draw().map(|b| b ^ publication)));
            script.push_back(Ok(media_draw().map(|b| b ^ publication)));
        }
        if let Some(index) = fail_at {
            script[index] = Err(KeySourceError::Unavailable);
        }
        Arc::new(Self {
            script: Mutex::new(script),
            calls: Mutex::new(0),
        })
    }
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}
impl KeyByteSource for ScriptedKeys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        *self.calls.lock().unwrap() += 1;
        self.script
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(KeySourceError::Unavailable))
    }
}
fn record_draw() -> [u8; 32] {
    std::array::from_fn(|i| (i as u8).wrapping_mul(7).wrapping_add(3))
}
fn media_draw() -> [u8; 32] {
    std::array::from_fn(|i| (i as u8).wrapping_mul(11).wrapping_add(137))
}
fn scope(build: &str) -> Scope {
    Scope {
        container: format!("test.meguri.publication.{build}"),
        environment: "development".into(),
        account: "synthetic-account-a".into(),
    }
}
fn device_seed(dir: &Path) -> u64 {
    dir.file_name()
        .unwrap()
        .as_encoded_bytes()
        .iter()
        .fold(SEED, |seed, byte| seed.wrapping_mul(131).wrapping_add(u64::from(*byte)))
}
fn core_seed(dir: &Path, scope: &Scope) -> u64 {
    scope
        .container
        .bytes()
        .chain(scope.environment.bytes())
        .fold(device_seed(dir), |seed, byte| {
            seed.wrapping_mul(131).wrapping_add(u64::from(byte))
        })
}
fn generated(root: &Path, name: &str, nonempty: bool) -> PathBuf {
    let dir = root.join(name);
    let decks = if nonempty {
        "decks:\n  - name: Generic::Publication\n    notetype: basic\n    new: 1\n    due: 1\n    tags: [generic]\n    fields: [\"generic {n}\", \"back {n}\"]\n"
    } else {
        "decks: []\n"
    };
    let recipe = Recipe::parse(&format!(
        "recipe: publication\ndescription: Public synthetic publication\nnow: \"2025-01-01T00:00:00Z\"\n{decks}"
    ))
    .unwrap();
    let manifest = generate_recipe(&recipe, device_seed(&dir), &dir).unwrap();
    assert_eq!(manifest.counts.notes, if nonempty { 2 } else { 0 });
    if nonempty {
        assert!(manifest.counts.revlog > 0);
    }
    assert_eq!(std::fs::read_dir(dir.join("collection.media")).unwrap().count(), 0);
    dir
}
fn open(dir: &Path, scope: &Scope, now: i64, keys: Arc<ScriptedKeys>) -> PublicationRig {
    PublicationRig::open(
        dir,
        scope,
        IdentityReads::Available(scope.account.clone()),
        false,
        now,
        core_seed(dir, scope),
        keys,
    )
    .unwrap()
}
fn service(root: &Path) -> Service<SeededRandom> {
    Service::open(root, Config::default(), SeededRandom::new(SEED)).unwrap()
}
fn cursor(service: &Service<SeededRandom>) -> u64 {
    service.request_log().unwrap().last().map_or(0, |r| r.request_id)
}
fn no_mutations_since(service: &Service<SeededRandom>, after: u64) {
    assert!(!service.request_log().unwrap().iter().any(|r| r.request_id > after
        && matches!(
            r.kind,
            RequestKind::CreateZone | RequestKind::DeleteZone | RequestKind::Modify
        )));
}
fn server_record(service: &Service<SeededRandom>, scope: &Scope, zone: &str, name: &str) -> Record {
    service.inspect(scope).unwrap().unwrap().zones[zone].records[name].clone()
}
fn encrypted(record: &Record) -> &[u8] {
    let Field::EncryptedBytes(e) = &record.fields["e"] else {
        panic!("actual encrypted field required")
    };
    e
}
fn active(service: &Service<SeededRandom>, scope: &Scope) -> control_codec::Active {
    control_codec::Active::decode(encrypted(&server_record(service, scope, "meguri-root", "active"))).unwrap()
}
fn supported_epoch(bytes: &[u8]) -> Epoch {
    match Epoch::decode(bytes).unwrap() {
        control_codec::DecodeOutcome::Supported(epoch) => epoch,
        control_codec::DecodeOutcome::UnsupportedFormat(format) => {
            panic!("supported epoch format required, got {format}")
        }
    }
}
fn supported_media_root(bytes: &[u8]) -> MediaRoot {
    match MediaRoot::decode(bytes).unwrap() {
        control_codec::DecodeOutcome::Supported(root) => root,
        control_codec::DecodeOutcome::UnsupportedFormat(format) => {
            panic!("supported media-root format required, got {format}")
        }
    }
}
fn payloads(rows: &[codec::Payload], key: &[u8; 32]) -> BTreeMap<String, codec::Payload> {
    rows.iter()
        .map(|p| {
            (
                codec::entity_name(key, p.kind(), &row_payload::payload_key(p).unwrap()).unwrap(),
                p.clone(),
            )
        })
        .collect()
}
fn counts(rows: &[codec::Payload]) -> BTreeMap<codec::Kind, u64> {
    (1..=9)
        .map(|code| {
            let kind = codec::Kind::try_from(code).unwrap();
            (kind, rows.iter().filter(|p| p.kind() == kind).count() as u64)
        })
        .collect()
}
fn assert_scan(rig: &PublicationRig, rows: &[codec::Payload]) {
    for (kind, count) in counts(rows) {
        assert!(rig.observations().iter().any(|o| matches!(o,
            Observation::ReconcileKindCompleted { kind: k, row_count }
            if *k == kind && *row_count as u64 == count)));
    }
    assert!(
        rig.observations()
            .iter()
            .any(|o| matches!(o, Observation::MediaFeedDrained { entry_count: 0, .. }))
    );
}
fn no_scan(rig: &PublicationRig) {
    no_scan_since(rig, 0);
}
fn no_scan_since(rig: &PublicationRig, after: usize) {
    assert!(!rig.observations()[after..].iter().any(|o| matches!(
        o,
        Observation::ReconcileKindCompleted { .. } | Observation::BatchBuilt { .. }
    )));
}
fn assert_server_rows(service: &Service<SeededRandom>, rig: &mut PublicationRig) {
    let facts = rig.facts().unwrap();
    let binding = facts.current.as_ref().unwrap();
    // Whole union: collection tables plus independent journal/file media rows.
    let mut rows = rig.driver().collection_rows().unwrap();
    rows.extend(rig.driver().media_rows().unwrap());
    assert!(rows.len() <= 2048);
    assert!(rows.iter().map(|p| p.encode().unwrap().len()).sum::<usize>() <= 8 * 1024 * 1024);
    let expected = payloads(&rows, &binding.facts.record_key.0);
    assert_eq!(expected.len(), rows.len());
    let db = service.inspect(rig.scope()).unwrap().unwrap();
    let zone = &db.zones[&rig.collection_zone().unwrap()];
    let records: BTreeMap<_, _> = zone
        .records
        .iter()
        .filter(|(_, r)| r.record_type == "MGEntity")
        .map(|(n, r)| (n.clone(), r))
        .collect();
    assert_eq!(records.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
    for (name, p) in expected {
        let record = records[&name];
        let received = codec::receive(
            &binding.facts.record_key.0,
            &name,
            encrypted(record),
            codec::AssetInput::Absent,
        )
        .unwrap();
        assert_eq!(received.payload.as_ref(), Some(&p));
        assert_eq!(received.entity.payload_sha256, p.payload_digest().unwrap());
        let tracked = &facts.entities[&name];
        assert_eq!(tracked.acked_rev, tracked.local_rev);
        assert_eq!(tracked.acked_system_fields, record.system_fields.bytes());
        assert_eq!(tracked.acked_row_digest, p.row_digest().unwrap());
        assert_eq!(tracked.acked_payload_digest, p.payload_digest().unwrap());
        assert!(tracked.mark.is_none());
    }
}
fn publish(rig: &mut PublicationRig, service: &mut Service<SeededRandom>) {
    let preview = rig.preview(service, STEPS).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(service, STEPS).unwrap();
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
    assert_server_rows(service, rig);
}
#[test]
fn collection_rows_stay_sql_only_while_media_rows_verify_retained_file_hashes() {
    let run = tempfile::tempdir().unwrap();
    let dir = generated(run.path(), "media-observation", true);
    let scope = scope("media-observation");
    let mut rig = open(&dir, &scope, ORIGIN, ScriptedKeys::new(None));
    let sql_rows = rig.driver().collection_rows().unwrap();
    assert!(sql_rows.iter().all(|row| row.kind() != codec::Kind::MediaName));
    assert!(rig.driver().media_rows().unwrap().is_empty());
    rig.driver().put_media("withheld.bin", b"abc").unwrap();
    assert_eq!(rig.driver().collection_rows().unwrap(), sql_rows);
    let media_rows = rig.driver().media_rows().unwrap();
    let [codec::Payload::MediaName(media)] = media_rows.as_slice() else {
        panic!("one independent MediaName row required");
    };
    assert_eq!(media.name, b"withheld.bin");
    assert_eq!(media.size, 3);
    let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    // Published hash vectors for the actual synthetic bytes, not tracking data.
    assert_eq!(hex(&media.sha1), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        hex(&media.sha256),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(rig.driver().media_rows().unwrap(), media_rows);
    assert_eq!(rig.driver().collection_rows().unwrap(), sql_rows);
    assert_eq!(rig.driver().media_files().unwrap()["withheld.bin"], b"abc");
    rig.close_clean().unwrap();
}

/// V13.1-5, K2/K18.4, Q2/Q3/Q5 isolation: discovery never connects or merges.
#[test]
fn discovery_reads_actual_cloud_without_binding_adoption_merge_or_publication() {
    for cloud_nonempty in [false, true] {
        for local_nonempty in [false, true] {
            let run = tempfile::tempdir().unwrap();
            let mut service = service(run.path());
            let scope = scope("discovery");
            if cloud_nonempty {
                let remote = generated(run.path(), "remote", true);
                let mut publisher = open(&remote, &scope, ORIGIN, ScriptedKeys::new(None));
                publish(&mut publisher, &mut service);
                publisher.close_clean().unwrap();
            }
            let dir = generated(run.path(), "local", local_nonempty);
            let keys = ScriptedKeys::new(None);
            let mut rig = open(&dir, &scope, ORIGIN, keys.clone());
            let before_rows = rig.driver().collection_rows().unwrap();
            let before = rig.facts().unwrap();
            assert!(before.current.is_none() && before.bindings.is_empty());
            let server_before = service.inspect(&scope).unwrap();
            let after = cursor(&service);
            let preview = rig.preview(&mut service, STEPS).unwrap();
            assert_eq!(preview.local.counts, counts(&before_rows));
            assert_eq!(
                preview.decision,
                match (cloud_nonempty, local_nonempty) {
                    (false, _) => ConnectDecision::ConfirmUpload,
                    (true, false) => ConnectDecision::ConfirmAdoption,
                    (true, true) => ConnectDecision::ChooseCollection,
                }
            );
            if cloud_nonempty {
                assert_eq!(preview.cloud.as_ref().unwrap().counts[&codec::Kind::Note], 2);
                assert!(preview.cloud.as_ref().unwrap().counts[&codec::Kind::Revlog] > 0);
            } else {
                assert!(preview.cloud.is_none());
            }
            rig.cancel_preview().unwrap();
            rig.drive_ready(&mut service, STEPS).unwrap();
            assert_eq!(rig.driver().collection_rows().unwrap(), before_rows);
            let after_facts = rig.facts().unwrap();
            assert!(after_facts.bindings.is_empty() && after_facts.current.is_none());
            assert!(after_facts.decision == before.decision);
            assert_eq!(
                service.inspect(&scope).unwrap().map(|db| db.zones).unwrap_or_default(),
                server_before.map(|db| db.zones).unwrap_or_default()
            );
            no_mutations_since(&service, after);
            assert!(service.request_log().unwrap().iter().any(|r| r.request_id > after
                && r.kind == RequestKind::Read
                && r.zone.as_deref() == Some("meguri-root")));
            assert_eq!(keys.calls(), 0);
            no_scan(&rig);
            rig.close_clean().unwrap();
        }
    }
}

/// K2/K4/K5/K10/K11/K18.3, V15/V26, P6, Q2/Q3/Q5 history/confirmed-root.
#[test]
fn confirmed_initial_publication_scans_unacked_rows_saves_real_batches_and_switches_last() {
    let run = tempfile::tempdir().unwrap();
    let dir = generated(run.path(), "local", true);
    let scope = scope("initial");
    let mut service = service(run.path());
    let keys = ScriptedKeys::new(None);
    let mut rig = open(&dir, &scope, ORIGIN, keys.clone());
    let rows = rig.driver().collection_rows().unwrap();
    let header = rig.driver().collection_header().unwrap();
    for kind in [
        codec::Kind::Note,
        codec::Kind::Card,
        codec::Kind::Revlog,
        codec::Kind::Deck,
        codec::Kind::Preset,
        codec::Kind::Notetype,
        codec::Kind::Config,
    ] {
        assert!(rows.iter().any(|p| p.kind() == kind));
    }
    let identity = rig.facts().unwrap().decision.publication.identity;
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::ScanCommitted).unwrap();
    let op_id = rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::ScanCommitted));
    let scanned = rig.facts().unwrap();
    let binding = scanned.current.clone().unwrap();
    let op = &scanned.recovery_ops[0];
    assert_eq!(op.id, op_id);
    assert_eq!(op.step, PublishStep::Zones);
    assert_eq!(op.epoch, binding.facts.epoch);
    assert_eq!(binding.facts.publication, identity);
    assert!(binding.facts.account.0 == scope.account.as_bytes());
    assert_eq!(binding.facts.state, BindingState::Active);
    assert!(scanned.root_acked && scanned.media_key.is_some());
    assert!(scanned.decision.publication.decision == PublicationDecision::Clear);
    assert_scan(&rig, &rows); // all nine, including zero-row media names
    let expected = payloads(&rows, &binding.facts.record_key.0);
    assert_eq!(
        scanned.entities.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    for (name, expected) in &expected {
        let e = &scanned.entities[name];
        assert_eq!(&e.payload, expected);
        assert!(e.local_rev > 0 && e.acked_rev == 0 && e.server_erev == 0);
        assert!(e.acked_system_fields.is_empty() && e.acked_payload.is_none());
        assert!(e.mark.is_some());
    }
    let zone = rig.collection_zone().unwrap();
    let server = service.inspect(&scope).unwrap().unwrap();
    assert_eq!(
        server.zones.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([zone.clone(), "meguri-root".into(), "meguri-media".into()])
    );
    assert_eq!(server.zones[&zone].records.len(), 1); // only real publishing epoch
    assert!(server.zones["meguri-root"].records.is_empty());
    let epoch = supported_epoch(encrypted(&server_record(&service, &scope, &zone, "epoch")));
    assert_eq!(epoch.state, EpochState::Publishing);
    assert_eq!(epoch.crt, header.crt);
    assert_eq!(epoch.ver, 18);
    assert!(epoch.counts.is_empty());
    assert!(epoch.record_key == binding.facts.record_key.0);
    let media = supported_media_root(encrypted(&server_record(&service, &scope, "meguri-media", "root")));
    assert!(Some(media.media_key) == scanned.media_key.as_ref().map(|k| k.0));
    let creates: Vec<_> = service
        .request_log()
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == RequestKind::CreateZone)
        .collect();
    assert_eq!(creates.len(), 3);
    assert!(creates.iter().all(|r| r.caller.instance.is_none()
        && r.caller.own_operation.is_some()
        && r.caller.own_operation == creates[0].caller.own_operation
        && r.outcome == LogOutcome::Success));
    assert!(rig.sdk_database_pending().is_empty());

    rig.release(Point::ScanCommitted).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_server_rows(&service, &mut rig);
    let complete = supported_epoch(encrypted(&server_record(&service, &scope, &zone, "epoch")));
    assert_eq!(complete.state, EpochState::Complete);
    let complete_counts: BTreeMap<_, _> = complete.counts.iter().map(|c| (c.kind, c.live)).collect();
    assert_eq!(complete_counts.len(), complete.counts.len());
    assert_eq!(complete_counts, counts(&rows));
    assert_eq!(complete.reason, EpochReason::Initial);
    assert!(complete.record_key == binding.facts.record_key.0);
    let landed = active(&service, &scope);
    assert_eq!(landed.epoch, binding.facts.epoch.0);
    assert_eq!(landed.previous, None);
    let logs = service.request_log().unwrap();
    let outcomes = service.outcomes().unwrap();
    let mut accepted = BTreeSet::new();
    let mut complete_request = None;
    let mut active_request = None;
    let mut last_entity_request = 0;
    for log in logs.iter().filter(|r| r.kind == RequestKind::Modify) {
        let sent = rig.sent_request(log.request_id).unwrap();
        let outcome = outcomes.iter().find(|o| o.request_id == log.request_id).unwrap();
        let Ok(ReplyReference::Modified(items)) = &outcome.result else {
            panic!("actual Service modify response required")
        };
        for item in items {
            let reference = item.result.as_ref().unwrap().as_ref().unwrap();
            let record = service.resolve_record(&scope, reference).unwrap();
            let Mutation::Save {
                record: input,
                if_unchanged,
            } = sent
                .mutations
                .iter()
                .find(|m| matches!(m, Mutation::Save { record, .. } if record.name == item.name))
                .unwrap()
            else {
                panic!("actual conditional save required")
            };
            assert_eq!(input.fields, record.fields);
            match record.record_type.as_str() {
                "MGEntity" => {
                    assert!(log.caller.own_operation.is_none() && log.caller.instance.is_some());
                    assert!(if_unchanged.is_none()); // SDK conditional-new, never unconditional overwrite
                    assert!(accepted.insert(record.name.clone()));
                    let received = codec::receive(
                        &binding.facts.record_key.0,
                        &record.name,
                        encrypted(&record),
                        codec::AssetInput::Absent,
                    )
                    .unwrap();
                    assert_eq!(received.entity.fields.erev, 1);
                    assert!(!received.entity.fields.tombstone);
                    assert_eq!(received.entity.fields.origin, scanned.entities[&record.name].origin);
                    assert_eq!(received.entity.fields.writer, binding.id.0);
                    assert_eq!(received.entity.fields.base_sha256, None);
                    assert_eq!(received.payload.as_ref(), expected.get(&record.name));
                    last_entity_request = log.request_id;
                }
                "MGEpoch" => {
                    assert!(log.caller.own_operation.is_some() && log.caller.instance.is_none());
                    if supported_epoch(encrypted(&record)).state == EpochState::Complete {
                        assert!(*if_unchanged == Some(server.zones[&zone].records["epoch"].system_fields.clone()));
                        complete_request = Some(log.request_id);
                    } else {
                        assert!(if_unchanged.is_none());
                    }
                }
                "MGMediaRoot" | "MGActive" => {
                    assert!(log.caller.own_operation.is_some() && log.caller.instance.is_none());
                    assert!(if_unchanged.is_none());
                    if record.record_type == "MGActive" {
                        active_request = Some(log.request_id);
                    }
                }
                _ => panic!("unexpected publication record type"),
            }
        }
    }
    assert_eq!(accepted, expected.keys().cloned().collect());
    assert!(last_entity_request < complete_request.unwrap());
    assert!(complete_request.unwrap() < active_request.unwrap());
    assert_eq!(
        logs.iter()
            .rfind(|r| matches!(r.kind, RequestKind::CreateZone | RequestKind::Modify))
            .unwrap()
            .request_id,
        active_request.unwrap()
    );
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
    assert_eq!(keys.calls(), 2);
    rig.close_clean().unwrap();
    let mut reopened = open(&dir, &scope, ORIGIN, keys.clone());
    reopened.drive_ready(&mut service, STEPS).unwrap();
    assert!(reopened.facts().unwrap().current.as_ref() == Some(&binding));
    assert_eq!(reopened.driver().collection_rows().unwrap(), rows);
    assert_eq!(keys.calls(), 2);
    assert_server_rows(&service, &mut reopened);
    reopened.close_clean().unwrap();
}

/// P24/Q11 CONFIRMED: K5.2/K18.3.5/K10.3 zones/V2.3. Real clock and cleanup.
#[test]
fn p24_active_lost_reply_preserves_attempt_fields_and_done_restarts_thirty_day_retention() {
    for unreadable_old_active in [false, true] {
        let run = tempfile::tempdir().unwrap();
        let scope = scope("retention");
        let mut service = service(run.path());
        let prior_dir = generated(run.path(), "prior", true);
        let mut prior = open(&prior_dir, &scope, ORIGIN, ScriptedKeys::new(None));
        publish(&mut prior, &mut service);
        let e0 = active(&service, &scope).epoch;
        let old_zone = prior.collection_zone().unwrap();
        prior.close_clean().unwrap();
        let dir = generated(run.path(), "publisher", true);
        let keys = ScriptedKeys::new(None);
        let mut rig = open(&dir, &scope, ORIGIN, keys.clone());
        let preview = if unreadable_old_active {
            rig.preview_with_active_fields(&mut service, RequestedFields::Named(vec![]), STEPS)
                .unwrap()
        } else {
            rig.preview(&mut service, STEPS).unwrap()
        };
        rig.hold(Point::BeforeEpochRead).unwrap();
        rig.confirm_publication(&preview).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(rig.is_held(Point::BeforeEpochRead));
        assert_eq!(rig.facts().unwrap().zones[&old_zone].first_seen_ms, ORIGIN);
        rig.advance_clock_ms(10 * DAY + 123).unwrap();
        rig.hold(Point::BeforeActiveSave).unwrap();
        rig.release(Point::BeforeEpochRead).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(rig.is_held(Point::BeforeActiveSave));
        let request = rig.held_request().unwrap();
        let before = rig.facts().unwrap();
        let e1 = before.current.as_ref().unwrap().facts.epoch.0;
        assert_ne!(e1, e0);
        let Operation::Modify { zone, changes, .. } = &request.operation else {
            panic!("original own Active save required")
        };
        assert_eq!(zone, "meguri-root");
        let [Mutation::Save { record, if_unchanged }] = changes.as_slice() else {
            panic!("single conditional Active save required")
        };
        let built = control_codec::Active::decode(match &record.fields["e"] {
            Field::EncryptedBytes(e) => e,
            _ => panic!("encrypted Active required"),
        })
        .unwrap();
        assert_eq!(built.epoch, e1);
        assert_eq!(built.previous, if unreadable_old_active { None } else { Some(e0) });
        assert_eq!(built.activated_at_ms, ORIGIN + 10 * DAY + 123);
        assert!(if_unchanged.as_ref() == Some(&server_record(&service, &scope, "meguri-root", "active").system_fields));
        let reply = service.request(&scope, request.caller, request.operation).unwrap();
        assert!(reply.result.is_ok());
        let landed_record = server_record(&service, &scope, "meguri-root", "active");
        assert!(active(&service, &scope) == built);
        rig.kill().unwrap(); // never deliver reply: real server commit, actual lost response
        let done = ORIGIN + 11 * DAY + 123;
        let mut rig = open(&dir, &scope, done, keys.clone());
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(rig.facts().unwrap().recovery_ops.is_empty());
        assert!(rig.facts().unwrap().current == before.current);
        assert_eq!(server_record(&service, &scope, "meguri-root", "active"), landed_record);
        assert!(active(&service, &scope) == built); // earlier attempt's time/previous win
        assert_eq!(keys.calls(), 2);
        assert_eq!(rig.facts().unwrap().zones[&old_zone].first_seen_ms, done);
        rig.advance_clock_ms(29 * DAY).unwrap();
        rig.sync_now_and_drain(&mut service, STEPS).unwrap();
        assert!(!rig.facts().unwrap().zones[&old_zone].deletion_due);
        rig.advance_clock_ms(DAY - 1).unwrap();
        rig.sync_now_and_drain(&mut service, STEPS).unwrap();
        assert!(!rig.facts().unwrap().zones[&old_zone].deletion_due);
        assert!(
            service.inspect(&scope).unwrap().unwrap().zones[&old_zone]
                .removed
                .is_none()
        );
        rig.hold(Point::BeforeZoneDeletion).unwrap();
        rig.advance_clock_ms(1).unwrap();
        rig.sync_now_and_drain(&mut service, STEPS).unwrap();
        assert!(rig.is_held(Point::BeforeZoneDeletion));
        assert!(rig.facts().unwrap().zones[&old_zone].deletion_due);
        rig.release(Point::BeforeZoneDeletion).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(
            service.inspect(&scope).unwrap().unwrap().zones[&old_zone]
                .removed
                .is_some()
        );
        assert!(!rig.facts().unwrap().zones.contains_key(&old_zone));
        assert!(
            service
                .request_log()
                .unwrap()
                .iter()
                .filter(|r| r.kind == RequestKind::DeleteZone)
                .all(|r| r.caller.own_operation.is_some() && r.caller.instance.is_none())
        );
        rig.close_clean().unwrap();
    }
}

/// K18.3.5/V15.2.4-5/V26.1: own complete epoch read, then conditional switch.
#[test]
fn deleted_own_epoch_or_zone_blocks_switch_and_active_conflict_preserves_other_publisher() {
    for variant in 0..3 {
        let run = tempfile::tempdir().unwrap();
        let scope = scope("switch-safety");
        let mut service = service(run.path());
        let dir = generated(run.path(), "publisher", true);
        let mut rig = open(&dir, &scope, ORIGIN, ScriptedKeys::new(None));
        let point = if variant == 2 {
            Point::BeforeActiveSave
        } else {
            Point::BeforeEpochRead
        };
        rig.hold(point).unwrap();
        let preview = rig.preview(&mut service, STEPS).unwrap();
        rig.confirm_publication(&preview).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(rig.is_held(point));
        let zone = rig.collection_zone().unwrap();
        assert_eq!(
            supported_epoch(encrypted(&server_record(&service, &scope, &zone, "epoch"))).state,
            EpochState::Complete
        );
        let other_active = if variant == 2 {
            let other_dir = generated(run.path(), "other-publisher", true);
            let mut other = open(&other_dir, &scope, ORIGIN, ScriptedKeys::new(None));
            publish(&mut other, &mut service); // actual competing production publication
            other.close_clean().unwrap();
            Some(server_record(&service, &scope, "meguri-root", "active"))
        } else {
            let operation = if variant == 0 {
                Operation::Modify {
                    zone: zone.clone(),
                    atomic: true,
                    changes: vec![Mutation::Delete { name: "epoch".into() }],
                }
            } else {
                Operation::DeleteZone {
                    zone: zone.clone(),
                    cause: RemovalCause::Deleted,
                }
            };
            let response = service
                .request(
                    &scope,
                    Caller {
                        device: Some("synthetic-remover".into()),
                        ..Caller::default()
                    },
                    operation,
                )
                .unwrap();
            assert!(response.result.is_ok());
            None
        };
        let after = cursor(&service);
        rig.release(point).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        let facts = rig.facts().unwrap();
        assert_eq!(facts.recovery_ops.len(), 1);
        assert_eq!(facts.recovery_ops[0].step, PublishStep::Complete);
        if let Some(other_active) = other_active {
            assert_eq!(server_record(&service, &scope, "meguri-root", "active"), other_active);
            assert!(facts.recovery_ops[0].switch_conflict.is_some());
            let stopped_at = cursor(&service);
            rig.advance_clock_ms(DAY).unwrap();
            rig.sync_now_and_drain(&mut service, STEPS).unwrap();
            assert!(!service.request_log().unwrap().iter().any(|r| r.request_id > stopped_at
                && r.kind == RequestKind::Modify
                && r.zone.as_deref() == Some("meguri-root")));
        } else {
            assert_eq!(
                facts.current.unwrap().facts.state,
                BindingState::Isolated(IsolationReason::ZoneMissing)
            );
            assert!(!service.request_log().unwrap().iter().any(|r| r.request_id > after
                && r.kind == RequestKind::Modify
                && r.zone.as_deref() == Some("meguri-root")));
            assert!(
                service.inspect(&scope).unwrap().unwrap().zones["meguri-root"]
                    .records
                    .is_empty()
            );
        }
        rig.close_clean().unwrap();
    }
}

/// P24/Q11 OPEN P6: V2.3/2.4.3,7-9/V8.2/V15.2 confirmed/V15.3.
#[test]
fn confirmed_restart_requires_exact_account_publication_and_native_file_authority() {
    for variant in 0..5 {
        let run = tempfile::tempdir().unwrap();
        let dir = generated(run.path(), "local", true);
        let scope = scope("confirmed");
        let mut service = service(run.path());
        let mut rig = open(&dir, &scope, ORIGIN, ScriptedKeys::new(None));
        publish(&mut rig, &mut service);
        let backup = rig.driver().backup_now().unwrap();
        let before = rig.facts().unwrap();
        let preview = rig.preview(&mut service, STEPS).unwrap();
        rig.hold(Point::Confirmed).unwrap();
        rig.confirm_publication(&preview).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(rig.is_held(Point::Confirmed));
        let confirmed = rig.facts().unwrap();
        let op = &confirmed.recovery_ops[0];
        assert_eq!(op.step, PublishStep::Confirmed);
        assert!(op.binding_after.is_none());
        assert!(op.account.0 == scope.account.as_bytes());
        assert_eq!(op.confirmed_publication, Some(before.decision.publication.identity));
        assert_eq!(op.confirmed_file, Some(before.native_file));
        assert!(confirmed.bindings == before.bindings);
        if variant == 4 {
            rig.driver().restore_backup(&backup).unwrap();
        }
        rig.kill().unwrap();
        if variant == 3 {
            PublicationRig::replace_native_file_offline(&dir, &scope, ORIGIN, core_seed(&dir, &scope), &confirmed)
                .unwrap();
        }
        let after = cursor(&service);
        let identity = match variant {
            1 => IdentityReads::FailAllSession,
            2 => IdentityReads::Available(format!("{}-different", scope.account)),
            _ => IdentityReads::Available(scope.account.clone()),
        };
        let mut rig = PublicationRig::open(
            &dir,
            &scope,
            identity,
            false,
            ORIGIN,
            core_seed(&dir, &scope),
            ScriptedKeys::new(None),
        )
        .unwrap();
        rig.hold(Point::BeforeScan).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        let resumed = rig.facts().unwrap();
        match variant {
            0 => {
                assert!(rig.is_held(Point::BeforeScan));
                assert_eq!(resumed.recovery_ops[0].step, PublishStep::Zones);
                assert!(resumed.current.as_ref().unwrap().id != before.current.as_ref().unwrap().id);
                assert!(resumed.decision.publication.decision == PublicationDecision::Clear);
                assert_eq!(
                    resumed.current.as_ref().unwrap().facts.publication,
                    op.confirmed_publication.unwrap()
                );
            }
            1 => {
                assert_eq!(resumed.recovery_ops[0].step, PublishStep::Confirmed);
                assert!(resumed.bindings == before.bindings);
                assert!(resumed.decision == before.decision);
                no_mutations_since(&service, after);
            }
            _ => {
                assert!(resumed.recovery_ops.is_empty());
                assert_eq!(resumed.bindings.len(), before.bindings.len());
                let state = if variant == 2 {
                    IsolationReason::AccountChanged
                } else if variant == 3 {
                    IsolationReason::FileMismatch
                } else {
                    IsolationReason::ReplacedLocally
                };
                assert_eq!(
                    resumed.current.as_ref().unwrap().facts.state,
                    BindingState::Isolated(state)
                );
                if variant == 4 {
                    assert!(resumed.decision.publication.decision == PublicationDecision::Pending);
                } else {
                    assert!(resumed.decision == before.decision);
                }
                no_mutations_since(&service, after);
            }
        }
        no_scan(&rig); // BeforeScan holds the valid case; every invalid case forbids it
        assert!(
            !rig.binding_inputs()
                .iter()
                .any(|b| b.publish_step == Some(PublishStep::Confirmed))
        );
        rig.close_clean().unwrap();
    }
}

/// P24/Q11 OPEN P6/V15.3 and preview provenance V13.4/V17.6, R6.7.
#[test]
fn cancel_after_file_mismatch_or_restore_preserves_isolation_fence_and_backup_notices() {
    for external in [false, true] {
        let run = tempfile::tempdir().unwrap();
        let dir = generated(run.path(), "local", true);
        let scope = scope("cancel");
        let mut service = service(run.path());
        let mut rig = open(&dir, &scope, ORIGIN, ScriptedKeys::new(None));
        publish(&mut rig, &mut service);
        let old = rig.facts().unwrap().current.unwrap();
        let old_zone = rig.collection_zone().unwrap();
        let backup = rig.driver().backup_now().unwrap();
        let note = rig
            .driver()
            .collection_rows()
            .unwrap()
            .into_iter()
            .find_map(|p| if let codec::Payload::Note(n) = p { Some(n) } else { None })
            .unwrap();
        rig.driver()
            .edit_note_fields(note.id, &["unsent prior binding", "back"])
            .unwrap();
        let unsent_note = rig
            .driver()
            .collection_rows()
            .unwrap()
            .into_iter()
            .find_map(|p| {
                if let codec::Payload::Note(n) = p {
                    (n.id == note.id).then_some(n)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(unsent_note.flds.as_slice(), b"unsent prior binding\x1fback".as_slice());
        assert_ne!(unsent_note.flds, note.flds);
        let old_name = codec::entity_name(
            &old.facts.record_key.0,
            codec::Kind::Note,
            &row_payload::payload_key(&codec::Payload::Note(note.clone())).unwrap(),
        )
        .unwrap();
        let published = server_record(&service, &scope, &old_zone, &old_name);
        let received = codec::receive(
            &old.facts.record_key.0,
            &old_name,
            encrypted(&published),
            codec::AssetInput::Absent,
        )
        .unwrap();
        assert_eq!(received.payload.as_ref(), Some(&codec::Payload::Note(note.clone())));
        let prior_pending = rig
            .facts()
            .unwrap()
            .entities
            .values()
            .filter(|e| e.mark.is_some())
            .count();
        assert!(prior_pending > 0);
        let preview = rig.preview(&mut service, STEPS).unwrap();
        rig.hold(Point::BeforeScan).unwrap();
        let op = rig.confirm_publication(&preview).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(rig.is_held(Point::BeforeScan));
        let mut observation_start = rig.observations().len();
        let safety_backup = if external {
            let before_replacement = rig.facts().unwrap();
            rig.kill().unwrap();
            PublicationRig::replace_native_file_offline(
                &dir,
                &scope,
                ORIGIN,
                core_seed(&dir, &scope),
                &before_replacement,
            )
            .unwrap();
            rig = open(&dir, &scope, ORIGIN, ScriptedKeys::new(None));
            observation_start = 0;
            None
        } else {
            rig.verify_identity("synthetic-account-b".into()).unwrap();
            let restored = rig.driver().restore_backup(&backup).unwrap();
            assert_eq!(restored.name, backup.name);
            let name = restored
                .safety_backup
                .expect("restore must preserve the unpublished edit");
            assert_ne!(name, backup.name);
            assert!(rig.driver().backup_path(&name).unwrap().is_file());
            let saved_rows = rig.driver().backup_collection_rows(&name).unwrap();
            assert!(saved_rows.iter().any(|p| matches!(p, codec::Payload::Note(n)
                if n.id == unsent_note.id && n.flds == unsent_note.flds)));
            let restored_rows = rig.driver().collection_rows().unwrap();
            assert!(restored_rows.iter().any(|p| matches!(p, codec::Payload::Note(n)
                if n.id == note.id && n.flds == note.flds)));
            assert!(!restored_rows.iter().any(|p| matches!(p, codec::Payload::Note(n)
                if n.id == unsent_note.id && n.flds == unsent_note.flds)));
            Some(name)
        };
        let preserved = rig.facts().unwrap().decision;
        let after = cursor(&service);
        rig.cancel_recovery_op(op).unwrap();
        let cancelled = rig.facts().unwrap();
        let reason = if external {
            IsolationReason::FileMismatch
        } else {
            IsolationReason::ReplacedLocally
        };
        assert_eq!(cancelled.current.as_ref().unwrap().id, old.id);
        assert_eq!(
            cancelled.current.as_ref().unwrap().facts.state,
            BindingState::Isolated(reason)
        );
        assert!(cancelled.recovery_ops.is_empty());
        assert!(cancelled.decision == preserved);
        if !external {
            assert!(cancelled.decision.publication.decision == PublicationDecision::Pending);
            assert_eq!(cancelled.decision.pending_reason, Some(PendingDecisionReason::Restore));
            rig.verify_identity(scope.account.clone()).unwrap();
        }
        no_scan_since(&rig, observation_start);
        let preview = rig.preview(&mut service, STEPS).unwrap();
        assert!(preview.unpublished_by_kind[&codec::Kind::Note] > 0);
        assert!(preview.backup_notices.contains(&if external {
            BackupNotice::OutsideReplacementNoBackup
        } else {
            BackupNotice::ReplacementBackupRetained {
                name: safety_backup.as_ref().unwrap().clone(),
            }
        }));
        // Observe real retention removing the replacement's safety backup.
        if let Some(name) = safety_backup {
            let safety_path = rig.driver().backup_path(&name).unwrap();
            assert!(safety_path.is_file());
            rig.driver().set_backup_retention(0, 0, 0).unwrap();
            rig.advance_clock_ms(3 * DAY).unwrap();
            let retained = rig.preview(&mut service, STEPS).unwrap();
            assert!(
                retained
                    .backup_notices
                    .contains(&BackupNotice::ReplacementBackupRetained { name: name.clone() })
            );
            assert!(
                !retained
                    .backup_notices
                    .contains(&BackupNotice::ReplacementBackupExpired { name: name.clone() })
            );
            let result = rig.driver().backup_now().unwrap();
            assert!(result.deleted.contains(&name));
            assert_eq!(
                std::fs::metadata(&safety_path).unwrap_err().kind(),
                std::io::ErrorKind::NotFound
            );
            let expired = rig.preview(&mut service, STEPS).unwrap();
            assert!(
                expired
                    .backup_notices
                    .contains(&BackupNotice::ReplacementBackupExpired { name })
            );
            assert!(
                !expired
                    .backup_notices
                    .iter()
                    .any(|notice| matches!(notice, BackupNotice::ReplacementBackupRetained { .. }))
            );
        }
        assert_eq!(server_record(&service, &scope, &old_zone, &old_name), published);
        assert!(
            !service
                .request_log()
                .unwrap()
                .iter()
                .any(|r| r.request_id > after && r.kind == RequestKind::Modify)
        );
        assert!(!service.request_log().unwrap().iter().any(|r| r.request_id > after
            && r.zone.as_deref() == Some(old_zone.as_str())
            && r.kind == RequestKind::Modify));
        rig.close_clean().unwrap();
    }
}

/// P24/Q11 OPEN other-build decisions: V2.3/2.4/V21/V15.2-3/R6.7.
#[test]
fn other_build_decision_survives_cancel_and_all_three_bound_restart_cases() {
    for restart_case in ['a', 'b', 'c'] {
        let run = tempfile::tempdir().unwrap();
        let dir = generated(run.path(), "shared", true);
        let a = scope("build-a");
        let b = scope("build-b");
        let mut service = service(run.path());
        let mut rig = open(&dir, &a, ORIGIN, ScriptedKeys::new(None));
        publish(&mut rig, &mut service);
        let backup = rig.driver().backup_now().unwrap();
        rig.driver().restore_backup(&backup).unwrap();
        let pending = rig.facts().unwrap().decision;
        assert!(pending.publication.decision == PublicationDecision::Pending);
        assert_eq!(pending.pending_reason, Some(PendingDecisionReason::Restore));
        let point = if restart_case == 'a' {
            Point::BoundBeforeDecision
        } else {
            Point::BoundAfterDecision
        };
        rig.hold(point).unwrap();
        let preview = rig.preview(&mut service, STEPS).unwrap();
        let a_op = rig.confirm_publication(&preview).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
        assert!(rig.is_held(point));
        assert_eq!(rig.facts().unwrap().recovery_ops[0].step, PublishStep::Bound);
        rig.kill().unwrap();
        if restart_case != 'c' {
            // (a) prior decision still present: write it now; (b) our write is
            // already present: resume. Neither asks for confirmation again.
            rig = open(&dir, &a, ORIGIN, ScriptedKeys::new(None));
            rig.hold(Point::BeforeScan).unwrap();
            rig.drive_ready(&mut service, STEPS).unwrap();
            assert!(rig.is_held(Point::BeforeScan));
            assert_eq!(rig.facts().unwrap().recovery_ops[0].id, a_op);
            assert!(rig.facts().unwrap().decision.publication.decision == PublicationDecision::Clear);
            rig.close_clean().unwrap();
        }
        let a_server = service.inspect(&a).unwrap();
        let mut other = open(&dir, &b, ORIGIN, ScriptedKeys::new(None));
        assert!(other.facts().unwrap().recovery_ops.is_empty()); // not Busy on A's row
        let preview = other.preview(&mut service, STEPS).unwrap();
        other.hold(Point::Confirmed).unwrap();
        let b_op = other.confirm_publication(&preview).unwrap();
        other.drive_ready(&mut service, STEPS).unwrap();
        assert!(other.is_held(Point::Confirmed));
        other.kill().unwrap(); // B confirmed before its own binding/sidecar write
        other = open(&dir, &b, ORIGIN, ScriptedKeys::new(None));
        other.drive_ready(&mut service, STEPS).unwrap();
        assert!(other.facts().unwrap().recovery_ops.iter().all(|op| op.id != b_op));
        let b_facts = other.facts().unwrap();
        assert!(b_facts.decision.publication.decision == PublicationDecision::Clear);
        assert!(b_facts.current.as_ref().unwrap().facts.account.0 == b.account.as_bytes());
        assert_server_rows(&service, &mut other);
        assert_eq!(service.inspect(&a).unwrap(), a_server); // B did not settle/run A
        other.close_clean().unwrap();
        rig = open(&dir, &a, ORIGIN, ScriptedKeys::new(None));
        if restart_case == 'c' {
            rig.drive_ready(&mut service, STEPS).unwrap(); // sidecar changed: cancel, leave B
            assert!(rig.facts().unwrap().recovery_ops.is_empty());
        } else {
            assert!(rig.facts().unwrap().recovery_ops.iter().any(|op| op.id == a_op));
            rig.cancel_recovery_op(a_op).unwrap();
        }
        assert!(rig.facts().unwrap().decision == b_facts.decision);
        assert!(rig.facts().unwrap().recovery_ops.is_empty());
        rig.close_clean().unwrap();
        let mut other = open(&dir, &b, ORIGIN, ScriptedKeys::new(None));
        other.drive_ready(&mut service, STEPS).unwrap();
        assert!(other.facts().unwrap().decision == b_facts.decision);
        assert!(other.facts().unwrap().current == b_facts.current);
        assert!(other.recovery_state().unwrap().publication_fence_decision.is_none());
        assert_server_rows(&service, &mut other);
        other.close_clean().unwrap();
    }
}

/// P24/Q11 OPEN V17.6/V2.4.8/V15.2.2,4: enabled dependency RED, real media applies.
#[test]
fn prior_binding_media_and_resolution_intents_settle_before_new_publication_scan() {
    for resolution in [false, true] {
        let run = tempfile::tempdir().unwrap();
        let scope = scope("intent-order");
        let mut service = service(run.path());
        let sender_dir = generated(run.path(), "sender", true);
        let mut sender = open(&sender_dir, &scope, ORIGIN, ScriptedKeys::new(None));
        publish(&mut sender, &mut service);
        if !resolution {
            sender
                .driver()
                .put_media("photo.jpg", b"synthetic initial image")
                .unwrap();
            sender.sync_now_and_drain(&mut service, STEPS).unwrap();
        }
        let dir = generated(run.path(), "receiver", true);
        let mut receiver = open(&dir, &scope, ORIGIN, ScriptedKeys::new(None));
        let preview = receiver.preview(&mut service, STEPS).unwrap();
        receiver.adopt_cloud(&preview).unwrap(); // actual production adoption, not arrange
        receiver.drive_ready(&mut service, STEPS).unwrap();
        if resolution {
            assert!(!receiver.driver().media_files().unwrap().contains_key("photo.jpg"));
            assert!(!receiver.facts().unwrap().entities.values().any(|entity| matches!(
                &entity.payload,
                codec::Payload::MediaName(media) if media.name == b"photo.jpg"
            )));
        } else {
            assert_eq!(
                receiver.driver().media_files().unwrap()["photo.jpg"],
                b"synthetic initial image"
            );
        }
        let old = receiver.facts().unwrap().current.unwrap();
        if resolution {
            let note = receiver
                .driver()
                .collection_rows()
                .unwrap()
                .into_iter()
                .find_map(|p| if let codec::Payload::Note(n) = p { Some(n) } else { None })
                .unwrap();
            receiver
                .driver()
                .edit_note_fields(note.id, &["<img src=\"photo.jpg\">", "back"])
                .unwrap();
            receiver
                .driver()
                .put_media("photo.jpg", b"synthetic local image")
                .unwrap();
        }
        sender
            .driver()
            .put_media("photo.jpg", b"synthetic remote image")
            .unwrap();
        sender.sync_now_and_drain(&mut service, STEPS).unwrap();
        let point = if resolution {
            Point::ResolutionMoveBeforeRewrite
        } else {
            Point::MediaTrashBeforeInstall
        };
        receiver.hold(point).unwrap();
        receiver.sync_now_and_drain(&mut service, STEPS).unwrap();
        assert!(receiver.is_held(point));
        assert!(!receiver.driver().apply_intents(old.id).unwrap().is_empty());
        // End the running platform I/O with a real failure. The durable intent
        // stays; step 2 must never isolate a binding while its apply still runs.
        receiver.fail_held_io(point).unwrap();
        receiver.hold(Point::BoundAfterDecision).unwrap();
        let preview = receiver.preview(&mut service, STEPS).unwrap();
        receiver.confirm_publication(&preview).unwrap();
        receiver.drive_ready(&mut service, STEPS).unwrap();
        assert!(receiver.is_held(Point::BoundAfterDecision));
        receiver.kill().unwrap();
        receiver = open(&dir, &scope, ORIGIN, ScriptedKeys::new(None));
        receiver.hold(Point::BeforeScan).unwrap();
        receiver.drive_ready(&mut service, STEPS).unwrap();
        assert!(receiver.is_held(Point::BeforeScan));
        assert!(receiver.driver().apply_intents(old.id).unwrap().is_empty());
        let settled_files = receiver.driver().media_files().unwrap();
        assert!(settled_files.values().any(|v| v == b"synthetic remote image"));
        if resolution {
            let local_names: Vec<_> = settled_files
                .iter()
                .filter(|(_, v)| *v == b"synthetic local image")
                .map(|(name, _)| name.clone())
                .collect();
            assert_eq!(local_names.len(), 1);
            assert_ne!(local_names[0], "photo.jpg");
            let settled_rows = receiver.driver().collection_rows().unwrap();
            assert!(settled_rows.iter().any(|p| matches!(p, codec::Payload::Note(n)
                if std::str::from_utf8(&n.flds).unwrap().contains(&local_names[0]) &&
                    !std::str::from_utf8(&n.flds).unwrap().contains("src=\"photo.jpg\""))));
        }
        let rows = receiver.driver().collection_rows().unwrap();
        receiver.release(Point::BeforeScan).unwrap();
        receiver.drive_ready(&mut service, STEPS).unwrap();
        assert_server_rows(&service, &mut receiver);
        assert_eq!(receiver.driver().collection_rows().unwrap(), rows);
        assert!(receiver.facts().unwrap().recovery_ops.is_empty());
        assert!(receiver.driver().apply_intents(old.id).unwrap().is_empty());
        receiver.close_clean().unwrap();
        sender.close_clean().unwrap();
    }
}

/// K4.1/K9.2 prerequisite: no u64 expansion, truncation, fallback or partial commit.
#[test]
fn platform_full_width_independent_keys_survive_reopen_and_failure_precedes_every_write() {
    for fail_at in [None, Some(0), Some(1)] {
        let run = tempfile::tempdir().unwrap();
        let dir = generated(run.path(), "local", true);
        let scope = scope("entropy");
        let mut service = service(run.path());
        let keys = ScriptedKeys::new(fail_at);
        let mut rig = open(&dir, &scope, ORIGIN, keys.clone());
        let preview = rig.preview(&mut service, STEPS).unwrap();
        let before = rig.facts().unwrap();
        let after = cursor(&service);
        let result = rig.confirm_publication(&preview);
        if let Some(index) = fail_at {
            assert!(result.is_err());
            rig.drive_ready(&mut service, STEPS).unwrap();
            let failed = rig.facts().unwrap();
            assert!(failed.decision == before.decision);
            assert!(failed.current.is_none() && failed.bindings.is_empty());
            assert!(failed.recovery_ops.is_empty() && failed.entities.is_empty());
            assert!(failed.media_key.is_none() && !failed.root_acked);
            assert_eq!(keys.calls(), index + 1);
            no_mutations_since(&service, after);
            rig.close_clean().unwrap();
            let mut reopened = open(&dir, &scope, ORIGIN, keys.clone());
            reopened.drive_ready(&mut service, STEPS).unwrap();
            let durable = reopened.facts().unwrap();
            assert!(durable.decision == before.decision);
            assert!(durable.bindings.is_empty() && durable.recovery_ops.is_empty());
            assert!(durable.media_key.is_none());
            assert_eq!(keys.calls(), index + 1);
            no_mutations_since(&service, after);
            reopened.close_clean().unwrap();
        } else {
            assert!(result.is_ok());
            rig.drive_ready(&mut service, STEPS).unwrap();
            assert_server_rows(&service, &mut rig);
            let facts = rig.facts().unwrap();
            assert!(facts.current.as_ref().unwrap().facts.record_key.0 == record_draw());
            assert!(facts.media_key.as_ref().unwrap().0 == media_draw());
            assert!(record_draw() != media_draw());
            assert_eq!(keys.calls(), 2);
            rig.close_clean().unwrap();
            let mut reopened = open(&dir, &scope, ORIGIN + 1, keys.clone());
            reopened.drive_ready(&mut service, STEPS).unwrap();
            let durable = reopened.facts().unwrap();
            assert!(durable.current.as_ref().unwrap().facts.record_key.0 == record_draw());
            assert!(durable.media_key.as_ref().unwrap().0 == media_draw());
            let zone = reopened.collection_zone().unwrap();
            let epoch = supported_epoch(encrypted(&server_record(&service, &scope, &zone, "epoch")));
            let root = supported_media_root(encrypted(&server_record(&service, &scope, "meguri-media", "root")));
            assert!(epoch.record_key == record_draw());
            assert!(root.media_key == media_draw());
            assert_eq!(keys.calls(), 2);
            reopened.close_clean().unwrap();
        }
    }
}

// Shared subprocess entry only, never publication behavior evidence.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
