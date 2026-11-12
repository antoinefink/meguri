//! Expected-red prerequisites: K17.4/K13.1/K22.2, P3/P24, V3.1, Q11.
//! Master base 6c156ea851e2b3ace4d84201864bdab1b03c3399;
//! original U113 start 1dfe87bc7138adc058a1a8af9d0a3b2a0f99957a;
//! integration batch 1416a520542a8c5daa38a23bfe398da704c3d6c9.
//! Service owns pages/tokens; Host owns intake, apply, WAL receipts and tracking.
//! The only new production surface is a diagnostic event declaration, un-emitted.
//! No simulated Changes, injected pass/completion facts, or tracking SQL writes.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{Arc, mpsc},
    time::Duration,
};

use meguri_engine::{OpSource, device_state::AccountBinding};
use meguri_host::CoreHost;
use meguri_model::{ids::DeckId, rng::SeededRandom};
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, Table};
use meguri_sync::cloud::{
    self as cloud, InboxValue, OpaqueBytes,
    entity_codec::{self as codec, Payload},
    policy::{ChangeCause, DeviceEvent, DeviceSettings},
    runtime::{CloudInputs, IdentityProvider, IdentityStatus, Runtime},
    testing::IncomingStage,
    workflow::{Saved, TraceEvent},
};
use meguri_testkit::{
    cloud::{
        Caller, Config, Field, Mutation, MutationSuccess, Operation, Page, Record, RecordChange, RecordInput, Reply,
        RequestedFields, Scope, Service, Token, ZoneChange,
        devices::{NativeFiles, options},
    },
    generate::recipe::{Recipe, generate_recipe},
};

const NOW: i64 = 1_735_689_600_000;
const BUDGET: Duration = Duration::from_secs(30);
const CHILD_ENV: &str = "MEGURI_ABSENCE_CHILD";
const CHECKPOINT: &str = "ABSENCE_CHECKPOINT ";

struct Account;
impl IdentityProvider for Account {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        identity()
    }
}
fn identity() -> IdentityStatus {
    IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec()))
}
fn scope() -> Scope {
    Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    }
}
fn open(directory: &Path) -> CoreHost {
    let mut config = options(directory, NOW, 1717);
    config.publication_binding = AccountBinding::Connected {
        account: scope().account,
        configuration: scope().container,
        environment: scope().environment,
    };
    config.cloud = Some(CloudInputs {
        scope: cloud::BuildScope {
            container: cloud::ContainerId::new(scope().container).unwrap(),
            environment: cloud::Environment::Development,
        },
        identity: Arc::new(Account),
        files: Arc::new(NativeFiles),
    });
    CoreHost::open(config).unwrap()
}
fn admit(host: &CoreHost) -> (String, Option<Vec<u8>>, String) {
    // Same order as Host Rig: settings/account/network BEFORE registering SDK.
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true,
        paused: false,
        automatic: false,
        media: false,
    }))
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: identity(),
        origin_sdk: None,
    })
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Network { reachable: true })
        .unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let media = host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    runtime
        .reconcile_binding(&host.collection().unwrap(), &media, BUDGET)
        .unwrap();
    let sdk = runtime.register_sdk(false).unwrap();
    host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    let state = host.cloud_policy_driver().unwrap().state();
    assert!(
        state.gate_open() && state.engine_runs(),
        "fixture admission: {state:#?}"
    );
    runtime.validate_sdk(&sdk.0).unwrap();
    sdk
}
fn service(root: &Path) -> Service<SeededRandom> {
    Service::open(
        root,
        Config {
            page_size: 50,
            ..Config::default()
        },
        SeededRandom::new(1717),
    )
    .unwrap()
}
fn saved(record: &Record) -> Saved {
    let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
        panic!("encrypted record required")
    };
    Saved {
        name: record.name.clone(),
        envelope: envelope.clone(),
        system_fields: record.system_fields.bytes().to_vec(),
    }
}
fn decode(record: &Saved) -> (codec::Entity, Payload) {
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&record.envelope).unwrap() else {
        panic!("supported baseline")
    };
    let payload = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap();
    (entity, payload)
}
fn put(service: &mut Service<SeededRandom>, zone: &str, records: &[Saved], kind: &str) {
    for batch in records.chunks(250) {
        let response = service
            .request(
                &scope(),
                Caller::default(),
                Operation::Modify {
                    zone: zone.into(),
                    atomic: false,
                    changes: batch
                        .iter()
                        .map(|r| Mutation::Save {
                            record: RecordInput::encrypted(&r.name, kind, r.envelope.clone()),
                            if_unchanged: None,
                        })
                        .collect(),
                },
            )
            .unwrap();
        let Ok(Reply::Modified(items)) = response.result else {
            panic!("actual save receipt required")
        };
        assert_eq!(items.len(), batch.len());
        assert!(items.iter().all(|i| matches!(i.result, Ok(MutationSuccess::Saved(_)))));
    }
}
fn read(service: &mut Service<SeededRandom>, zone: &str, name: &str) -> Record {
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Read {
                zone: zone.into(),
                name: name.into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    let Ok(Reply::Record(record)) = response.result else {
        panic!("actual record read required")
    };
    record
}
fn page(service: &mut Service<SeededRandom>, zone: &str, token: Option<Token>) -> Page<RecordChange> {
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::FetchZone {
                zone: zone.into(),
                token,
            },
        )
        .unwrap();
    let Ok(Reply::ZoneChanges(page)) = response.result else {
        panic!("actual page required")
    };
    page
}
fn begin(runtime: &Runtime, sdk: &str, zone: &str) -> u64 {
    let id = runtime.adapter_begin_fetch(sdk, vec![zone.into()]).unwrap();
    runtime.adapter_will_fetch(sdk, format!("absence-{id}"), true).unwrap();
    runtime.adapter_fetch_zone(sdk, zone.into(), true, false).unwrap();
    id
}
fn finish(runtime: &Runtime, sdk: &str, zone: &str, id: u64, failed: bool) {
    runtime.adapter_fetch_zone(sdk, zone.into(), false, failed).unwrap();
    runtime.adapter_did_fetch(sdk, &format!("absence-{id}"), true).unwrap();
    assert_eq!(runtime.adapter_finish_fetch(sdk, id).unwrap(), !failed);
}
fn confirm_root(host: &CoreHost, sdk: &str, service: &mut Service<SeededRandom>) {
    let runtime = host.cloud_runtime().unwrap();
    let id = begin(runtime, sdk, "meguri-root");
    let p = page(service, "meguri-root", None);
    assert!(!p.more);
    let records: Vec<_> = p
        .changes
        .iter()
        .map(|c| match c {
            RecordChange::Saved(record) => saved(record),
            _ => panic!("live root required"),
        })
        .collect();
    runtime.adapter_fetched(sdk, "meguri-root", &records, &[]).unwrap();
    finish(runtime, sdk, "meguri-root", id, false);
    assert!(runtime.sdk_root_confirmed());
    let proof = runtime.adapter_snapshot().unwrap().root.unwrap();
    assert!(
        records
            .iter()
            .any(|r| r.envelope == proof.envelope.0 && r.system_fields == proof.system_fields.0)
    );
}
fn deliver(host: &CoreHost, sdk: &str, zone: &str, p: &Page<RecordChange>, apply: bool) -> BTreeSet<String> {
    let runtime = host.cloud_runtime().unwrap();
    let mut records = Vec::new();
    let mut deleted = Vec::new();
    for change in &p.changes {
        match change {
            RecordChange::Saved(record) => records.push(saved(record)),
            RecordChange::Deleted { name, .. } => deleted.push(name.clone()),
        }
    }
    runtime.adapter_fetched(sdk, zone, &records, &deleted).unwrap();
    // Opaque SDK state contains this Service's actual zone + pagination token.
    let bytes = serde_json::to_vec(&(zone, &p.token)).unwrap();
    runtime.adapter_state_update(sdk, &bytes).unwrap();
    assert_eq!(runtime.adapter_snapshot().unwrap().serialization, Some(bytes));
    if apply {
        let collection = host.collection().unwrap();
        for _ in 0..16 {
            if runtime.drain_incoming(&collection, BUDGET).unwrap() == 0 {
                break;
            }
        }
        if !p.more {
            assert!(
                runtime
                    .inbox_entries(runtime.status().unwrap().binding.unwrap().id)
                    .unwrap()
                    .is_empty()
            );
        }
    }
    records.into_iter().map(|r| r.name).chain(deleted).collect()
}
fn rows(host: &CoreHost) -> BTreeMap<Table, Vec<Vec<Cell>>> {
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    let mut out = BTreeMap::new();
    for table in Table::ALL {
        let mut images = Vec::new();
        let mut after = None;
        loop {
            let page = reader.row_page(table, after.as_ref()).unwrap();
            if page.is_empty() {
                break;
            }
            for row in page {
                let key = RowKey::from_stored(table, &row).unwrap();
                after = Some(key);
                images.push(row);
            }
        }
        out.insert(table, images);
    }
    out
}
fn safe(host: &CoreHost, before: &BTreeMap<Table, Vec<Vec<Cell>>>) {
    assert_eq!(
        &rows(host),
        before,
        "absence detection must not change actual collection rows"
    );
    let runtime = host.cloud_runtime().unwrap();
    assert!(
        runtime
            .adapter_snapshot()
            .unwrap()
            .inbox
            .iter()
            .all(|e| !matches!(e.value, InboxValue::Deletion))
    );
}
type AbsencePassLogEntry = (cloud::BindingId, String, [u8; 16], usize, BTreeSet<String>);

fn logged(runtime: &Runtime) -> Vec<AbsencePassLogEntry> {
    runtime
        .trace()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::AbsencePassCompleted {
                binding,
                zone,
                pass,
                absent_count,
                absent_names,
            } => Some((binding, zone, pass, absent_count, absent_names)),
            _ => None,
        })
        .collect()
}
fn completed(host: &CoreHost, zone: &str, expected: BTreeSet<String>) -> [u8; 16] {
    let runtime = host.cloud_runtime().unwrap();
    // Safety/content assertions at the caller run BEFORE these expected-red checks.
    assert!(
        runtime.adapter_snapshot().unwrap().absence_pass_completed,
        "K17.4: fetched:true must finish the durable absence pass"
    );
    let events = logged(runtime);
    assert_eq!(
        events.len(),
        1,
        "K17.4: one exact-name Copy Debug Info completion required"
    );
    let (binding, actual_zone, pass, count, names) = &events[0];
    assert_eq!(*binding, runtime.status().unwrap().binding.unwrap().id);
    assert_eq!(actual_zone, zone);
    assert_eq!(*count, expected.len());
    assert_eq!(names, &expected);
    *pass
}

struct Rig {
    root: tempfile::TempDir,
    host: CoreHost,
    service: Service<SeededRandom>,
    records: Vec<Saved>,
    sdk: String,
    zone: String,
}
fn fixture(notes: usize, saved_state: bool) -> Rig {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("device");
    let recipe = Recipe::parse(&format!("recipe: native-absence\ndescription: Synthetic absence prerequisites\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Absence\n    notetype: basic\n    new: {notes}\n    fields: [\"question {{n}}\", \"answer\"]\n")).unwrap();
    generate_recipe(&recipe, 1717, &directory).unwrap();
    let host = open(&directory);
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let config = options(&directory, NOW, 1717);
    let (mut records, active, _) = host
        .cloud_runtime()
        .unwrap()
        .prepare_adapter_baseline_for_testing(
            &host.collection().unwrap(),
            config.clock.as_ref(),
            config.random.as_ref(),
            saved_state.then(|| b"prior-sdk-state".to_vec()),
        )
        .unwrap();
    records.sort_by(|a, b| a.name.cmp(&b.name));
    let (sdk, serialization, zone) = admit(&host);
    assert_eq!(serialization.is_some(), saved_state);
    let mut service = service(root.path());
    for z in ["meguri-root", zone.as_str()] {
        assert!(matches!(
            service
                .request(&scope(), Caller::default(), Operation::CreateZone { zone: z.into() })
                .unwrap()
                .result,
            Ok(Reply::Done)
        ));
    }
    put(&mut service, "meguri-root", &[active], "MGActive");
    put(&mut service, &zone, &records, "MGEntity");
    confirm_root(&host, &sdk, &mut service);
    Rig {
        root,
        host,
        service,
        records,
        sdk,
        zone,
    }
}

#[test]
fn cursorless_first_fetch_half_zone_logs_exact_missing_acked_names_without_deletions() {
    let mut rig = fixture(60, false);
    let half = rig.records.len().div_ceil(2);
    assert!(half <= 200);
    assert!(matches!(
        rig.service
            .request(
                &scope(),
                Caller::default(),
                Operation::Configure(Config {
                    page_size: half,
                    ..Config::default()
                })
            )
            .unwrap()
            .result,
        Ok(Reply::Done)
    ));
    let collection = rig.host.collection().unwrap();
    let mid = rig
        .records
        .iter()
        .find_map(|r| match decode(r).1 {
            Payload::Note(n) => Some(n.mid),
            _ => None,
        })
        .unwrap();
    let added = meguri_engine::notes::add(
        &collection,
        meguri_engine::notes::AddNote {
            notetype_id: meguri_model::ids::NotetypeId(mid),
            fields: vec!["never acked".into(), "local".into()],
            tags: Vec::new(),
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value
    .id;
    let runtime = rig.host.cloud_runtime().unwrap();
    runtime
        .reconcile_binding(&collection, &rig.host.media().unwrap(), BUDGET)
        .unwrap();
    let never = codec::entity_name(&[114; 32], codec::Kind::Note, &codec::id_key(added.0)).unwrap();
    assert_eq!(runtime.entity_facts(&never).unwrap().unwrap().acked_rev, 0);
    let before = rows(&rig.host);
    let entities = runtime.adapter_snapshot().unwrap().entities;
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let p = page(&mut rig.service, &rig.zone, None);
    assert!(p.more, "fixture must have an unread half");
    let seen = deliver(&rig.host, &rig.sdk, &rig.zone, &p, true);
    assert_eq!(seen.len(), half);
    // Intentionally lie about A9a through the SDK event seam: no final page read.
    finish(runtime, &rig.sdk, &rig.zone, id, false);
    safe(&rig.host, &before);
    assert!(
        runtime.adapter_snapshot().unwrap().entities == entities,
        "half-zone absence must not alter tracked entities"
    );
    let expected = rig
        .records
        .iter()
        .map(|r| r.name.clone())
        .filter(|n| !seen.contains(n))
        .collect();
    assert!(!logged(runtime).iter().any(|e| e.4.contains(&never)));
    completed(&rig.host, &rig.zone, expected);
}

#[test]
fn same_identity_signin_after_zone_end_defers_completion_until_next_zone_start() {
    let mut rig = fixture(10, false);
    let before = rows(&rig.host);
    let runtime = rig.host.cloud_runtime().unwrap();
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let p = page(&mut rig.service, &rig.zone, None);
    assert!(!p.more);
    let seen = deliver(&rig.host, &rig.sdk, &rig.zone, &p, true);
    runtime
        .adapter_fetch_zone(&rig.sdk, rig.zone.clone(), false, false)
        .unwrap();
    let guard = runtime.adapter_guard();
    rig.host
        .cloud_policy_input(DeviceEvent::Account {
            identity: identity(),
            origin_sdk: Some(rig.sdk.clone()),
        })
        .unwrap();
    assert_eq!(runtime.adapter_guard(), guard);
    runtime
        .adapter_did_fetch(&rig.sdk, &format!("absence-{id}"), true)
        .unwrap();
    let call_completed = runtime.adapter_finish_fetch(&rig.sdk, id).unwrap();
    safe(&rig.host, &before);
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed);
    assert!(logged(runtime).is_empty());
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let again = page(&mut rig.service, &rig.zone, None);
    assert_eq!(deliver(&rig.host, &rig.sdk, &rig.zone, &again, true), seen);
    finish(runtime, &rig.sdk, &rig.zone, id, false);
    safe(&rig.host, &before);
    completed(&rig.host, &rig.zone, BTreeSet::new());
    assert!(!call_completed, "pre-event zone is no post-event drain");
}

#[test]
fn saved_cursor_clean_reopen_account_reset_and_partial_fetch_start_no_absence_pass() {
    let rig = fixture(60, true);
    let directory = rig.root.path().join("device");
    rig.host.close().unwrap();
    drop(rig.host);
    let host = open(&directory);
    let (sdk, state, zone) = admit(&host);
    assert_eq!(state, Some(b"prior-sdk-state".to_vec()));
    let mut service = rig.service;
    confirm_root(&host, &sdk, &mut service);
    let before = rows(&host);
    let runtime = host.cloud_runtime().unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: identity(),
        origin_sdk: Some(sdk.clone()),
    })
    .unwrap();
    let id = begin(runtime, &sdk, &zone);
    let p = page(&mut service, &zone, None);
    assert!(p.more); // Also models an undetectable SDK token restart's partial fetch.
    deliver(&host, &sdk, &zone, &p, true);
    finish(runtime, &sdk, &zone, id, false);
    safe(&host, &before);
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed);
    assert!(
        logged(runtime).is_empty(),
        "a restored cursor remains ineligible after account notification"
    );
    host.close().unwrap();
}

