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
        RequestedFields, Scope, Service, Token,
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
