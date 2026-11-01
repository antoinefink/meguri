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

#[path = "support/sign_out_absence.rs"]
mod sign_out_absence;

const Q6_CHILD_ENV: &str = "MEGURI_Q6_RESUME_ABSENCE_CHILD";
const Q6_CHECKPOINT: &str = "Q6_RESUME_ABSENCE_CHECKPOINT ";

#[derive(serde::Serialize, serde::Deserialize)]
struct Q6LocalFacts {
    note_id: i64,
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
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cloud_q6_resume_absence_child_checkpoint",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(Q6_CHILD_ENV, root)
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
                if let Some((_, json)) = line.split_once(Q6_CHECKPOINT) {
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
    assert!(runtime.entity_facts_page(Kind::Note, None).unwrap().iter().any(
        |e| matches!(&e.payload, Payload::Note(n) if n.id == facts.note_id && n.flds == facts.edited_fields)
            && e.mark.is_some()
    ));
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