#[test]
fn failed_zone_fetch_preserves_marks_until_later_successful_fetch() {
    let mut rig = fixture(60, false);
    let before = rows(&rig.host);
    let runtime = rig.host.cloud_runtime().unwrap();
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let first = page(&mut rig.service, &rig.zone, None);
    let seen = deliver(&rig.host, &rig.sdk, &rig.zone, &first, true);
    finish(runtime, &rig.sdk, &rig.zone, id, true);
    safe(&rig.host, &before);
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed);
    assert!(logged(runtime).is_empty());
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let mut token = first.token;
    let mut all_seen = seen;
    loop {
        let p = page(&mut rig.service, &rig.zone, Some(token));
        all_seen.extend(deliver(&rig.host, &rig.sdk, &rig.zone, &p, true));
        if !p.more {
            break;
        }
        token = p.token;
    }
    finish(runtime, &rig.sdk, &rig.zone, id, false);
    safe(&rig.host, &before);
    assert_eq!(all_seen, rig.records.iter().map(|r| r.name.clone()).collect());
    completed(&rig.host, &rig.zone, BTreeSet::new());
}

// Choose synthetic new pairs that occur in the FIRST HALF of the real Service
// lexical snapshot. This controls fixture data, never delivered pages or tokens.
fn new_pairs(records: &[Saved]) -> Vec<Saved> {
    let (note_entity, note) = records
        .iter()
        .find_map(|r| match decode(r) {
            (e, Payload::Note(n)) => Some((e, n)),
            _ => None,
        })
        .unwrap();
    let (card_entity, card) = records
        .iter()
        .find_map(|r| match decode(r) {
            (e, Payload::Card(c)) if c.nid == note.id => Some((e, c)),
            _ => None,
        })
        .unwrap();
    let cutoff = &records[records.len() / 4].name;
    let mut out = Vec::new();
    for id in 8_000_000..9_000_000 {
        let nn = codec::entity_name(&[114; 32], codec::Kind::Note, &codec::id_key(id)).unwrap();
        let cn = codec::entity_name(&[114; 32], codec::Kind::Card, &codec::id_key(id)).unwrap();
        if &nn >= cutoff || &cn >= cutoff {
            continue;
        }
        let mut n = note.clone();
        n.id = id;
        n.guid = format!("absence-{id}").into_bytes();
        let mut c = card.clone();
        c.id = id;
        c.nid = id;
        for (mut fields, payload, name) in [
            (note_entity.fields.clone(), Payload::Note(n), nn),
            (card_entity.fields.clone(), Payload::Card(c), cn),
        ] {
            fields.key = codec::id_key(id).to_vec();
            let envelope = codec::Entity::build(fields, &payload.encode().unwrap())
                .unwrap()
                .encode()
                .unwrap();
            out.push(Saved {
                name,
                envelope,
                system_fields: Vec::new(),
            });
        }
        if out.len() == 600 {
            return out;
        }
    }
    panic!("could not arrange 300 first-half pairs");
}

#[test]
fn interrupted_pass_kill_reopen_saved_cursor_keeps_first_half_and_300_applied_pairs() {
    let mut rig = fixture(700, false);
    let new = new_pairs(&rig.records);
    put(&mut rig.service, &rig.zone, &new, "MGEntity");
    // Omit two acked names from the whole server snapshot, without supplying a
    // deletion callback. Delete happens before the child's cursorless fetch.
    let missing: BTreeSet<_> = rig.records.iter().rev().take(2).map(|r| r.name.clone()).collect();
    let response = rig
        .service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: rig.zone.clone(),
                atomic: false,
                changes: missing
                    .iter()
                    .map(|name| Mutation::Delete { name: name.clone() })
                    .collect(),
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("actual deletion receipts required")
    };
    assert!(items.iter().all(|i| matches!(i.result, Ok(MutationSuccess::Deleted))));
    let root = rig.root.path().to_owned();
    let directory = root.join("device");
    std::fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&(rig.records.clone(), new.clone(), missing.clone())).unwrap(),
    )
    .unwrap();
    rig.host.close().unwrap();
    drop(rig.host);
    drop(rig.service);

    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cloud_absence_child_checkpoint",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if let Some((_, data)) = line.split_once(CHECKPOINT) {
                let _ = tx.send(data.to_owned());
                return;
            }
        }
    });
    let checkpoint = rx.recv_timeout(BUDGET);
    // Kill/reap even on a failed checkpoint; a clean close cannot stand for it.
    let killed = child.kill();
    let status = child.wait().unwrap();
    reader.join().unwrap();
    let (serialization, first_half, pending_marks): (Vec<u8>, BTreeSet<String>, Vec<Option<[u8; 16]>>) =
        serde_json::from_str(&checkpoint.expect("child must reach real FetchAfterCursor seam")).unwrap();
    killed.unwrap();
    assert!(!status.success());
    let new_names: BTreeSet<_> = new.iter().map(|r| r.name.clone()).collect();
    assert_eq!(new_names.len(), 600);
    assert!(new_names.is_subset(&first_half));
    assert!(!pending_marks.is_empty());

    let host = open(&directory);
    let (sdk, stored, zone) = admit(&host);
    assert_eq!(
        stored,
        Some(serialization.clone()),
        "new instance must resume the actual saved token"
    );
    let mut service = service(&root);
    confirm_root(&host, &sdk, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap().id;
    let reopened_marks: Vec<_> = runtime
        .inbox_entries(binding)
        .unwrap()
        .iter()
        .map(|e| e.entry.absence_pass)
        .collect();
    assert_eq!(
        reopened_marks, pending_marks,
        "inbox marks must survive actual process death"
    );
    // Apply the final held page, then re-deliver a real old page. These marks
    // must remain the ORIGINAL pass; reopening with a cursor cannot start one.
    runtime.drain_incoming(&host.collection().unwrap(), BUDGET).unwrap();
    let before = rows(&host);
    // A manual fetch with no zone callbacks supplies fetched:false. K17.4
    // requires the restored pass to remain pending across that call.
    let idle_fetch = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
    let group = format!("idle-fetch-{idle_fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&sdk, idle_fetch).unwrap());
    let fetch_log = runtime.adapter_fetch_drains();
    let last_fetch = fetch_log.last().unwrap();
    assert_eq!(last_fetch.zone, zone);
    assert!(!last_fetch.fetched);
    safe(&host, &before);
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed);
    assert!(logged(runtime).is_empty());
    let id = begin(runtime, &sdk, &zone);
    let replay = page(&mut service, &zone, None);
    let replayed = deliver(&host, &sdk, &zone, &replay, true);
    assert!(replayed.is_subset(&first_half));
    let (stored_zone, mut token): (String, Token) = serde_json::from_slice(&serialization).unwrap();
    assert_eq!(stored_zone, zone);
    let mut resumed = BTreeSet::new();
    loop {
        let p = page(&mut service, &zone, Some(token));
        resumed.extend(deliver(&host, &sdk, &zone, &p, true));
        if !p.more {
            break;
        }
        token = p.token;
    }
    assert!(
        resumed.is_disjoint(&first_half),
        "saved snapshot token must not re-deliver its first half"
    );
    assert!(
        resumed.is_disjoint(&new_names),
        "all 300 new pairs applied before death must be skipped by resume"
    );
    for r in &new {
        let facts = runtime.entity_facts(&r.name).unwrap().unwrap();
        assert!(facts.acked_rev > 0);
        assert_eq!(facts.acked_payload, Some(decode(r).1));
    }
    finish(runtime, &sdk, &zone, id, false);
    safe(&host, &before);
    let pass = completed(&host, &zone, missing);
    assert!(
        pending_marks.iter().all(|mark| *mark == Some(pass)),
        "all first-half inbox marks must name the completed durable pass"
    );
    host.close().unwrap();
}

/// Subprocess infrastructure only, like testkit's cloud_child_entry. It never
/// supplies a pass result. The parent owns every absence/completion assertion.
#[test]
fn cloud_absence_child_checkpoint() {
    let Some(root) = std::env::var_os(CHILD_ENV).map(std::path::PathBuf::from) else {
        return;
    };
    let (records, new, missing): (Vec<Saved>, Vec<Saved>, BTreeSet<String>) =
        serde_json::from_slice(&std::fs::read(root.join("fixture.json")).unwrap()).unwrap();
    let host = open(&root.join("device"));
    let (sdk, state, zone) = admit(&host);
    assert!(state.is_none());
    let mut service = service(&root);
    confirm_root(&host, &sdk, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    begin(runtime, &sdk, &zone);
    let total = records.len() + new.len() - missing.len();
    let pages = (total / 2).div_ceil(50);
    let mut token = None;
    let mut seen = BTreeSet::new();
    for _ in 0..pages {
        let p = page(&mut service, &zone, token);
        assert!(p.more);
        seen.extend(deliver(&host, &sdk, &zone, &p, true));
        token = Some(p.token);
    }
    // K13.3: references can span pages.
    let collection = host.collection().unwrap();
    for _ in 0..16 {
        if runtime.drain_incoming(&collection, BUDGET).unwrap() == 0 {
            break;
        }
    }
    let binding = runtime.status().unwrap().binding.unwrap().id;
    let inbox = runtime.inbox_entries(binding).unwrap();
    assert!(
        inbox.is_empty(),
        "first-half intake still pending: {:?}",
        inbox
            .iter()
            .map(|entry| (&entry.entry.record_name, &entry.entry.reason))
            .collect::<Vec<_>>()
    );
    // Verify all 300 received pairs before holding another page. Each apply
    // must have a matching durability receipt from the collection writer.
    let stored_rows = rows(&host);
    assert_eq!(stored_rows[&Table::Notes].len(), 700 + 300);
    assert_eq!(stored_rows[&Table::Cards].len(), 700 + 300);
    for r in &new {
        let facts = runtime.entity_facts(&r.name).unwrap().unwrap();
        assert!(facts.acked_rev > 0);
        assert_eq!(facts.acked_payload, Some(decode(r).1));
    }
    let runs = runtime.incoming_runs();
    let receipts = runtime.incoming_wal_receipts();
    assert!(!runs.is_empty());
    for run in runs {
        assert_eq!(run.generation, collection.generation().0);
        assert!(receipts.iter().any(|receipt| receipt.identity().native_file()
            == (run.native_file.device, run.native_file.inode)
            && receipt.through().0 >= run.seq));
    }
    let intents = runtime.apply_intents(binding).unwrap();
    assert!(
        intents.is_empty(),
        "first-half apply intents remaining: {}",
        intents.len()
    );
    // A real newer server revision creates an inbox entry in this same pass.
    // It carries a mark across kill/reopen even though it writes no new content.
    let original = read(&mut service, &zone, &new[0].name);
    let old = saved(&original);
    let (mut entity, payload) = decode(&old);
    entity.fields.erev += 1;
    let envelope = codec::Entity::build(entity.fields, &payload.encode().unwrap())
        .unwrap()
        .encode()
        .unwrap();
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: zone.clone(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&old.name, "MGEntity", envelope),
                    if_unchanged: Some(original.system_fields),
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("real newer receipt")
    };
    let Ok(MutationSuccess::Saved(record)) = &items[0].result else {
        panic!("saved revision")
    };
    runtime.adapter_fetched(&sdk, &zone, &[saved(record)], &[]).unwrap();
    let snapshot = runtime.adapter_snapshot().unwrap();
    let marks: Vec<_> = snapshot.inbox.iter().map(|e| e.absence_pass).collect();
    let checkpoint = serde_json::to_string(&(snapshot.serialization.unwrap(), seen, marks)).unwrap();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == IncomingStage::FetchAfterCursor {
            println!("{CHECKPOINT}{checkpoint}");
            std::io::stdout().flush().unwrap();
            let _ = std::io::stdin().read(&mut [0]);
            panic!("parent must kill this process at the checkpoint");
        }
    });
    // Same real post-state-save seam used by the existing child incoming driver.
    runtime.incoming_stage_for_testing(IncomingStage::FetchAfterCursor);
    drop(collection);
}

// Additional bounded prerequisites are intentionally written before the rule wiring.
#[test]
fn stale_refused_and_deleted_intake_are_seen_without_applying_them() {
    let rig = fixture(10, false);
    let runtime = rig.host.cloud_runtime().unwrap();
    let before = rows(&rig.host);
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let stale = rig.records[0].clone();
    let mut refused = rig.records[1].clone();
    refused.envelope = b"invalid synthetic envelope".to_vec();
    let deleted = rig.records[2].name.clone();
    runtime
        .adapter_fetched(
            &rig.sdk,
            &rig.zone,
            &[stale.clone(), refused.clone()],
            std::slice::from_ref(&deleted),
        )
        .unwrap();
    let binding = runtime.status().unwrap().binding.unwrap().id;
    assert!(
        runtime
            .refused_entries(binding)
            .unwrap()
            .iter()
            .any(|r| r.record_name == refused.name)
    );
    assert!(
        runtime
            .inbox_entries(binding)
            .unwrap()
            .iter()
            .any(|e| e.entry.record_name == deleted && matches!(e.entry.value, InboxValue::Deletion))
    );
    finish(runtime, &rig.sdk, &rig.zone, id, false);
    assert_eq!(rows(&rig.host), before);
    completed(
        &rig.host,
        &rig.zone,
        rig.records
            .iter()
            .map(|r| r.name.clone())
            .filter(|n| n != &stale.name && n != &refused.name && n != &deleted)
            .collect(),
    );
}

#[test]
fn failed_seen_intake_rolls_back_and_forbids_the_following_cursor() {
    let rig = fixture(10, false);
    let runtime = rig.host.cloud_runtime().unwrap();
    let _id = begin(runtime, &rig.sdk, &rig.zone);
    let before = runtime.adapter_snapshot().unwrap();
    runtime.arm_adapter_faults_for_testing(true, false);
    assert!(
        runtime
            .adapter_fetched(&rig.sdk, &rig.zone, &rig.records[..1], &[])
            .is_err()
    );
    assert!(runtime.adapter_state_update(&rig.sdk, b"must-not-advance").is_err());
    let after = runtime.adapter_snapshot().unwrap();
    assert_eq!(after.tracking_content, before.tracking_content);
    assert_eq!(after.serialization, before.serialization);
    assert!(logged(runtime).is_empty());
}

#[test]
fn pre_pass_inbox_apply_marks_no_new_entity_and_redelivery_does() {
    let mut rig = fixture(10, false);
    let pairs = new_pairs(&rig.records)[..4].to_vec();
    put(&mut rig.service, &rig.zone, &pairs, "MGEntity");
    let pairs: Vec<_> = pairs
        .iter()
        .map(|r| saved(&read(&mut rig.service, &rig.zone, &r.name)))
        .collect();
    let runtime = rig.host.cloud_runtime().unwrap();
    // A real entry predating the first collection-zone will event has no pass.
    runtime.adapter_fetched(&rig.sdk, &rig.zone, &pairs, &[]).unwrap();
    let binding = runtime.status().unwrap().binding.unwrap().id;
    assert!(
        runtime
            .inbox_entries(binding)
            .unwrap()
            .iter()
            .all(|e| e.entry.absence_pass.is_none())
    );
    let id = begin(runtime, &rig.sdk, &rig.zone);
    for _ in 0..32 {
        if runtime.drain_incoming(&rig.host.collection().unwrap(), BUDGET).unwrap() == 0 {
            break;
        }
    }
    assert!(runtime.inbox_entries(binding).unwrap().is_empty());
    let redelivered = &pairs[..2];
    runtime.adapter_fetched(&rig.sdk, &rig.zone, redelivered, &[]).unwrap();
    for r in &pairs {
        assert!(runtime.entity_facts(&r.name).unwrap().unwrap().server_erev > 0);
    }
    finish(runtime, &rig.sdk, &rig.zone, id, false);
    let delivered: BTreeSet<_> = redelivered.iter().map(|r| r.name.clone()).collect();
    completed(
        &rig.host,
        &rig.zone,
        rig.records
            .iter()
            .chain(pairs.iter())
            .map(|r| r.name.clone())
            .filter(|n| !delivered.contains(n))
            .collect(),
    );
}

#[test]
fn saved_response_and_replayed_write_mark_running_fetch() {
    for receipt_missing in [false, true] {
        let mut rig = fixture(10, false);
        let runtime = rig.host.cloud_runtime().unwrap();
        let collection = rig.host.collection().unwrap();
        let (name, note) = rig
            .records
            .iter()
            .find_map(|r| match decode(r).1 {
                Payload::Note(n) => Some((r.name.clone(), n)),
                _ => None,
            })
            .unwrap();
        // K13.1.
        let server_base = saved(&read(&mut rig.service, &rig.zone, &name));
        let (mut revision, payload) = decode(&server_base);
        revision.fields.erev += 1;
        let refreshed = codec::Entity::build(revision.fields, &payload.encode().unwrap())
            .unwrap()
            .encode()
            .unwrap();
        let reply = rig
            .service
            .request(
                &scope(),
                Caller::default(),
                Operation::Modify {
                    zone: rig.zone.clone(),
                    atomic: false,
                    changes: vec![Mutation::Save {
                        record: RecordInput::encrypted(&name, "MGEntity", refreshed),
                        if_unchanged: Some(meguri_testkit::cloud::SystemFields::from_bytes(
                            server_base.system_fields,
                        )),
                    }],
                },
            )
            .unwrap();
        let Reply::Modified(receipts) = reply.result.unwrap() else {
            panic!("server revision response required")
        };
        let MutationSuccess::Saved(server_record) = receipts.into_iter().next().unwrap().result.unwrap() else {
            panic!("server revision save required")
        };
        let authority = saved(&server_record);
        runtime
            .adapter_fetched(&rig.sdk, &rig.zone, std::slice::from_ref(&authority), &[])
            .unwrap();
        runtime.drain_incoming(&collection, BUDGET).unwrap();
        let settled = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(settled.acked_system_fields, authority.system_fields);
        assert_eq!(settled.server_erev, 2);
        assert!(settled.mark.is_none());
        assert!(!runtime.incoming_wal_receipts().is_empty());
        assert!(
            runtime
                .inbox_entries(runtime.status().unwrap().binding.unwrap().id)
                .unwrap()
                .is_empty()
        );
        meguri_engine::notes::update(
            &collection,
            meguri_engine::notes::UpdateNote {
                id: meguri_model::ids::NoteId(note.id),
                fields: vec!["edited front".into(), "edited back".into()],
                tags: vec![],
            },
            OpSource::UserUndoable,
        )
        .unwrap();
        runtime
            .reconcile_binding(&collection, &rig.host.media().unwrap(), BUDGET)
            .unwrap();
        let batch = runtime
            .build_collection_batch(&rig.sdk, &collection, &BTreeSet::from([name.clone()]))
            .unwrap();
        assert_eq!(batch.saves.len(), 1);
        let record = &batch.saves[0];
        assert_eq!(record.system_fields.as_ref(), Some(&authority.system_fields));
        let response = rig
            .service
            .request(
                &scope(),
                Caller::default(),
                Operation::Modify {
                    zone: rig.zone.clone(),
                    atomic: false,
                    changes: vec![Mutation::Save {
                        record: RecordInput::encrypted(&name, "MGEntity", record.envelope.clone()),
                        if_unchanged: record
                            .system_fields
                            .clone()
                            .map(meguri_testkit::cloud::SystemFields::from_bytes),
                    }],
                },
            )
            .unwrap();
        let Reply::Modified(items) = response.result.unwrap() else {
            panic!("conditional edit response")
        };
        let MutationSuccess::Saved(record) = items.into_iter().next().unwrap().result.unwrap() else {
            panic!("server save receipt required")
        };
        let landed = saved(&record);
        let id = begin(runtime, &rig.sdk, &rig.zone);
        if receipt_missing {
            runtime.adapter_fetched(&rig.sdk, &rig.zone, &[landed], &[]).unwrap();
            runtime.drain_incoming(&collection, BUDGET).unwrap();
            assert!(!runtime.incoming_wal_receipts().is_empty());
        } else {
            runtime.enter_sent_handler(&rig.sdk, batch.id).unwrap();
            runtime
                .sent_results(&rig.sdk, &collection, batch.id, &[landed], &[], 1)
                .unwrap();
            runtime.return_sent_handler(&rig.sdk).unwrap();
        }
        let acknowledged = runtime.entity_facts(&name).unwrap().unwrap();
        assert!(acknowledged.server_erev > 1 && acknowledged.mark.is_none());
        finish(runtime, &rig.sdk, &rig.zone, id, false);
        completed(
            &rig.host,
            &rig.zone,
            rig.records
                .iter()
                .map(|r| r.name.clone())
                .filter(|n| n != &name)
                .collect(),
        );
    }
}

#[test]
fn cursor_removal_cancels_tracking_and_next_open_allocates_another_pass() {
    let rig = fixture(10, false);
    let runtime = rig.host.cloud_runtime().unwrap();
    let original_rows = rows(&rig.host);
    let id = begin(runtime, &rig.sdk, &rig.zone);
    runtime.adapter_state_update(&rig.sdk, b"cursor-one").unwrap();
    runtime.forget_adapter_state_for_testing(&rig.sdk).unwrap();
    // K17.4.
    runtime.adapter_state_update(&rig.sdk, b"cursor-two").unwrap();
    finish(runtime, &rig.sdk, &rig.zone, id, false);
    assert!(logged(runtime).is_empty());
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed);
    runtime.forget_adapter_state_for_testing(&rig.sdk).unwrap();
    rig.host.close().unwrap();
    drop(rig.host);
    let host = open(&rig.root.path().join("device"));
    let (sdk, state, zone) = admit(&host);
    assert!(state.is_none());
    let runtime = host.cloud_runtime().unwrap();
    let id = begin(runtime, &sdk, &zone);
    finish(runtime, &sdk, &zone, id, false);
    safe(&host, &original_rows);
    let previous_pass = completed(&host, &zone, rig.records.iter().map(|r| r.name.clone()).collect());
    host.close().unwrap();
    drop(host);
    // Reopen without a stored cursor and verify a distinct ledger identity.
    let host = open(&rig.root.path().join("device"));
    let (sdk, state, zone) = admit(&host);
    assert!(state.is_none());
    let runtime = host.cloud_runtime().unwrap();
    assert!(logged(runtime).is_empty());
    let id = begin(runtime, &sdk, &zone);
    finish(runtime, &sdk, &zone, id, false);
    assert_ne!(
        completed(&host, &zone, rig.records.iter().map(|r| r.name.clone()).collect()),
        previous_pass
    );
}

#[test]
fn account_isolation_ends_the_running_pass_without_a_completion() {
    let rig = fixture(10, false);
    let runtime = rig.host.cloud_runtime().unwrap();
    let before = rows(&rig.host);
    let id = begin(runtime, &rig.sdk, &rig.zone);
    runtime.adapter_state_update(&rig.sdk, b"before-signout").unwrap();
    rig.host
        .cloud_policy_input(DeviceEvent::Account {
            identity: IdentityStatus::SignedOut,
            origin_sdk: Some(rig.sdk.clone()),
        })
        .unwrap();
    assert!(
        runtime
            .adapter_fetch_zone(&rig.sdk, rig.zone.clone(), false, false)
            .is_err()
    );
    assert!(runtime.adapter_finish_fetch(&rig.sdk, id).is_err());
    assert!(logged(runtime).is_empty());
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed);
    assert_eq!(rows(&rig.host), before);
}

#[test]
fn real_surfaced_token_expiry_then_partial_drain_starts_no_pass() {
    use meguri_testkit::cloud::{ErrorCode, RemovalCause};
    let mut rig = fixture(60, true);
    let token = page(&mut rig.service, &rig.zone, None).token;
    for operation in [
        Operation::DeleteZone {
            zone: rig.zone.clone(),
            cause: RemovalCause::Deleted,
        },
        Operation::CreateZone { zone: rig.zone.clone() },
    ] {
        assert!(matches!(
            rig.service
                .request(&scope(), Caller::default(), operation)
                .unwrap()
                .result,
            Ok(Reply::Done)
        ));
    }
    put(&mut rig.service, &rig.zone, &rig.records, "MGEntity");
    let runtime = rig.host.cloud_runtime().unwrap();
    let before = rows(&rig.host);
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let response = rig
        .service
        .request(
            &scope(),
            Caller::default(),
            Operation::FetchZone {
                zone: rig.zone.clone(),
                token: Some(token),
            },
        )
        .unwrap();
    let error = response.result.err().unwrap();
    assert_eq!(error.code, ErrorCode::ChangeTokenExpired);
    // The fake SDK surfaces this actual service failure as a failed zone end.
    finish(runtime, &rig.sdk, &rig.zone, id, true);
    let id = begin(runtime, &rig.sdk, &rig.zone);
    let p = page(&mut rig.service, &rig.zone, None);
    assert!(p.more);
    deliver(&rig.host, &rig.sdk, &rig.zone, &p, true);
    finish(runtime, &rig.sdk, &rig.zone, id, false);
    safe(&rig.host, &before);
    assert!(logged(runtime).is_empty());
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed);
}

#[path = "support/sign_out_absence.rs"]
mod sign_out_absence;

const Q6_CHILD_ENV: &str = "MEGURI_Q6_RESUME_ABSENCE_CHILD";
const Q6_CHECKPOINT: &str = "Q6_RESUME_ABSENCE_CHECKPOINT ";

#[derive(serde::Serialize, serde::Deserialize)]
struct Q6LocalFacts {
    note_id: i64,
    note_name: String,
    edited_fields: Vec<u8>,
    review_id: i64,
    reviewed_card: i64,
    baseline_notes: usize,
    baseline_cards: usize,
    baseline_reviews: usize,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Q6Checkpoint {
    serialization: Vec<u8>,
    seen: BTreeSet<String>,
    pass: [u8; 16],
    entities: Vec<cloud::workflow::EntityFacts>,
    inbox_marks: Vec<(String, Option<[u8; 16]>)>,
    // Real, checked receipt coverage, represented only as semantic scalars.
    wal: Vec<(u64, u64)>,
}

// Compare native collection contents; compatibility USNs are device-local.
fn q6_content(host: &CoreHost) -> BTreeMap<Table, Vec<Vec<Cell>>> {
    let actual = rows(host);
    [(Table::Notes, 4), (Table::Cards, 5), (Table::Revlog, 2)]
        .into_iter()
        .map(|(table, usn)| {
            let content = actual[&table]
                .iter()
                .map(|row| {
                    row.iter()
                        .enumerate()
                        .filter(|(index, _)| *index != usn)
                        .map(|(_, cell)| cell.clone())
                        .collect()
                })
                .collect();
            (table, content)
        })
        .collect()
}
fn q6_local_rows(host: &CoreHost, facts: &Q6LocalFacts) {
    let actual = rows(host);
    assert_eq!(actual[&Table::Revlog].len(), facts.baseline_reviews + 1);
    let note = actual[&Table::Notes]
        .iter()
        .find(|row| row[0] == Cell::Integer(facts.note_id))
        .unwrap();
    assert_eq!(note[6], Cell::Text(facts.edited_fields.clone()));
    let review = actual[&Table::Revlog]
        .iter()
        .find(|row| row[0] == Cell::Integer(facts.review_id))
        .unwrap();
    assert_eq!(review[1], Cell::Integer(facts.reviewed_card));
    assert!(
        actual[&Table::Cards]
            .iter()
            .any(|row| row[0] == Cell::Integer(facts.reviewed_card))
    );
}
fn q6_no_deletions(host: &CoreHost) {
    let runtime = host.cloud_runtime().unwrap();
    assert!(
        runtime
            .adapter_snapshot()
            .unwrap()
            .inbox
            .iter()
            .all(|e| !matches!(e.value, InboxValue::Deletion))
    );
    assert!(rows(host)[&Table::Graves].is_empty());
}
// Observe intake before apply can consume entries; a transient deletion must
// fail here even if a later apply could hide it from the final inbox snapshot.
fn q6_deliver(host: &CoreHost, sdk: &str, zone: &str, p: &Page<RecordChange>) -> BTreeSet<String> {
    let runtime = host.cloud_runtime().unwrap();
    let before = sign_out_absence::names(runtime);
    let seen = deliver(host, sdk, zone, p, false);
    q6_no_deletions(host);
    for _ in 0..16 {
        if runtime.drain_incoming(&host.collection().unwrap(), BUDGET).unwrap() == 0 {
            break;
        }
    }
    assert!(
        before.is_subset(&sign_out_absence::names(runtime)),
        "fetch must remove no tracked entity"
    );
    q6_no_deletions(host);
    if !p.more {
        assert!(
            runtime
                .inbox_entries(runtime.status().unwrap().binding.unwrap().id)
                .unwrap()
                .is_empty()
        );
    }
    seen
}
fn q6_assert_pairs(host: &CoreHost, accepted: &[Saved], facts: &Q6LocalFacts) {
    let actual = rows(host);
    assert_eq!(actual[&Table::Notes].len(), facts.baseline_notes + 300);
    assert_eq!(actual[&Table::Cards].len(), facts.baseline_cards + 300);
    let runtime = host.cloud_runtime().unwrap();
    for record in accepted {
        let (_, payload) = decode(record);
        let tracked = runtime.entity_facts(&record.name).unwrap().unwrap();
        assert!(tracked.acked_rev > 0);
        assert!(tracked.acked_payload.as_ref() == Some(&payload));
        assert_eq!(tracked.acked_system_fields, record.system_fields);
        match payload {
            Payload::Note(note) => {
                let row = actual[&Table::Notes]
                    .iter()
                    .find(|row| row[0] == Cell::Integer(note.id))
                    .unwrap();
                assert_eq!(row[6], Cell::Text(note.flds));
            }
            Payload::Card(card) => {
                let row = actual[&Table::Cards]
                    .iter()
                    .find(|row| row[0] == Cell::Integer(card.id))
                    .unwrap();
                assert_eq!(row[1], Cell::Integer(card.nid));
            }
            _ => panic!("only the actual 300 new pairs belong to this witness"),
        }
    }
    q6_no_deletions(host);
}

/// Owns the process and its sole pipe reader before any checkpoint assertion.
/// Drop also kills/reaps/joins on timeout, parse failure, or parent assertion.
struct Q6Child {
    child: Option<std::process::Child>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Q6Child {
    fn spawn(root: &Path) -> (Self, mpsc::Receiver<String>) {
        Self::spawn_checkpoint(
            root,
            "cloud_q6_resume_absence_child_checkpoint",
            Q6_CHILD_ENV,
            Q6_CHECKPOINT,
        )
    }
    fn spawn_checkpoint(
        root: &Path,
        entry: &str,
        environment: &str,
        prefix: &'static str,
    ) -> (Self, mpsc::Receiver<String>) {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", entry, "--nocapture", "--test-threads=1"])
            .env(environment, root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut owner = Self {
            child: Some(child),
            reader: None,
        };
        let stdout = owner.child.as_mut().unwrap().stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        owner.reader = Some(std::thread::spawn(move || {
            // Continue draining until EOF, including after a checkpoint. There
            // is no detached writer or pipe thread at parent teardown.
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Some((_, json)) = line.split_once(prefix) {
                    let _ = tx.send(json.to_owned());
                }
            }
        }));
        (owner, rx)
    }
    fn stop(&mut self) -> Option<(std::io::Result<()>, std::io::Result<std::process::ExitStatus>)> {
        let result = self.child.take().map(|mut child| {
            let killed = child.kill();
            let reaped = child.wait();
            (killed, reaped)
        });
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        result
    }
}
impl Drop for Q6Child {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// V9.2/V13/V26.2, K17.4, Q6 sign_out_resume + no_state_resume_absence.
/// Synthetic Service with the actual Rust Host and native SDK callback seam;
/// this makes no claim about live Apple SDK behavior (A7/A9a/A12).
#[test]
fn sign_out_resume_real_adoption_300_pairs_survive_first_page_apply_process_kill() {
    use cloud::entity_codec::Kind;
    use meguri_engine::session::{AnswerOutcome, StartResult};
    use meguri_model::{
        ids::{NoteId, NotetypeId},
        time::EpochMillis,
    };
    use meguri_sched::answer::Rating;
    use sign_out_absence as actual;

    let root = tempfile::tempdir().unwrap();
    let a_dir = root.path().join("q6-a");
    let b_dir = root.path().join("q6-b");
    actual::generated(&a_dir);
    let account = Arc::new(actual::MutableAccount(std::sync::Mutex::new(identity())));
    let a = actual::open_actual(&a_dir, account.clone());
    let mut service = service(root.path());
    let binding = actual::publish(&a, &mut service);
    let b = actual::adopt(&b_dir, &mut service);
    assert_eq!(
        b.cloud_runtime()
            .unwrap()
            .status()
            .unwrap()
            .binding
            .unwrap()
            .facts
            .epoch,
        binding.facts.epoch
    );
    assert_eq!(q6_content(&a), q6_content(&b));

    let (old_sdk, _, zone) = admit(&a);
    confirm_root(&a, &old_sdk, &mut service);
    // Save an actual Service cursor before sign-out; Resume must delete it.
    let fetch = begin(a.cloud_runtime().unwrap(), &old_sdk, &zone);
    let baseline = page(&mut service, &zone, None);
    assert!(!baseline.more);
    q6_deliver(&a, &old_sdk, &zone, &baseline);
    finish(a.cloud_runtime().unwrap(), &old_sdk, &zone, fetch, false);
    let old_cursor = a
        .cloud_runtime()
        .unwrap()
        .adapter_snapshot()
        .unwrap()
        .serialization
        .unwrap();
    assert!(!old_cursor.is_empty());
    let original = a
        .cloud_runtime()
        .unwrap()
        .entity_facts_page(Kind::Note, None)
        .unwrap()
        .remove(0);
    let Payload::Note(note) = original.payload.clone() else {
        panic!("actual published Note")
    };
    let before_rows = rows(&a);
    let requests = service.request_log().unwrap().len();
    *account.0.lock().unwrap() = IdentityStatus::SignedOut;
    a.cloud_runtime().unwrap().refresh_identity().unwrap();
    assert_eq!(
        a.cloud_runtime()
            .unwrap()
            .status()
            .unwrap()
            .binding
            .unwrap()
            .facts
            .state,
        cloud::BindingState::Isolated(cloud::IsolationReason::SignedOut)
    );
    assert!(a.cloud_runtime().unwrap().validate_sdk(&old_sdk).is_err());

    let mut fields: Vec<String> = std::str::from_utf8(&note.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    fields[1] = "edited while signed out; public Q6 witness".into();
    let edited_fields = fields.join("\u{1f}").into_bytes();
    assert!(
        meguri_engine::notes::update(
            &a.collection().unwrap(),
            meguri_engine::notes::UpdateNote {
                id: NoteId(note.id),
                fields,
                tags: Vec::new(),
            },
            OpSource::UserUndoable
        )
        .unwrap()
        .value
        .written
    );
    let card = a
        .cloud_runtime()
        .unwrap()
        .entity_facts_page(Kind::Card, None)
        .unwrap()
        .remove(0);
    let Payload::Card(card) = card.payload else {
        panic!("published Card")
    };
    let StartResult::Started { session_id, head, .. } = a.review().start(DeckId(card.did), None).unwrap() else {
        panic!("local review must work while signed out")
    };
    let item = &head.items[0];
    let reviewed_card = item.card_id.0;
    assert!(matches!(
        a.review()
            .answer(
                session_id,
                item.card_id,
                Rating::Easy,
                item.state_token,
                EpochMillis(NOW),
                123
            )
            .unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    a.review().end(session_id).unwrap();
    let after_rows = rows(&a);
    let reviews: Vec<_> = after_rows[&Table::Revlog]
        .iter()
        .filter(|row| !before_rows[&Table::Revlog].iter().any(|old| old[0] == row[0]))
        .collect();
    assert_eq!(reviews.len(), 1);
    let Cell::Integer(review_id) = reviews[0][0] else {
        panic!("review ID")
    };
    let facts = Q6LocalFacts {
        note_id: note.id,
        note_name: original.name.clone(),
        edited_fields,
        review_id,
        reviewed_card,
        baseline_notes: before_rows[&Table::Notes].len(),
        baseline_cards: before_rows[&Table::Cards].len(),
        baseline_reviews: before_rows[&Table::Revlog].len(),
    };
    q6_local_rows(&a, &facts);
    assert!(
        a.cloud_runtime()
            .unwrap()
            .entity_facts(&original.name)
            .unwrap()
            .unwrap()
            == original,
        "isolated changes must remain local until reconciliation"
    );
    assert_eq!(service.request_log().unwrap().len(), requests);
    assert!(a.cloud_runtime().unwrap().resume_binding_offer().is_none());
    a.close().unwrap();
    drop(a);

    let (b_sdk, _, b_zone) = admit(&b);
    assert_eq!(b_zone, zone);
    confirm_root(&b, &b_sdk, &mut service);
    let mut added_ids = BTreeSet::new();
    for n in 0..300 {
        let added = meguri_engine::notes::add(
            &b.collection().unwrap(),
            meguri_engine::notes::AddNote {
                notetype_id: NotetypeId(note.mid),
                fields: vec![format!("Q6 remote question {n}"), format!("Q6 remote answer {n}")],
                tags: Vec::new(),
                deck_id: DeckId(card.did),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap()
        .value;
        assert_eq!(added.cards.len(), 1);
        assert!(added_ids.insert((Kind::Note, added.id.0)));
        assert!(added_ids.insert((Kind::Card, added.cards[0].0)));
    }
    b.cloud_runtime()
        .unwrap()
        .reconcile_binding(&b.collection().unwrap(), &b.media().unwrap(), BUDGET)
        .unwrap();
    let accepted: Vec<Saved> = actual::send_pending(&b, &mut service, &b_sdk)
        .into_iter()
        .filter(|record| {
            let (_, payload) = decode(record);
            match payload {
                Payload::Note(n) => added_ids.contains(&(Kind::Note, n.id)),
                Payload::Card(c) => added_ids.contains(&(Kind::Card, c.id)),
                _ => false,
            }
        })
        .collect();
    assert_eq!(accepted.len(), 600);
    assert_eq!(
        accepted.iter().map(|r| r.name.clone()).collect::<BTreeSet<_>>().len(),
        600
    );
    q6_assert_pairs(&b, &accepted, &facts);
    std::fs::write(
        root.path().join("q6-local-facts.json"),
        serde_json::to_vec(&facts).unwrap(),
    )
    .unwrap();
    // Serialize Service ownership: no parent Service SQLite access during child.
    drop(service);
    let (mut child, checkpoint_rx) = Q6Child::spawn(root.path());
    let checkpoint = checkpoint_rx.recv_timeout(BUDGET);
    let (killed, reaped) = child.stop().unwrap();
    // All pipes are joined and the process is reaped before parsing/asserting.
    killed.unwrap();
    let status = reaped.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9), "checkpoint must end with actual SIGKILL");
    }
    assert!(!status.success());
    let checkpoint: Q6Checkpoint =
        serde_json::from_str(&checkpoint.expect("first real WAL-covered page checkpoint within unchanged BUDGET"))
            .unwrap();
    assert_eq!(checkpoint.seen.len(), 50);
    assert!(!checkpoint.wal.is_empty());
    assert!(
        !checkpoint.inbox_marks.is_empty(),
        "real first-page Card references must witness durable inbox pass marks"
    );
    assert!(
        checkpoint
            .inbox_marks
            .iter()
            .all(|(_, mark)| *mark == Some(checkpoint.pass))
    );
    assert!(
        checkpoint
            .seen
            .iter()
            .any(|name| accepted.iter().any(|r| &r.name == name))
    );

    let a = actual::open_actual(&a_dir, Arc::new(Account));
    let runtime = a.cloud_runtime().unwrap();
    q6_local_rows(&a, &facts);
    assert_eq!(
        runtime.adapter_snapshot().unwrap().serialization,
        Some(checkpoint.serialization.clone())
    );
    assert_eq!(actual::running_pass(runtime), checkpoint.pass);
    assert!(
        logged(runtime).is_empty(),
        "partial fetch must not report a new completion on open"
    );
    for before in &checkpoint.entities {
        let reopened = runtime.entity_facts(&before.name).unwrap().unwrap();
        assert_eq!(reopened.local_rev, before.local_rev);
        assert_eq!(reopened.acked_rev, before.acked_rev);
        assert_eq!(reopened.acked_envelope, before.acked_envelope);
        assert_eq!(reopened.acked_system_fields, before.acked_system_fields);
        assert!(reopened.acked_payload == before.acked_payload);
    }
    let reopened_marks: Vec<_> = runtime
        .inbox_entries(binding.id)
        .unwrap()
        .into_iter()
        .map(|e| (e.entry.record_name, e.entry.absence_pass))
        .collect();
    assert_eq!(reopened_marks, checkpoint.inbox_marks);
    let (sdk, state, resumed_zone) = admit(&a);
    assert_eq!(resumed_zone, zone);
    assert_eq!(state, Some(checkpoint.serialization.clone()));
    let mut service = self::service(root.path());
    // This SDK retains the interrupted cursor; the fresh cursorless helper is
    // exercised before the child's first fetch instead.
    runtime.validate_sdk(&sdk).unwrap();
    assert!(!runtime.sdk_root_confirmed());
    let fenced = runtime
        .build_collection_batch(&sdk, &a.collection().unwrap(), &actual::names(runtime))
        .unwrap();
    assert_eq!(fenced.id, 0);
    assert!(fenced.saves.is_empty());
    confirm_root(&a, &sdk, &mut service);
    assert_eq!(actual::running_pass(runtime), checkpoint.pass);
    // A fetched:false call must leave the interrupted pass running.
    let idle = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
    let group = format!("q6-idle-{idle}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&sdk, idle).unwrap());
    assert_eq!(actual::running_pass(runtime), checkpoint.pass);
    assert!(logged(runtime).is_empty());
    let (stored_zone, token): (String, Token) = serde_json::from_slice(&checkpoint.serialization).unwrap();
    assert_eq!(stored_zone, zone);
    let fetch = begin(runtime, &sdk, &zone);
    assert_eq!(
        actual::running_pass(runtime),
        checkpoint.pass,
        "a restored cursor must continue the original pass"
    );
    let mut token = Some(token);
    let mut remaining = BTreeSet::new();
    loop {
        let p = page(&mut service, &zone, token);
        remaining.extend(q6_deliver(&a, &sdk, &zone, &p));
        q6_no_deletions(&a);
        if !p.more {
            break;
        }
        token = Some(p.token);
    }
    assert!(
        remaining.is_disjoint(&checkpoint.seen),
        "the saved actual page token must skip its covered prefix"
    );
    assert!(
        accepted
            .iter()
            .all(|record| checkpoint.seen.contains(&record.name) || remaining.contains(&record.name))
    );
    finish(runtime, &sdk, &zone, fetch, false);
    assert_eq!(completed(&a, &zone, BTreeSet::new()), checkpoint.pass);
    q6_assert_pairs(&a, &accepted, &facts);
    assert!(
        checkpoint
            .entities
            .iter()
            .all(|entity| actual::names(runtime).contains(&entity.name))
    );
    q6_local_rows(&a, &facts);
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    // First actual A sends follow Resume's settle/reconcile and this fresh SDK's
    // root confirmation. Only Service Saved outcomes acknowledge local work.
    assert!(runtime.sdk_root_confirmed());
    let local_acks = actual::send_pending(&a, &mut service, &sdk);
    assert!(
        local_acks
            .iter()
            .any(|r| matches!(decode(r).1, Payload::Note(n) if n.id == facts.note_id && n.flds == facts.edited_fields))
    );
    assert!(
        local_acks
            .iter()
            .any(|r| matches!(decode(r).1, Payload::Revlog(r) if r.id == facts.review_id))
    );
    for record in &local_acks {
        let tracked = runtime.entity_facts(&record.name).unwrap().unwrap();
        assert_eq!(tracked.acked_system_fields, record.system_fields);
        assert_eq!(tracked.acked_envelope.0, record.envelope);
        assert!(tracked.acked_rev > 0);
    }
    let fetch = begin(b.cloud_runtime().unwrap(), &b_sdk, &zone);
    let mut token = None;
    loop {
        let p = page(&mut service, &zone, token);
        q6_deliver(&b, &b_sdk, &zone, &p);
        q6_no_deletions(&b);
        if !p.more {
            break;
        }
        token = Some(p.token);
    }
    finish(b.cloud_runtime().unwrap(), &b_sdk, &zone, fetch, false);
    q6_local_rows(&b, &facts);
    assert_eq!(q6_content(&a), q6_content(&b));
    q6_assert_pairs(&a, &accepted, &facts);
    q6_assert_pairs(&b, &accepted, &facts);
    for record in &local_acks {
        let tracked = b.cloud_runtime().unwrap().entity_facts(&record.name).unwrap().unwrap();
        assert!(tracked.acked_payload.as_ref() == Some(&decode(record).1));
    }
    assert!(runtime.inbox_entries(binding.id).unwrap().is_empty());
    assert!(
        b.cloud_runtime()
            .unwrap()
            .inbox_entries(b.cloud_runtime().unwrap().status().unwrap().binding.unwrap().id)
            .unwrap()
            .is_empty()
    );
    a.close().unwrap();
    b.close().unwrap();
}

/// Q6 sign_out_resume's other-account variant; the positive test owns 300 pairs
/// and kill/reopen. All previews below use actual Service reads in their account.
#[test]
fn sign_out_resume_wrong_account_keeps_pending_note_and_reads_own_namespace() {
    use cloud::{
        entity_codec::Kind,
        publication::{ConnectDecision, PublicationError},
        runtime::RuntimeError,
    };
    use meguri_testkit::cloud::{ErrorCode, LogOutcome, RequestKind};
    use sign_out_absence as actual;

    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("q6-wrong-account");
    actual::generated(&dir);
    let account = Arc::new(actual::MutableAccount(std::sync::Mutex::new(identity())));
    let host = actual::open_actual(&dir, account.clone());
    let mut service = service(root.path());
    let binding = actual::publish(&host, &mut service);
    let (old_sdk, _, _) = admit(&host);
    confirm_root(&host, &old_sdk, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let original = runtime.entity_facts_page(Kind::Note, None).unwrap().remove(0);
    assert!(original.acked_rev > 0);
    assert!(!original.acked_system_fields.is_empty());
    assert!(!original.acked_envelope.0.is_empty());
    let Payload::Note(note) = &original.payload else {
        panic!("actual published Note")
    };
    let mut fields: Vec<String> = std::str::from_utf8(&note.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    fields[1] = "Q6 edit before sign-out; never sent to the other account".into();
    let edited_fields = fields.join("\u{1f}").into_bytes();
    let edit = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id: meguri_model::ids::NoteId(note.id),
            fields,
            tags: std::str::from_utf8(&note.tags)
                .unwrap()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    assert!(edit.value.written);
    let pending = runtime.entity_facts(&original.name).unwrap().unwrap();
    assert!(pending.mark.is_some());
    assert!(pending.local_rev > original.local_rev && pending.local_rev > pending.acked_rev);
    assert_eq!(pending.observed_seq, edit.seq.0);
    assert_eq!(pending.acked_rev, original.acked_rev);
    assert_eq!(pending.acked_system_fields, original.acked_system_fields);
    assert!(pending.acked_payload == original.acked_payload);
    assert!(pending.acked_envelope == original.acked_envelope);
    assert!(matches!(&pending.payload, Payload::Note(n) if n.flds == edited_fields));
    let local_rows = rows(&host);
    let account1_records = service.inspect(&scope()).unwrap().unwrap();
    let other_scope = Scope {
        account: "synthetic-account-B".into(),
        ..scope()
    };
    assert!(service.inspect(&other_scope).unwrap().is_none());
    let requests = service.request_log().unwrap().len();

    *account.0.lock().unwrap() = IdentityStatus::SignedOut;
    runtime.refresh_identity().unwrap();
    assert_eq!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::SignedOut)
    );
    assert!(runtime.resume_binding_offer().is_none());
    assert!(runtime.validate_sdk(&old_sdk).is_err());
    let blocked = runtime
        .build_collection_batch(&old_sdk, &collection, &actual::names(runtime))
        .unwrap();
    assert_eq!(blocked.id, 0);
    assert!(blocked.saves.is_empty());
    assert!(runtime.entity_facts(&original.name).unwrap().unwrap() == pending);
    assert_eq!(service.request_log().unwrap().len(), requests);
    // The original SDK owns cancellation and release; no replacement SDK is made.
    runtime.retire_sdk(&old_sdk, false).unwrap();
    runtime.release_sdk(&old_sdk).unwrap();
    assert_eq!(service.request_log().unwrap().len(), requests);

    *account.0.lock().unwrap() = identity();
    runtime.refresh_identity().unwrap();
    let retained = actual::actual_preview(&host, &mut service);
    assert_eq!(retained.decision, ConnectDecision::ConfirmResume);
    assert_eq!(retained.resume_binding, Some(binding.id));
    assert!(retained.unsent_by_kind[&Kind::Note] >= 1);
    let requests = service.request_log().unwrap().len();
    *account.0.lock().unwrap() = IdentityStatus::Available(OpaqueBytes(other_scope.account.as_bytes().to_vec()));
    // Provider revalidation must refuse even before the account notification.
    assert!(matches!(
        host.resume_cloud_binding(&retained, collection.generation()),
        Err(RuntimeError::Publication(PublicationError::AccountMismatch))
    ));
    assert!(matches!(
        host.resume_cloud_binding(&retained, collection.generation()),
        Err(RuntimeError::Publication(PublicationError::NotOffered))
    ));
    runtime.refresh_identity().unwrap();
    let assert_retained = || {
        let current = runtime.status().unwrap().binding.unwrap();
        assert_eq!(current.id, binding.id);
        assert!(current.facts.account == binding.facts.account);
        assert!(matches!(
            current.facts.state,
            cloud::BindingState::Isolated(cloud::IsolationReason::SignedOut | cloud::IsolationReason::AccountChanged)
        ));
        assert!(runtime.entity_facts(&original.name).unwrap().unwrap() == pending);
        assert_eq!(rows(&host), local_rows);
    };
    assert_retained();
    assert!(runtime.resume_binding_offer().is_none());
    assert_eq!(service.request_log().unwrap().len(), requests);

    let other = actual::actual_preview_for_scope(&host, &mut service, &other_scope);
    assert_ne!(other.id, retained.id);
    assert!(other.account.0 == other_scope.account.as_bytes());
    assert_eq!(other.decision, ConnectDecision::ConfirmUpload);
    assert!(other.resume_binding.is_none() && other.active.is_none() && other.cloud.is_none());
    assert!(runtime.resume_binding_offer().is_none());
    assert!(matches!(
        host.resume_cloud_binding(&other, collection.generation()),
        Err(RuntimeError::Publication(PublicationError::NotOffered))
    ));
    let log = service.request_log().unwrap();
    assert_eq!(log.len(), requests + 1);
    let read = &log[requests];
    assert!(read.scope == other_scope);
    assert_eq!(read.kind, RequestKind::Read);
    assert_eq!(read.zone.as_deref(), Some("meguri-root"));
    assert_eq!(read.outcome, LogOutcome::Failure(ErrorCode::ZoneNotFound));
    assert!(
        log.iter()
            .filter(|r| r.scope == other_scope)
            .all(|r| r.kind == RequestKind::Read)
    );
    assert!(
        service
            .inspect(&other_scope)
            .unwrap()
            .is_none_or(|db| db.zones.is_empty() && db.used_bytes == 0)
    );
    assert!(service.inspect(&scope()).unwrap().unwrap() == account1_records);
    assert_retained();

    *account.0.lock().unwrap() = identity();
    runtime.refresh_identity().unwrap();
    assert!(runtime.resume_binding_offer().is_none());
    assert!(matches!(
        host.resume_cloud_binding(&retained, collection.generation()),
        Err(RuntimeError::Publication(PublicationError::NotOffered))
    ));
    assert_retained();
    assert_eq!(service.request_log().unwrap().len(), log.len());
    let fresh = actual::actual_preview(&host, &mut service);
    assert_ne!(fresh.id, retained.id);
    assert_ne!(fresh.id, other.id);
    assert_eq!(fresh.decision, ConnectDecision::ConfirmResume);
    assert_eq!(fresh.resume_binding, Some(binding.id));
    assert!(fresh.unsent_by_kind[&Kind::Note] >= 1);
    assert!(matches!(
        host.resume_cloud_binding(&retained, collection.generation()),
        Err(RuntimeError::Publication(PublicationError::NotOffered))
    ));
    assert_retained();
    assert!(service.inspect(&scope()).unwrap().unwrap() == account1_records);
    assert!(
        service
            .inspect(&other_scope)
            .unwrap()
            .is_none_or(|db| db.zones.is_empty() && db.used_bytes == 0)
    );
    let isolated = runtime.status().unwrap().binding.unwrap();
    host.close().unwrap();
    drop(collection);
    drop(host);

    // A real isolated reopen advances only per-open bookkeeping. The pending
    // edit, captured base, ACKs and observer stamps must remain exact.
    let cloud::OpenOutcome::Opened(store) = cloud::TrackingStore::open(&dir, binding.scope.clone()).unwrap() else {
        panic!("closed tracking must remain readable")
    };
    let mut expected = store.stored_runtime_state(binding.id).unwrap().unwrap();
    drop(store);
    expected.open_counter = expected.open_counter.checked_add(1).unwrap();
    expected.drained = false;
    let requests = service.request_log().unwrap().len();
    let host = actual::open_actual(&dir, account);
    let runtime = host.cloud_runtime().unwrap();
    assert_eq!(runtime.stored_state_for_testing().unwrap(), expected);
    assert!(runtime.status().unwrap().binding.unwrap() == isolated);
    assert!(runtime.entity_facts(&original.name).unwrap().unwrap() == pending);
    assert_eq!(rows(&host), local_rows);
    assert!(runtime.resume_binding_offer().is_none());
    assert_eq!(service.request_log().unwrap().len(), requests);
    host.close().unwrap();
}

/// Subprocess infrastructure only. Parent owns kill/reap even if this panics.
#[test]
fn cloud_q6_resume_absence_child_checkpoint() {
    use cloud::{entity_codec::Kind, publication::ConnectDecision};
    use sign_out_absence as actual;
    let Some(root) = std::env::var_os(Q6_CHILD_ENV).map(std::path::PathBuf::from) else {
        return;
    };
    let facts: Q6LocalFacts =
        serde_json::from_slice(&std::fs::read(root.join("q6-local-facts.json")).unwrap()).unwrap();
    let account = Arc::new(actual::MutableAccount(std::sync::Mutex::new(IdentityStatus::SignedOut)));
    let host = actual::open_actual(&root.join("q6-a"), account.clone());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    q6_local_rows(&host, &facts);
    *account.0.lock().unwrap() = identity();
    runtime.refresh_identity().unwrap();
    assert_eq!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::SignedOut)
    );
    assert!(runtime.resume_binding_offer().is_none());
    assert!(runtime.resume_binding(binding.id, &collection).is_err());
    let mut service = service(&root);
    let preview = actual::actual_preview(&host, &mut service);
    assert_eq!(preview.decision, ConnectDecision::ConfirmResume);
    assert_eq!(preview.resume_binding, Some(binding.id));
    assert_eq!(runtime.resume_binding_offer(), Some(binding.id));
    assert!(preview.unsent_by_kind[&Kind::Note] >= 1);
    assert!(preview.unsent_by_kind[&Kind::Revlog] >= 1);
    let open = runtime.stored_state_for_testing().unwrap().open_counter;
    let requests = service.request_log().unwrap().len();
    host.resume_cloud_binding(&preview, collection.generation()).unwrap();
    assert_eq!(
        service.request_log().unwrap().len(),
        requests,
        "Resume performs no Service writes"
    );
    assert_eq!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        cloud::BindingState::Active
    );
    assert_eq!(runtime.stored_state_for_testing().unwrap().open_counter, open);
    assert!(!runtime.stored_state_for_testing().unwrap().drained);
    assert!(runtime.incoming_engine_state_for_testing(binding.id).unwrap().is_none());
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(!runtime.status().unwrap().scan_required);
    let edited_note = runtime.entity_facts(&facts.note_name).unwrap().unwrap();
    assert!(
        matches!(&edited_note.payload, Payload::Note(n) if n.id == facts.note_id && n.flds == facts.edited_fields)
            && edited_note.mark.is_some(),
        "payload-is-note={} id-matches={} fields-match={} mark-present={} local_rev={} acked_rev={} observer_open={} open={} observed_seq={} collection_seq={} observed_generation={} collection_generation={}",
        matches!(&edited_note.payload, Payload::Note(_)),
        matches!(&edited_note.payload, Payload::Note(n) if n.id == facts.note_id),
        matches!(&edited_note.payload, Payload::Note(n) if n.flds == facts.edited_fields),
        edited_note.mark.is_some(),
        edited_note.local_rev,
        edited_note.acked_rev,
        edited_note.observer_open,
        open,
        edited_note.observed_seq,
        collection.last_seq().0,
        edited_note.observed_generation,
        collection.generation().0
    );
    assert!(!runtime.entity_facts_page(Kind::Revlog, None).unwrap().is_empty());
    let (sdk, state, zone) = admit(&host);
    assert!(state.is_none(), "explicit Resume must start a cursorless SDK");
    actual::assert_send_fenced(&host, &sdk);
    confirm_root(&host, &sdk, &mut service);
    assert!(
        runtime.incoming_runs().is_empty(),
        "no earlier apply may stand in for the first fetched page"
    );
    let _fetch = begin(runtime, &sdk, &zone);
    let p = page(&mut service, &zone, None);
    assert_eq!(p.changes.len(), 50);
    assert!(p.more);
    assert!(p.changes.iter().all(|c| matches!(c, RecordChange::Saved(_))));
    let seen = q6_deliver(&host, &sdk, &zone, &p);
    let runs = runtime.incoming_runs();
    let receipts = runtime.incoming_wal_receipts();
    assert!(
        !runs.is_empty(),
        "the first real page must apply, not just advance a cursor"
    );
    let mut wal = Vec::new();
    for run in runs {
        assert_eq!(run.generation, collection.generation().0);
        assert!(receipts.iter().any(|receipt| receipt.identity().native_file()
            == (run.native_file.device, run.native_file.inode)
            && receipt.identity().generation().0 == run.generation
            && receipt.through().0 >= run.seq));
        wal.push((run.generation, run.seq));
    }
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(logged(runtime).is_empty());
    q6_no_deletions(&host);
    q6_local_rows(&host, &facts);
    let snapshot = runtime.adapter_snapshot().unwrap();
    let pass = actual::running_pass(runtime);
    let checkpoint = Q6Checkpoint {
        serialization: snapshot.serialization.unwrap(),
        seen,
        pass,
        entities: snapshot.entities,
        inbox_marks: snapshot
            .inbox
            .into_iter()
            .map(|entry| (entry.record_name, entry.absence_pass))
            .collect(),
        wal,
    };
    assert!(checkpoint.inbox_marks.iter().all(|(_, mark)| *mark == Some(pass)));
    let checkpoint = serde_json::to_string(&checkpoint).unwrap();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == IncomingStage::FetchAfterCursor {
            println!("{Q6_CHECKPOINT}{checkpoint}");
            std::io::stdout().flush().unwrap();
            let _ = std::io::stdin().read(&mut [0]);
            panic!("parent must SIGKILL the A process at the real checkpoint");
        }
    });
    // Same post-cursor checkpoint seam as cloud_absence_child_checkpoint. The
    // apply and WAL receipt above are real; the seam only holds this process.
    runtime.incoming_stage_for_testing(IncomingStage::FetchAfterCursor);
}

const EPOCH_LOSS_CHILD_ENV: &str = "MEGURI_EPOCH_LOSS_CHILD";
const EPOCH_LOSS_CHECKPOINT: &str = "EPOCH_LOSS_ACCEPTED_CHECKPOINT ";

#[derive(serde::Serialize, serde::Deserialize)]
struct EpochLossCheckpoint {
    serialization: Vec<u8>,
    binding: Vec<u8>,
    entities: Vec<cloud::workflow::EntityFacts>,
    root: Option<cloud::workflow::RootAcknowledgement>,
    drains: Vec<cloud::adapter::FetchDrain>,
}

#[test]
fn failed_inferred_epoch_read_then_real_collection_fetch_completes_cursorless_absence() {
    use cloud::publication::RecordRead;
    use sign_out_absence as actual;

    struct OwnerCleanup<'a>(&'a CoreHost);
    impl Drop for OwnerCleanup<'_> {
        fn drop(&mut self) {
            if let Some(runtime) = self.0.cloud_runtime()
                && let Some(sdk) = runtime.incoming_sdk_instance_for_testing()
            {
                let _ = runtime.retire_sdk(&sdk, false);
                let _ = runtime.release_sdk(&sdk);
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    actual::generated(&root.path().join("epoch-progress-a"));
    let publisher = actual::open_actual(&root.path().join("epoch-progress-a"), Arc::new(Account));
    let publisher_cleanup = OwnerCleanup(&publisher);
    let mut service = service(root.path());
    actual::publish(&publisher, &mut service);
    let host = actual::adopt(&root.path().join("epoch-progress-b"), &mut service);
    let owner_cleanup = OwnerCleanup(&host);
    drop(publisher_cleanup);
    publisher.close().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let (sdk, serialization, zone) = admit(&host);
    assert!(serialization.is_none());
    confirm_root(&host, &sdk, &mut service);
    let before_rows = rows(&host);
    let expected_names = actual::names(runtime);
    assert!(!expected_names.is_empty());
    let drains_before = runtime.adapter_fetch_drains();
    assert!(logged(runtime).is_empty());

    // A qualified owned fetch with no per-zone callbacks issues its real epoch
    // verification. Service has no transient read-failure injection; only this
    // initial negative result uses Runtime's defined failure-input seam.
    let fetch = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    assert!(runtime.adapter_fetch_qualified(&sdk, fetch).unwrap());
    let mut reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
    assert_eq!(reads.len(), 1);
    let epoch = reads.remove(0);
    assert!(!epoch.is_active() && epoch.zone() == zone);
    let issued: serde_json::Value =
        serde_json::from_slice(&runtime.adapter_snapshot().unwrap().tracking_content).unwrap();
    assert!(!issued[4]["epoch_verification"].is_null());
    assert!(
        runtime
            .adapter_complete_fetch_read(&epoch, &collection, RecordRead::Failed, false, None, None)
            .unwrap()
            .is_none()
    );
    assert!(!runtime.adapter_finish_fetch(&sdk, fetch).unwrap());
    let failed: serde_json::Value =
        serde_json::from_slice(&runtime.adapter_snapshot().unwrap().tracking_content).unwrap();
    assert_eq!(failed[4]["epoch_verification"], issued[4]["epoch_verification"]);
    assert!(failed[4]["absence"].is_null());
    assert_eq!(runtime.adapter_fetch_drains(), drains_before);
    let diagnostic = runtime.adapter_root_read_diagnostics().pop().unwrap();
    assert!(diagnostic.accepted && !diagnostic.active && diagnostic.outcome == "failed");
    safe(&host, &before_rows);

    // The next call really starts and finishes the collection zone at the same
    // no-state point. Its page and any still-issued epoch read come from Service.
    let fetch = begin(runtime, &sdk, &zone);
    let pass = actual::running_pass(runtime);
    let drains_before_success = runtime.adapter_fetch_drains();
    let p = page(&mut service, &zone, None);
    assert!(!p.more);
    assert!(p.changes.iter().all(|c| matches!(c, RecordChange::Saved(_))));
    let seen = deliver(&host, &sdk, &zone, &p, true);
    assert!(expected_names.is_subset(&seen));
    runtime.adapter_fetch_zone(&sdk, zone.clone(), false, false).unwrap();
    runtime
        .adapter_did_fetch(&sdk, &format!("absence-{fetch}"), true)
        .unwrap();
    assert!(runtime.adapter_fetch_qualified(&sdk, fetch).unwrap());
    let reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
    assert!(reads.len() <= 1);
    for epoch in reads {
        assert!(!epoch.is_active() && epoch.zone() == zone);
        let result = actual::read_control(&mut service, epoch.zone(), "epoch");
        assert!(matches!(&result, RecordRead::Found(_)));
        assert!(
            runtime
                .adapter_complete_fetch_read(&epoch, &collection, result, false, None, None)
                .unwrap()
                .is_none()
        );
    }
    let finished = runtime.adapter_finish_fetch(&sdk, fetch).unwrap();
    let after = runtime.adapter_snapshot().unwrap();
    let after_rows = rows(&host);
    let drains = runtime.adapter_fetch_drains();
    let completions = logged(runtime);
    let binding = runtime.status().unwrap().binding.unwrap().id;
    runtime.validate_sdk(&sdk).unwrap();
    // Retire/release and close before the expected-red progress assertions.
    runtime.retire_sdk(&sdk, false).unwrap();
    runtime.release_sdk(&sdk).unwrap();
    drop(owner_cleanup);
    drop(collection);
    host.close().unwrap();

    assert_eq!(after_rows, before_rows);
    assert!(after.inbox.iter().all(|e| !matches!(e.value, InboxValue::Deletion)));
    assert_eq!(&drains[..drains_before_success.len()], &drains_before_success);
    assert_eq!(
        &drains[drains_before_success.len()..],
        &[cloud::adapter::FetchDrain {
            zone: zone.clone(),
            fetched: true,
        }],
        "R5 P2: successful post-point zone callbacks must retain fetched:true across epoch verification"
    );
    assert!(finished);
    assert!(
        after.absence_pass_completed,
        "R5 P2: the original cursorless pass must complete"
    );
    assert_eq!(completions, vec![(binding, zone, pass, 0, BTreeSet::new())]);
}

// Each case is independently selectable; a RED crash witness cannot hide the
// other service outcome or the already-durable stronger account reasons.
#[test]
fn missing_epoch_unknown_item_sigkill_saved_state_reopen() {
    epoch_loss_sigkill_reopen(false, "none");
}
#[test]
fn missing_epoch_zone_not_found_sigkill_saved_state_reopen() {
    epoch_loss_sigkill_reopen(true, "none");
}
#[test]
fn missing_epoch_unknown_item_sigkill_preserves_signed_out() {
    epoch_loss_sigkill_reopen(false, "signedOut");
}
#[test]
fn missing_epoch_zone_not_found_sigkill_preserves_signed_out() {
    epoch_loss_sigkill_reopen(true, "signedOut");
}
#[test]
fn missing_epoch_unknown_item_sigkill_preserves_account_changed() {
    epoch_loss_sigkill_reopen(false, "accountChanged");
}
#[test]
fn missing_epoch_zone_not_found_sigkill_preserves_account_changed() {
    epoch_loss_sigkill_reopen(true, "accountChanged");
}

fn epoch_loss_sigkill_reopen(zone_missing: bool, stronger: &str) {
    use sign_out_absence as actual;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("epoch-loss-case.json"),
        serde_json::to_vec(&(zone_missing, stronger)).unwrap(),
    )
    .unwrap();
    let (mut child, rx) = Q6Child::spawn_checkpoint(
        root.path(),
        "cloud_missing_epoch_accepted_child_checkpoint",
        EPOCH_LOSS_CHILD_ENV,
        EPOCH_LOSS_CHECKPOINT,
    );
    let checkpoint = rx.recv_timeout(BUDGET);
    // Never parse/assert before the process has been killed and reaped. Drop
    // owns this same cleanup if receiving, parsing or a later assertion fails.
    let (killed, reaped) = child.stop().unwrap();
    killed.unwrap();
    let status = reaped.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9), "actual SIGKILL at accepted epoch return");
    }
    let checkpoint: EpochLossCheckpoint = serde_json::from_str(
        &checkpoint.expect("accepted missing epoch must return its real Active followup within BUDGET"),
    )
    .unwrap();
    assert!(!checkpoint.serialization.is_empty());
    assert!(
        checkpoint
            .entities
            .iter()
            .any(|e| e.mark.is_some() && e.local_rev > e.acked_rev)
    );

    // Reopen the actual adopted collection and its original tracking files.
    // CoreHost's Drop closes on assertion/panic as well as explicit close below.
    let host = actual::open_actual(&root.path().join("epoch-loss-b"), Arc::new(Account));
    let runtime = host.cloud_runtime().unwrap();
    let reopened = runtime.adapter_snapshot().unwrap();
    let before_rows = rows(&host);
    let mut service = service(root.path());
    let mut owner = None;
    let mut restored = None;
    let drains_before;
    if runtime.status().unwrap().binding.unwrap().facts.state == cloud::BindingState::Active {
        let (sdk, serialization, zone) = admit(&host);
        restored = serialization;
        owner = Some(sdk.clone());
        // Replay owned fetch boundaries for unchanged zones, with no record or
        // zone callbacks. Reopen keeps the SDK bytes; no account event resets
        // them. Any recovered loss evidence must come from production.
        drains_before = runtime.adapter_fetch_drains();
        for target in ["meguri-root", zone.as_str()] {
            if runtime.validate_sdk(&sdk).is_err() {
                break;
            }
            let fetch = runtime.adapter_begin_fetch(&sdk, vec![target.into()]).unwrap();
            let group = format!("meguri-{fetch}");
            runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
            let did = runtime.adapter_did_fetch(&sdk, &group, true);
            if runtime.validate_sdk(&sdk).is_err() {
                break;
            }
            did.unwrap();
            let reads = runtime
                .adapter_fetch_reads(&sdk, fetch, &host.collection().unwrap())
                .unwrap();
            for read in reads {
                let result = actual::read_control(
                    &mut service,
                    read.zone(),
                    if read.is_active() { "active" } else { "epoch" },
                );
                let followup = runtime
                    .adapter_complete_fetch_read(&read, &host.collection().unwrap(), result, false, None, None)
                    .unwrap();
                if let Some(active) = followup {
                    assert!(active.is_active());
                    let result = actual::read_control(&mut service, active.zone(), "active");
                    assert!(
                        runtime
                            .adapter_complete_fetch_read(
                                &active,
                                &host.collection().unwrap(),
                                result,
                                false,
                                None,
                                None
                            )
                            .unwrap()
                            .is_none()
                    );
                }
            }
            if runtime.validate_sdk(&sdk).is_ok() {
                let _ = runtime.adapter_finish_fetch(&sdk, fetch).unwrap();
            }
        }
    } else {
        drains_before = runtime.adapter_fetch_drains();
        assert!(
            runtime.register_sdk(false).is_err(),
            "durably isolated recovery must refuse SDK admission"
        );
    }
    let had_owner = owner.is_some();
    let confirmed = runtime.sdk_root_confirmed();
    let confirmations = runtime
        .trace()
        .iter()
        .filter(|e| matches!(e, TraceEvent::RootConfirmed { .. }))
        .count();
    let drains = runtime.adapter_fetch_drains();
    let batch = owner.as_ref().and_then(|sdk| {
        runtime
            .build_collection_batch(sdk, &host.collection().unwrap(), &actual::names(runtime))
            .ok()
    });
    let send_fenced = batch.as_ref().is_none_or(|b| b.id == 0 && b.saves.is_empty());
    let after = runtime.adapter_snapshot().unwrap();
    let state = runtime.status().unwrap().binding.unwrap().facts.state;
    let unchanged_rows = rows(&host) == before_rows;
    let no_absence_completion = logged(runtime).is_empty();
    // Capture every outcome before cleanup can change authority or isolation.
    if let Some(sdk) = owner {
        runtime.retire_sdk(&sdk, false).unwrap();
        runtime.release_sdk(&sdk).unwrap();
    }
    host.close().unwrap();

    assert!(
        reopened.serialization == Some(checkpoint.serialization.clone()),
        "saved SDK state must survive SIGKILL"
    );
    if had_owner {
        assert_eq!(restored, Some(checkpoint.serialization.clone()));
    }
    assert!(reopened.binding == checkpoint.binding && after.binding == checkpoint.binding);
    for pending in checkpoint
        .entities
        .iter()
        .filter(|e| e.mark.is_some() && e.local_rev > e.acked_rev)
    {
        if let Payload::Note(note) = &pending.payload {
            let row = before_rows[&Table::Notes]
                .iter()
                .find(|row| row[0] == Cell::Integer(note.id))
                .unwrap();
            assert_eq!(
                row[6],
                Cell::Text(note.flds.clone()),
                "pending local row must survive the process kill"
            );
        }
        for snapshot in [&reopened, &after] {
            let retained = snapshot.entities.iter().find(|e| e.name == pending.name).unwrap();
            assert!(
                retained.local_rev == pending.local_rev
                    && retained.acked_rev == pending.acked_rev
                    && retained.mark == pending.mark
                    && retained.payload == pending.payload
                    && retained.acked_payload == pending.acked_payload
                    && retained.in_flight == pending.in_flight,
                "actual pending entity must survive loss recovery without send/ACK"
            );
        }
    }
    assert!(unchanged_rows);
    assert!(
        after
            .inbox
            .iter()
            .all(|entry| !matches!(entry.value, InboxValue::Deletion))
    );
    assert!(reopened.root == checkpoint.root && after.root == checkpoint.root);
    assert!(
        drains == drains_before,
        "unchanged callbacks must not drain an accepted lost epoch"
    );
    assert!(
        !confirmed && confirmations == 0,
        "saved Active cannot confirm after accepted epoch loss"
    );
    assert!(send_fenced, "recovery must fence collection sends");
    assert!(no_absence_completion && !after.absence_pass_completed);
    let expected = match stronger {
        "signedOut" => cloud::IsolationReason::SignedOut,
        "accountChanged" => cloud::IsolationReason::AccountChanged,
        "none" => cloud::IsolationReason::ZoneMissing,
        _ => unreachable!(),
    };
    assert_eq!(
        state,
        cloud::BindingState::Isolated(expected),
        "loss must settle without user confirmation"
    );
}

/// Same Q6 child owner/pipe family; this checkpoint is an actual returned
/// FetchRead, not a stage callback that invents acceptance or isolation.
#[test]
fn cloud_missing_epoch_accepted_child_checkpoint() {
    use sign_out_absence as actual;
    let Some(root) = std::env::var_os(EPOCH_LOSS_CHILD_ENV).map(std::path::PathBuf::from) else {
        return;
    };
    let (zone_missing, stronger): (bool, String) =
        serde_json::from_slice(&std::fs::read(root.join("epoch-loss-case.json")).unwrap()).unwrap();
    actual::generated(&root.join("epoch-loss-a"));
    let publisher = actual::open_actual(&root.join("epoch-loss-a"), Arc::new(Account));
    let mut service = service(&root);
    actual::publish(&publisher, &mut service);
    let host = actual::adopt(&root.join("epoch-loss-b"), &mut service);
    publisher.close().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let (old, _, zone) = admit(&host);
    confirm_root(&host, &old, &mut service);
    // Deliver a Service page and retain its cursor through the SDK state event.
    // The replacement SDK loads those bytes before its real account callback
    // establishes savedAccountReset; no tracking facts are supplied by hand.
    let fetch = begin(runtime, &old, &zone);
    let p = page(&mut service, &zone, None);
    assert!(!p.more);
    deliver(&host, &old, &zone, &p, true);
    finish(runtime, &old, &zone, fetch, false);
    runtime.retire_sdk(&old, false).unwrap();
    runtime.release_sdk(&old).unwrap();
    let (sdk, serialization, _) = admit(&host);
    assert!(serialization.is_some());
    runtime.adapter_account(Some(&sdk), identity()).unwrap();
    let note = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = note.payload else {
        panic!("actual adopted note required")
    };
    meguri_engine::notes::update(
        &host.collection().unwrap(),
        meguri_engine::notes::UpdateNote {
            id: meguri_model::ids::NoteId(note.id),
            fields: vec!["pending crash witness".into(), "retained back".into()],
            tags: vec![],
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    runtime
        .reconcile_binding(&host.collection().unwrap(), &host.media().unwrap(), BUDGET)
        .unwrap();
    let operation = if zone_missing {
        Operation::DeleteZone {
            zone: zone.clone(),
            cause: meguri_testkit::cloud::RemovalCause::Deleted,
        }
    } else {
        Operation::Modify {
            zone: zone.clone(),
            atomic: false,
            changes: vec![Mutation::Delete { name: "epoch".into() }],
        }
    };
    service
        .request(&scope(), Caller::default(), operation)
        .unwrap()
        .result
        .unwrap();
    let fetch = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    let mut reads = runtime
        .adapter_fetch_reads(&sdk, fetch, &host.collection().unwrap())
        .unwrap();
    assert_eq!(reads.len(), 1);
    let epoch = reads.remove(0);
    assert!(!epoch.is_active() && epoch.zone() == zone);
    let result = actual::read_control(&mut service, &zone, "epoch");
    assert!(matches!(
        (&result, zone_missing),
        (cloud::publication::RecordRead::Missing, false) | (cloud::publication::RecordRead::ZoneMissing, true)
    ));
    let active = runtime
        .adapter_complete_fetch_read(&epoch, &host.collection().unwrap(), result, false, None, None)
        .unwrap()
        .expect("accepted epoch loss must return the original Active followup");
    assert!(active.is_active() && active.zone() == "meguri-root");
    let diagnostic = runtime.adapter_root_read_diagnostics().pop().unwrap();
    assert!(diagnostic.accepted && !diagnostic.active && !diagnostic.lost_collection);
    assert_eq!(
        diagnostic.outcome,
        if zone_missing { "zoneNotFound" } else { "unknownItem" }
    );
    // Apply the stronger account reasons through runtime events after the
    // epoch result returns, keeping the original SDK and tracking ownership.
    match stronger.as_str() {
        "none" => {}
        "signedOut" => runtime.adapter_account(None, IdentityStatus::SignedOut).unwrap(),
        "accountChanged" => runtime
            .adapter_account(
                None,
                IdentityStatus::Available(OpaqueBytes(b"synthetic-account-B".to_vec())),
            )
            .unwrap(),
        _ => panic!("unknown synthetic case"),
    }
    let snapshot = runtime.adapter_snapshot().unwrap();
    let checkpoint = EpochLossCheckpoint {
        serialization: snapshot.serialization.unwrap(),
        binding: snapshot.binding,
        entities: snapshot.entities,
        root: snapshot.root,
        drains: runtime.adapter_fetch_drains(),
    };
    assert!(
        checkpoint
            .entities
            .iter()
            .any(|e| e.mark.is_some() && e.local_rev > e.acked_rev)
    );
    // Keep Active and Host alive. No completion, finish-fetch, retire, close or
    // Drop runs between this real accepted return and the parent's SIGKILL.
    println!("{EPOCH_LOSS_CHECKPOINT}{}", serde_json::to_string(&checkpoint).unwrap());
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0]);
    panic!("parent must SIGKILL/reap before accepting Active or retiring SDK");
}

// K12.5/K22: characterize a returned storage failure before loss is durable.
// Keep the two Service outcomes and two I/O kinds independently selectable.
#[test]
fn missing_epoch_unknown_item_obligation_commit_other_saved_state_reopen() {
    epoch_loss_commit_error_reopen(false, std::io::ErrorKind::Other);
}
#[test]
fn missing_epoch_unknown_item_obligation_commit_storage_full_saved_state_reopen() {
    epoch_loss_commit_error_reopen(false, std::io::ErrorKind::StorageFull);
}
#[test]
fn missing_epoch_zone_not_found_obligation_commit_other_saved_state_reopen() {
    epoch_loss_commit_error_reopen(true, std::io::ErrorKind::Other);
}
#[test]
fn missing_epoch_zone_not_found_obligation_commit_storage_full_saved_state_reopen() {
    epoch_loss_commit_error_reopen(true, std::io::ErrorKind::StorageFull);
}

fn epoch_loss_commit_error_reopen(zone_missing: bool, kind: std::io::ErrorKind) {
    use meguri_model::faultpoint::{self, Fault};
    use sign_out_absence as actual;
    const POINT: &str = "cloud.adapter.before_commit";
    // Use the existing fault helpers with the same unwind-safe scope pattern
    // as the I/O tests. Nothing except the completion runs while armed.
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            faultpoint::disarm(POINT);
        }
    }
    // CoreHost already closes on unwind. End the actual occupied SDK slot
    // first, including if admission or a later assertion panics.
    struct OwnerCleanup<'a>(&'a CoreHost);
    impl Drop for OwnerCleanup<'_> {
        fn drop(&mut self) {
            if let Some(runtime) = self.0.cloud_runtime()
                && let Some(sdk) = runtime.incoming_sdk_instance_for_testing()
            {
                let _ = runtime.retire_sdk(&sdk, false);
                let _ = runtime.release_sdk(&sdk);
            }
        }
    }
    let root = tempfile::tempdir().unwrap();
    actual::generated(&root.path().join("commit-loss-a"));
    let publisher = actual::open_actual(&root.path().join("commit-loss-a"), Arc::new(Account));
    let publisher_cleanup = OwnerCleanup(&publisher);
    let mut service = service(root.path());
    actual::publish(&publisher, &mut service);
    let directory = root.path().join("commit-loss-b");
    let host = actual::adopt(&directory, &mut service);
    let owner_cleanup = OwnerCleanup(&host);
    drop(publisher_cleanup);
    publisher.close().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let (old, _, zone) = admit(&host);
    confirm_root(&host, &old, &mut service);
    // Match the crash fixture's savedAccountReset setup using actual pages,
    // SDK events and account input. This reset occurs only BEFORE the fault.
    let fetch = begin(runtime, &old, &zone);
    let p = page(&mut service, &zone, None);
    assert!(!p.more);
    deliver(&host, &old, &zone, &p, true);
    finish(runtime, &old, &zone, fetch, false);
    runtime.retire_sdk(&old, false).unwrap();
    runtime.release_sdk(&old).unwrap();
    let (sdk, serialization, _) = admit(&host);
    assert!(serialization.is_some());
    runtime.adapter_account(Some(&sdk), identity()).unwrap();
    let note = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = note.payload else {
        panic!("actual adopted note required")
    };
    meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id: meguri_model::ids::NoteId(note.id),
            fields: vec!["pending commit witness".into(), "retained back".into()],
            tags: vec![],
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), BUDGET)
        .unwrap();
    let operation = if zone_missing {
        Operation::DeleteZone {
            zone: zone.clone(),
            cause: meguri_testkit::cloud::RemovalCause::Deleted,
        }
    } else {
        Operation::Modify {
            zone: zone.clone(),
            atomic: false,
            changes: vec![Mutation::Delete { name: "epoch".into() }],
        }
    };
    service
        .request(&scope(), Caller::default(), operation)
        .unwrap()
        .result
        .unwrap();
    let fetch = runtime.adapter_begin_fetch(&sdk, vec![zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    let mut reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
    assert_eq!(reads.len(), 1);
    let epoch = reads.remove(0);
    assert!(!epoch.is_active() && epoch.zone() == zone);
    let result = actual::read_control(&mut service, &zone, "epoch");
    assert!(matches!(
        (&result, zone_missing),
        (cloud::publication::RecordRead::Missing, false) | (cloud::publication::RecordRead::ZoneMissing, true)
    ));
    let before = runtime.adapter_snapshot().unwrap();
    let before_rows = rows(&host);
    let drains_before = runtime.adapter_fetch_drains();
    let absence_before = logged(runtime);
    let confirmations_before = runtime
        .trace()
        .iter()
        .filter(|e| matches!(e, TraceEvent::RootConfirmed { .. }))
        .count();
    let requests_before = service.request_log().unwrap();
    let durable_before: serde_json::Value = serde_json::from_slice(&before.tracking_content).unwrap();
    assert!(
        before
            .entities
            .iter()
            .any(|e| e.mark.is_some() && e.local_rev > e.acked_rev)
    );
    assert!(before.root.is_some() && before.serialization == serialization);
    assert!(durable_before[4]["collection_loss"].is_null());
    assert!(!runtime.sdk_root_confirmed());

    let failed = {
        let _disarm = Disarm;
        faultpoint::arm(POINT, Fault::IoError(kind));
        runtime.adapter_complete_fetch_read(&epoch, &collection, result, false, None, None)
    }; // Disarm before assertions, snapshot reads, sends or owner cleanup.
    assert!(
        !matches!(&failed, Ok(Some(_))),
        "a failed obligation commit must return no Active followup"
    );
    assert!(
        matches!(failed, Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::Io(actual))) if actual == kind),
        "the real completion must return the injected tracking I/O kind"
    );
    let status = runtime.status().unwrap();
    assert!(
        status.tracking_stopped && status.scan_required && !status.scan_completed_this_open && !status.scan_drained
    );
    let batch = runtime
        .build_collection_batch(&sdk, &collection, &actual::names(runtime))
        .unwrap();
    assert_eq!(batch.id, 0);
    assert!(batch.saves.is_empty());
    let after = runtime.adapter_snapshot().unwrap();
    let durable_after: serde_json::Value = serde_json::from_slice(&after.tracking_content).unwrap();
    // Invalidation clears the runtime's drained flag separately. Compare the
    // entire adapter row to prove the INSERT and its loss obligation rolled back.
    assert_eq!(durable_after[4], durable_before[4]);
    assert!(after.tracking_stopped && after.binding == before.binding);
    assert!(after.entities == before.entities && after.inbox == before.inbox);
    assert!(after.root == before.root && after.serialization == before.serialization);
    assert_eq!(rows(&host), before_rows);
    assert_eq!(runtime.adapter_fetch_drains(), drains_before);
    assert!(logged(runtime) == absence_before);
    assert_eq!(after.absence_pass_completed, before.absence_pass_completed);
    assert!(!runtime.sdk_root_confirmed());
    assert_eq!(
        runtime
            .trace()
            .iter()
            .filter(|e| matches!(e, TraceEvent::RootConfirmed { .. }))
            .count(),
        confirmations_before
    );
    assert!(service.request_log().unwrap() == requests_before);
    // accepted is diagnostic validation recorded before the failing transaction.
    let diagnostic = runtime.adapter_root_read_diagnostics().pop().unwrap();
    assert!(diagnostic.accepted && !diagnostic.active && !diagnostic.lost_collection);
    assert_eq!(
        diagnostic.outcome,
        if zone_missing { "zoneNotFound" } else { "unknownItem" }
    );
    runtime.retire_sdk(&sdk, false).unwrap();
    runtime.release_sdk(&sdk).unwrap();
    drop(owner_cleanup);
    drop(collection);
    host.close().unwrap();
    drop(host);

    let host = actual::open_actual(&directory, Arc::new(Account));
    let owner_cleanup = OwnerCleanup(&host);
    let runtime = host.cloud_runtime().unwrap();
    let reopened = runtime.adapter_snapshot().unwrap();
    let (sdk, restored, restored_zone) = admit(&host);
    assert_eq!(restored, before.serialization);
    assert_eq!(restored_zone, zone);
    let recovery_drains = runtime.adapter_fetch_drains();
    // No account reset, state deletion, reseed, zone callbacks or records on
    // reopen. Answer only the reads these unchanged owned fetches actually issue.
    for target in ["meguri-root", zone.as_str()] {
        if runtime.validate_sdk(&sdk).is_err() {
            break;
        }
        let fetch = runtime.adapter_begin_fetch(&sdk, vec![target.into()]).unwrap();
        let group = format!("meguri-{fetch}");
        runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
        let did = runtime.adapter_did_fetch(&sdk, &group, true);
        if runtime.validate_sdk(&sdk).is_err() {
            break;
        }
        did.unwrap();
        let reads = runtime
            .adapter_fetch_reads(&sdk, fetch, &host.collection().unwrap())
            .unwrap();
        for read in reads {
            let result = actual::read_control(
                &mut service,
                read.zone(),
                if read.is_active() { "active" } else { "epoch" },
            );
            if let Some(active) = runtime
                .adapter_complete_fetch_read(&read, &host.collection().unwrap(), result, false, None, None)
                .unwrap()
            {
                assert!(active.is_active());
                let result = actual::read_control(&mut service, active.zone(), "active");
                assert!(
                    runtime
                        .adapter_complete_fetch_read(&active, &host.collection().unwrap(), result, false, None, None)
                        .unwrap()
                        .is_none()
                );
            }
        }
        if runtime.validate_sdk(&sdk).is_ok() {
            let _ = runtime.adapter_finish_fetch(&sdk, fetch).unwrap();
        }
    }
    let confirmed = runtime.sdk_root_confirmed();
    let confirmations = runtime
        .trace()
        .iter()
        .filter(|e| matches!(e, TraceEvent::RootConfirmed { .. }))
        .count();
    let drains = runtime.adapter_fetch_drains();
    let batch = runtime
        .build_collection_batch(&sdk, &host.collection().unwrap(), &actual::names(runtime))
        .unwrap();
    let after = runtime.adapter_snapshot().unwrap();
    let after_rows = rows(&host);
    let absence = logged(runtime);
    let requests = service.request_log().unwrap();
    let binding_state = runtime.status().unwrap().binding.unwrap().facts.state;
    // Preserve all observations, then release the real owner before asserting
    // the recovery witness, including against unsafe existing behavior.
    runtime.retire_sdk(&sdk, false).unwrap();
    runtime.release_sdk(&sdk).unwrap();
    drop(owner_cleanup);
    host.close().unwrap();
    assert!(reopened.serialization == before.serialization && after.serialization == before.serialization);
    assert!(reopened.binding == before.binding && after.binding == before.binding);
    assert!(reopened.root == before.root && after.root == before.root);
    for pending in before
        .entities
        .iter()
        .filter(|e| e.mark.is_some() && e.local_rev > e.acked_rev)
    {
        for snapshot in [&reopened, &after] {
            let retained = snapshot.entities.iter().find(|e| e.name == pending.name).unwrap();
            assert!(
                retained.local_rev == pending.local_rev
                    && retained.acked_rev == pending.acked_rev
                    && retained.mark == pending.mark
                    && retained.payload == pending.payload
                    && retained.base_payload == pending.base_payload
                    && retained.acked_payload == pending.acked_payload
                    && retained.acked_system_fields == pending.acked_system_fields
                    && retained.acked_envelope == pending.acked_envelope
                    && retained.acked_row_digest == pending.acked_row_digest
                    && retained.acked_payload_digest == pending.acked_payload_digest
                    && retained.server_erev == pending.server_erev
                    && retained.in_flight == pending.in_flight,
                "reopen must preserve pending revisions, payloads, ACKs and flights"
            );
        }
    }
    assert_eq!(after_rows, before_rows);
    assert!(
        after
            .inbox
            .iter()
            .all(|entry| !matches!(entry.value, InboxValue::Deletion))
    );
    assert!(absence.is_empty());
    assert_eq!(
        binding_state,
        cloud::BindingState::Isolated(cloud::IsolationReason::ZoneMissing)
    );
    assert!(!after.absence_pass_completed);
    let durable_after: serde_json::Value = serde_json::from_slice(&after.tracking_content).unwrap();
    assert!(durable_after[4]["absence"].is_null());
    assert!(
        requests[requests_before.len()..]
            .iter()
            .all(|r| r.kind == meguri_testkit::cloud::RequestKind::Read)
    );
    assert!(
        !confirmed && confirmations == 0,
        "failed loss persistence must not let saved Active confirm on reopen"
    );
    assert_eq!(recovery_drains, drains_before);
    assert_eq!(
        drains, recovery_drains,
        "unchanged recovery callbacks must remain fenced from drains"
    );
    assert_eq!(batch.id, 0, "recovery must keep collection sends fenced");
    assert!(batch.saves.is_empty());
    // If saved-state callbacks request no epoch read, there is no owned
    // FetchRead available for another attempt. Leave loss, isolation and SDK state unchanged.
}

#[path = "cloud_native_absence/live_resume.rs"]
mod live_resume;

// K12.5 (another c- zone), K10.3 zones, K13.1.2-.3, P10.3/P24, Q11.
// Reuse the native adapter baseline and real retained-zone fixture. No seeded
// deletion outcome: each cause is delivered by the existing Service database log.
fn fetched_database_changes(service: &mut Service<SeededRandom>, mut token: Option<Token>) -> (Vec<ZoneChange>, Token) {
    let mut changes = Vec::new();
    loop {
        let response = service
            .request(&scope(), Caller::default(), Operation::FetchDatabase { token })
            .unwrap();
        let Ok(Reply::DatabaseChanges(page)) = response.result else {
            panic!("actual database changes required")
        };
        changes.extend(page.changes);
        if !page.more {
            return (changes, page.token);
        }
        token = Some(page.token);
    }
}

#[test]
fn fetched_other_epoch_deletions_remove_due_rows_for_every_cause_and_survive_reopen() {
    use meguri_testkit::cloud::RemovalCause;
    let mut rig = fixture(1, true);
    let runtime = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    let deck = meguri_engine::decks::add(&collection, "Pending zone-loss edit".into())
        .unwrap()
        .value
        .id;
    let media = rig.host.media().unwrap();
    runtime.reconcile_binding(&collection, &media, BUDGET).unwrap();
    let local = rows(&rig.host);
    let pending = runtime.tracking_sendables();
    assert!(!pending.is_empty());
    let zones = runtime.prepare_zone_deletion_fixture_for_testing(&rig.sdk).unwrap();
    let retained = runtime.publication_control_for_testing().unwrap();
    assert!(retained.zones.values().all(|z| z.deletion_due));
    let before = runtime.adapter_snapshot().unwrap();
    assert!(matches!(
        rig.service
            .request(
                &scope(),
                Caller::default(),
                Operation::Configure(Config {
                    page_size: 1,
                    ..Config::default()
                })
            )
            .unwrap()
            .result,
        Ok(Reply::Done)
    ));
    // Finish the initial inventory before any deletion. Its actual terminal
    // token is the watermark for history, including across incremental pages.
    let (_, mut token) = fetched_database_changes(&mut rig.service, None);
    let mut observed_zones = Vec::new();
    for (zone, cause) in zones.iter().zip([
        RemovalCause::Deleted,
        RemovalCause::Purged,
        RemovalCause::EncryptedDataReset,
    ]) {
        for operation in [
            Operation::CreateZone { zone: zone.clone() },
            Operation::DeleteZone {
                zone: zone.clone(),
                cause,
            },
        ] {
            assert!(matches!(
                rig.service
                    .request(&scope(), Caller::default(), operation)
                    .unwrap()
                    .result,
                Ok(Reply::Done)
            ));
        }
        let (changes, terminal) = fetched_database_changes(&mut rig.service, Some(token));
        token = terminal;
        assert!(
            changes
                .iter()
                .any(|change| change.zone == *zone && change.removed == Some(cause))
        );
        let deletion = changes
            .iter()
            .find(|change| change.removed.is_some())
            .expect("actual database deletion required");
        assert_eq!(deletion.zone, *zone);
        assert!(deletion.removed == Some(cause));
        runtime
            .adapter_fetched_collection_zone_deletions(&rig.sdk, &[deletion.zone.clone(), deletion.zone.clone()])
            .unwrap();
        runtime
            .adapter_fetched_collection_zone_deletions(&rig.sdk, &[deletion.zone.clone()])
            .unwrap();
        observed_zones.push(deletion.zone.clone());
        let after = runtime.publication_control_for_testing().unwrap();
        assert!(!after.zones.contains_key(zone));
        assert_eq!(after.current, retained.current);
        assert_eq!(runtime.tracking_sendables(), pending);
        assert_eq!(
            runtime.adapter_snapshot().unwrap().tracking_content,
            before.tracking_content
        );
        runtime.validate_sdk(&rig.sdk).unwrap();
    }
    assert_eq!(rows(&rig.host), local);
    assert!(runtime.publication_control_for_testing().unwrap().zones.is_empty());
    runtime.adapter_state_update(&rig.sdk, b"after-zone-deletions").unwrap();
    let durable = runtime.adapter_snapshot().unwrap();
    drop(media);
    drop(collection);
    rig.host.close().unwrap();
    let reopened = open(&rig.root.path().join("device"));
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.adapter_snapshot().unwrap().entities, durable.entities);
    // Check raw persistence before admission reconciles binding and refreshes observer stamps.
    let (sdk, state, _) = admit(&reopened);
    assert_eq!(state.as_deref(), Some(b"after-zone-deletions".as_slice()));
    assert!(runtime.publication_control_for_testing().unwrap().zones.is_empty());
    assert_eq!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        cloud::BindingState::Active
    );
    assert_eq!(rows(&reopened), local);
    let name = codec::entity_name(&[114; 32], codec::Kind::Deck, &codec::id_key(deck.0)).unwrap();
    let entity = runtime.entity_facts(&name).unwrap().unwrap();
    assert!(entity.mark.is_some());
    assert_eq!(observed_zones, zones);
    runtime
        .adapter_fetched_collection_zone_deletions(&sdk, &observed_zones)
        .unwrap();
    reopened.close().unwrap();
}

#[test]
fn fetched_other_epoch_removed_zone_rejects_stale_listing_then_tracks_recreated_zone() {
    use meguri_testkit::cloud::RemovalCause;
    let mut rig = fixture(1, true);
    let runtime = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    meguri_engine::decks::add(&collection, "Pending held-inventory edit".into()).unwrap();
    runtime
        .reconcile_binding(&collection, &rig.host.media().unwrap(), BUDGET)
        .unwrap();
    let local = rows(&rig.host);
    let pending = runtime.tracking_sendables();
    assert!(!pending.is_empty());
    let zones = runtime.prepare_zone_deletion_fixture_for_testing(&rig.sdk).unwrap();
    let retained = runtime.publication_control_for_testing().unwrap();
    assert!(retained.zones.values().all(|z| z.deletion_due));
    let before = runtime.adapter_snapshot().unwrap();
    assert!(matches!(
        rig.service
            .request(
                &scope(),
                Caller::default(),
                Operation::Configure(Config {
                    page_size: 1,
                    ..Config::default()
                })
            )
            .unwrap()
            .result,
        Ok(Reply::Done)
    ));
    for zone in &zones {
        assert!(matches!(
            rig.service
                .request(
                    &scope(),
                    Caller::default(),
                    Operation::CreateZone { zone: zone.clone() }
                )
                .unwrap()
                .result,
            Ok(Reply::Done)
        ));
    }
    let zone = &zones[0];
    let held_request = runtime.begin_zone_listing(&rig.sdk, &collection).unwrap();
    let (inventory, token) = fetched_database_changes(&mut rig.service, None);
    assert!(inventory.iter().all(|change| change.removed.is_none()));
    let held_names: Vec<_> = inventory.into_iter().map(|change| change.zone).collect();
    assert!(held_names.contains(zone) && held_names.contains(&rig.zone));
    assert!(matches!(
        rig.service
            .request(
                &scope(),
                Caller::default(),
                Operation::DeleteZone {
                    zone: zone.clone(),
                    cause: RemovalCause::Deleted
                }
            )
            .unwrap()
            .result,
        Ok(Reply::Done)
    ));
    let (changes, _) = fetched_database_changes(&mut rig.service, Some(token));
    let deletion = changes
        .iter()
        .find(|change| change.removed.is_some())
        .expect("actual database deletion required");
    assert_eq!(&deletion.zone, zone);
    assert!(deletion.removed == Some(RemovalCause::Deleted));
    runtime
        .adapter_fetched_collection_zone_deletions(&rig.sdk, &[deletion.zone.clone()])
        .unwrap();
    let mut remaining = retained.zones.clone();
    assert!(remaining.remove(zone).is_some());
    assert_eq!(runtime.publication_control_for_testing().unwrap().zones, remaining);
    assert!(
        !runtime
            .complete_zone_listing(held_request, &collection, Some(&held_names))
            .unwrap()
    );
    assert_eq!(runtime.publication_control_for_testing().unwrap().zones, remaining);
    assert_eq!(
        runtime.publication_control_for_testing().unwrap().current,
        retained.current
    );
    assert_eq!(runtime.tracking_sendables(), pending);
    assert_eq!(
        runtime.adapter_snapshot().unwrap().tracking_content,
        before.tracking_content
    );
    assert_eq!(rows(&rig.host), local);
    runtime.validate_sdk(&rig.sdk).unwrap();

    assert!(matches!(
        rig.service
            .request(
                &scope(),
                Caller::default(),
                Operation::CreateZone { zone: zone.clone() }
            )
            .unwrap()
            .result,
        Ok(Reply::Done)
    ));
    let fresh_request = runtime.begin_zone_listing(&rig.sdk, &collection).unwrap();
    let (inventory, _) = fetched_database_changes(&mut rig.service, None);
    assert!(inventory.iter().all(|change| change.removed.is_none()));
    let fresh_names: Vec<_> = inventory.into_iter().map(|change| change.zone).collect();
    assert!(fresh_names.contains(zone) && fresh_names.contains(&rig.zone));
    assert!(
        runtime
            .complete_zone_listing(fresh_request, &collection, Some(&fresh_names))
            .unwrap()
    );
    let after = runtime.publication_control_for_testing().unwrap();
    assert!(after.zones.contains_key(zone));
    for (name, row) in remaining {
        assert_eq!(after.zones[&name], row);
    }
    assert_eq!(after.current, retained.current);
    assert_eq!(runtime.tracking_sendables(), pending);
    assert_eq!(
        runtime.adapter_snapshot().unwrap().tracking_content,
        before.tracking_content
    );
    assert_eq!(rows(&rig.host), local);
    runtime.validate_sdk(&rig.sdk).unwrap();
    drop(collection);
    rig.host.close().unwrap();
}

#[test]
fn fetched_other_epoch_deletion_refuses_mixed_bound_reserved_and_malformed_without_partial_mutation() {
    let malformed = [
        "c-",
        "c-current",
        "c-aaaaaaaaaaaaaaaaaaaaaaaaab",
        "c-AAAAAAAAAAAAAAAAAAAAAAAAAA",
        "c-aaaaaaaaaaaaaaaaaaaaaaaaaa=",
        "c-aaaaaaaaaaaaaaaaaaaaaaaaa",
        "unrelated-zone",
    ];
    for refused in ["meguri-root", "meguri-media"]
        .into_iter()
        .chain(malformed)
        .chain(["BOUND"])
    {
        let rig = fixture(1, true);
        let runtime = rig.host.cloud_runtime().unwrap();
        let zones = runtime.prepare_zone_deletion_fixture_for_testing(&rig.sdk).unwrap();
        let invalid = if refused == "BOUND" { rig.zone.as_str() } else { refused };
        let before = runtime.adapter_snapshot().unwrap();
        let retained = runtime.publication_control_for_testing().unwrap().zones;
        assert!(
            runtime
                .adapter_fetched_collection_zone_deletions(
                    &rig.sdk,
                    &[zones[0].clone(), invalid.into(), zones[1].clone()]
                )
                .is_err()
        );
        assert_eq!(runtime.publication_control_for_testing().unwrap().zones, retained);
        assert_eq!(
            runtime.adapter_snapshot().unwrap().tracking_content,
            before.tracking_content
        );
        assert!(runtime.adapter_state_update(&rig.sdk, b"must-not-advance").is_err());
        assert_eq!(runtime.adapter_snapshot().unwrap().serialization, before.serialization);
        rig.host.close().unwrap();
    }
}

#[test]
fn fetched_other_epoch_deletion_transaction_failure_rolls_back_and_closes_only_original_intake() {
    use cloud::testing::{TrackingFault, arm_tracking_fault};
    for fault in [TrackingFault::Io, TrackingFault::Constraint, TrackingFault::Panic] {
        let rig = fixture(1, true);
        let runtime = rig.host.cloud_runtime().unwrap();
        let zones = runtime.prepare_zone_deletion_fixture_for_testing(&rig.sdk).unwrap();
        let before = runtime.adapter_snapshot().unwrap();
        let retained = runtime.publication_control_for_testing().unwrap().zones;
        arm_tracking_fault("cloud.tracking.commit.inbox", fault).unwrap();
        assert!(
            runtime
                .adapter_fetched_collection_zone_deletions(&rig.sdk, &zones)
                .is_err()
        );
        assert_eq!(runtime.publication_control_for_testing().unwrap().zones, retained);
        assert_eq!(
            runtime.adapter_snapshot().unwrap().tracking_content,
            before.tracking_content
        );
        assert!(runtime.adapter_snapshot().unwrap().tracking_stopped);
        assert!(runtime.adapter_state_update(&rig.sdk, b"must-not-advance").is_err());
        assert_eq!(runtime.adapter_snapshot().unwrap().serialization, before.serialization);
        rig.host.close().unwrap();
        let reopened = open(&rig.root.path().join("device"));
        let (_, state, _) = admit(&reopened);
        let runtime = reopened.cloud_runtime().unwrap();
        assert_eq!(state, before.serialization);
        assert_eq!(runtime.publication_control_for_testing().unwrap().zones, retained);
        reopened.close().unwrap();
    }
}

#[test]
fn fetched_other_epoch_deletion_from_stale_sdk_cannot_mutate_or_invalidate_successor() {
    let rig = fixture(1, true);
    let runtime = rig.host.cloud_runtime().unwrap();
    let zones = runtime.prepare_zone_deletion_fixture_for_testing(&rig.sdk).unwrap();
    runtime.retire_sdk(&rig.sdk, false).unwrap();
    runtime.release_sdk(&rig.sdk).unwrap();
    let (sdk, _, _) = runtime.register_sdk(false).unwrap();
    let before = runtime.adapter_snapshot().unwrap();
    let retained = runtime.publication_control_for_testing().unwrap().zones;
    assert!(matches!(
        runtime.adapter_fetched_collection_zone_deletions(&rig.sdk, &zones),
        Err(cloud::runtime::RuntimeError::StaleIncarnation)
    ));
    assert_eq!(
        runtime.adapter_snapshot().unwrap().tracking_content,
        before.tracking_content
    );
    assert_eq!(runtime.publication_control_for_testing().unwrap().zones, retained);
    runtime.validate_sdk(&sdk).unwrap();
    runtime.adapter_state_update(&sdk, b"successor-state").unwrap();
    runtime.adapter_fetched_collection_zone_deletions(&sdk, &zones).unwrap();
    assert!(runtime.publication_control_for_testing().unwrap().zones.is_empty());
    rig.host.close().unwrap();
}
