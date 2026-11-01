//! Real stdio process tests for the synthetic Service transport only.
use meguri_model::rng::SeededRandom;
use meguri_testkit::cloud::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const FRAME_BYTES: usize = 1_048_576;
const SENTINEL: &str = "synthetic-body-must-not-appear-in-errors";

fn scope() -> Scope {
    Scope {
        container: "fixture-container".into(),
        environment: "development".into(),
        account: "fixture-account".into(),
    }
}
fn caller() -> Caller {
    Caller {
        device: Some("fixture-device".into()),
        instance: Some("fixture-instance".into()),
        own_operation: Some("fixture-operation".into()),
    }
}
fn envelope(operation: Operation) -> Value {
    json!({"scope": scope(), "caller": caller(), "operation": operation})
}
fn command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_meguri-cli"));
    command.arg("internal-cloud-service");
    command
}
struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}
impl Client {
    fn open(directory: &Path) -> Self {
        let mut child = command()
            .arg(directory)
            .arg("18446744073709551615")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
        }
    }
    fn write(&mut self, bytes: &[u8]) {
        let input = self.input.as_mut().unwrap();
        input.write_all(bytes).unwrap();
        input.flush().unwrap();
    }
    fn line(&mut self) -> String {
        let mut line = String::new();
        assert!(self.output.read_line(&mut line).unwrap() > 0);
        assert!(line.ends_with('\n'));
        line
    }
    fn response(&mut self, operation: Operation) -> Response {
        let mut bytes = serde_json::to_vec(&envelope(operation)).unwrap();
        bytes.push(b'\n');
        self.write(&bytes);
        serde_json::from_str(&self.line()).unwrap()
    }
    fn call(&mut self, operation: Operation) -> Reply {
        self.response(operation).result.unwrap()
    }
    fn create(&mut self, zone: &str) {
        assert_eq!(self.call(Operation::CreateZone { zone: zone.into() }), Reply::Done);
    }
    fn modify(&mut self, atomic: bool, changes: Vec<Mutation>) -> Vec<MutationResult> {
        match self.call(Operation::Modify {
            zone: "z".into(),
            atomic,
            changes,
        }) {
            Reply::Modified(results) => results,
            _ => panic!("expected modify"),
        }
    }
    fn read(&mut self, name: &str) -> Record {
        match self.call(read_operation(name)) {
            Reply::Record(record) => record,
            _ => panic!("expected record"),
        }
    }
    fn page(&mut self, token: Option<Token>) -> Page<RecordChange> {
        match self.call(Operation::FetchZone {
            zone: "z".into(),
            token,
        }) {
            Reply::ZoneChanges(page) => page,
            _ => panic!("expected zone page"),
        }
    }
    fn finish(&mut self, code: i32) -> String {
        drop(self.input.take());
        let mut trailing = String::new();
        self.output.read_to_string(&mut trailing).unwrap();
        assert!(trailing.is_empty(), "unexpected trailing output");
        assert_eq!(self.child.wait().unwrap().code(), Some(code));
        let mut stderr = String::new();
        self.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        stderr
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn read_operation(name: &str) -> Operation {
    Operation::Read {
        zone: "z".into(),
        name: name.into(),
        fields: RequestedFields::All,
    }
}
fn save(record: RecordInput, if_unchanged: Option<SystemFields>) -> Mutation {
    Mutation::Save { record, if_unchanged }
}
fn saved(result: &MutationResult) -> Record {
    match result.result.as_ref().unwrap() {
        MutationSuccess::Saved(record) => record.clone(),
        _ => panic!("expected saved"),
    }
}
fn input(name: &str, e: &[u8]) -> RecordInput {
    RecordInput::encrypted(name, "MGEntity", e.to_vec())
}
fn matches_input(actual: &Record, sent: &RecordInput) {
    assert_eq!(actual.name, sent.name);
    assert_eq!(actual.record_type, sent.record_type);
    assert_eq!(actual.fields, sent.fields);
    assert!(!actual.system_fields.bytes().is_empty());
}

#[test]
fn retained_service_preserves_payloads_conflicts_atomicity_and_reopens_after_eof() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::open(directory.path());
    client.create("z");
    let mut entity = input("entity", &[0, 255, 128, 10, 34, 92]);
    entity
        .fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(vec![255, 0, 128, 10])));
    let mut blob = RecordInput::encrypted("blob", "MGBlob", vec![13, 0, 254]);
    blob.fields
        .insert("a".into(), Field::Asset(Asset::synthetic(vec![0, 255, 13, 92, 34])));
    let results = client.modify(true, vec![save(entity.clone(), None), save(blob.clone(), None)]);
    let first = saved(&results[0]);
    let saved_blob = saved(&results[1]);
    matches_input(&first, &entity);
    matches_input(&saved_blob, &blob);
    assert_eq!(client.read("entity"), first);
    assert_eq!(client.read("blob"), saved_blob);

    let mut changed = entity.clone();
    changed
        .fields
        .insert("e".into(), Field::EncryptedBytes(vec![7, 0, 255]));
    let updated = saved(&client.modify(true, vec![save(changed.clone(), Some(first.system_fields.clone()))])[0]);
    matches_input(&updated, &changed);
    assert_ne!(updated.system_fields, first.system_fields);
    let conflict = client.modify(true, vec![save(entity.clone(), Some(first.system_fields.clone()))]);
    let error = conflict[0].result.as_ref().unwrap_err();
    assert_eq!(error.code, ErrorCode::ServerRecordChanged);
    assert_eq!(error.server_record.as_deref(), Some(&updated));

    // The atomic flag must reach Service unchanged: a sibling delete rolls back.
    let results = client.modify(
        true,
        vec![
            Mutation::Delete { name: "blob".into() },
            save(entity.clone(), Some(first.system_fields.clone())),
        ],
    );
    assert_eq!(
        results[0].result.as_ref().unwrap_err().code,
        ErrorCode::BatchRequestFailed
    );
    assert_eq!(
        results[1].result.as_ref().unwrap_err().code,
        ErrorCode::ServerRecordChanged
    );
    assert_eq!(client.read("blob"), saved_blob);
    let results = client.modify(
        false,
        vec![
            Mutation::Delete { name: "blob".into() },
            save(entity, Some(first.system_fields)),
        ],
    );
    assert_eq!(results[0].result, Ok(MutationSuccess::Deleted));
    assert_eq!(
        results[1].result.as_ref().unwrap_err().code,
        ErrorCode::ServerRecordChanged
    );
    assert_eq!(
        client.response(read_operation("blob")).result.unwrap_err().code,
        ErrorCode::UnknownItem
    );
    assert_eq!(client.read("entity"), updated);
    assert!(client.finish(0).is_empty());

    let mut reopened = Client::open(directory.path());
    assert_eq!(reopened.read("entity"), updated);
    assert_eq!(
        reopened.response(read_operation("blob")).result.unwrap_err().code,
        ErrorCode::UnknownItem
    );
    assert!(reopened.finish(0).is_empty());
    let service = Service::open(directory.path(), Config::default(), SeededRandom::new(3)).unwrap();
    let logs = service.request_log().unwrap();
    assert!(logs.windows(2).all(|pair| pair[0].request_id < pair[1].request_id));
    assert!(logs.iter().all(|log| log.scope == scope() && log.caller == caller()));
}

#[test]
fn default_zone_pagination_survives_intervening_requests_replay_and_process_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::open(directory.path());
    client.create("z");
    let count = Config::default().page_size + 1;
    let sent: Vec<_> = (0..count).map(|i| input(&format!("r{i:04}"), &[0, 255, 17])).collect();
    let results = client.modify(true, sent.iter().cloned().map(|r| save(r, None)).collect());
    assert_eq!(results.len(), count);
    let expected: Vec<_> = results.iter().map(saved).collect();
    for (record, sent) in expected.iter().zip(&sent) {
        matches_input(record, sent);
    }
    let first = client.page(None);
    assert!(first.more);
    assert_eq!(
        first.changes,
        expected[..count - 1]
            .iter()
            .cloned()
            .map(RecordChange::Saved)
            .collect::<Vec<_>>()
    );

    let last = &expected[count - 1];
    let replacement = input(&last.name, &[99, 0, 128]);
    let updated = saved(&client.modify(true, vec![save(replacement.clone(), Some(last.system_fields.clone()))])[0]);
    matches_input(&updated, &replacement);
    assert_eq!(client.read(&last.name), updated);
    assert!(client.finish(0).is_empty());

    let mut client = Client::open(directory.path());
    let second = client.page(Some(first.token.clone()));
    assert!(!second.more);
    assert_eq!(second.changes, vec![RecordChange::Saved(last.clone())]);
    let replay = client.page(Some(first.token));
    assert_eq!(replay, second);
    let delta = client.page(Some(second.token));
    assert!(!delta.more);
    assert_eq!(delta.changes, vec![RecordChange::Saved(updated)]);
    assert!(client.finish(0).is_empty());
}

#[test]
fn default_database_pagination_roundtrips_tokens_over_other_requests() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::open(directory.path());
    let count = Config::default().page_size + 1;
    let zones: Vec<_> = (0..count).map(|i| format!("z{i:04}")).collect();
    for zone in &zones {
        client.create(zone);
    }
    let Reply::DatabaseChanges(first) = client.call(Operation::FetchDatabase { token: None }) else {
        panic!("expected database page");
    };
    assert!(first.more);
    assert_eq!(
        first.changes,
        zones[..count - 1]
            .iter()
            .map(|zone| ZoneChange {
                zone: zone.clone(),
                removed: None,
            })
            .collect::<Vec<_>>()
    );
    client.create("later");
    let Reply::DatabaseChanges(second) = client.call(Operation::FetchDatabase {
        token: Some(first.token),
    }) else {
        panic!("expected database continuation");
    };
    assert!(!second.more);
    assert_eq!(
        second.changes,
        vec![ZoneChange {
            zone: zones[count - 1].clone(),
            removed: None,
        }]
    );
    let Reply::DatabaseChanges(delta) = client.call(Operation::FetchDatabase {
        token: Some(second.token),
    }) else {
        panic!("expected database delta");
    };
    assert!(!delta.more);
    assert_eq!(
        delta.changes,
        vec![ZoneChange {
            zone: "later".into(),
            removed: None,
        }]
    );
    assert!(client.finish(0).is_empty());
}

#[test]
fn refused_frames_do_not_dispatch_mutate_or_echo_bodies_and_cap_is_checked_before_lf() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::open(directory.path());
    client.create("z");
    let baseline = saved(&client.modify(true, vec![save(input("keep", &[9, 0, 255]), None)])[0]);
    assert!(client.finish(0).is_empty());

    let attempted_save = || {
        envelope(Operation::Modify {
            zone: "z".into(),
            atomic: true,
            changes: vec![save(
                input("keep", SENTINEL.as_bytes()),
                Some(baseline.system_fields.clone()),
            )],
        })
    };
    let mut unknown_field = attempted_save();
    unknown_field["credential"] = json!(SENTINEL);
    let mut unknown_scope = attempted_save();
    unknown_scope["scope"]["credential"] = json!(SENTINEL);
    let mut unknown_caller = attempted_save();
    unknown_caller["caller"]["credential"] = json!(SENTINEL);
    let mut missing_scope = attempted_save();
    missing_scope.as_object_mut().unwrap().remove("scope");
    let mut unknown_operation = attempted_save();
    unknown_operation["operation"] = json!({"NotAnOperation": SENTINEL});
    let mut cases: Vec<(Vec<u8>, &str)> = vec![
        (serde_json::to_vec(&unknown_field).unwrap(), "malformed-frame"),
        (serde_json::to_vec(&unknown_scope).unwrap(), "malformed-frame"),
        (serde_json::to_vec(&unknown_caller).unwrap(), "malformed-frame"),
        (serde_json::to_vec(&missing_scope).unwrap(), "malformed-frame"),
        (serde_json::to_vec(&unknown_operation).unwrap(), "malformed-frame"),
        (format!("{{\"{SENTINEL}\":").into_bytes(), "malformed-frame"),
        (b"[]".to_vec(), "malformed-frame"),
        (Vec::new(), "malformed-frame"),
    ];
    for operation in [
        Operation::Configure(Config::default()),
        Operation::SetQuota { bytes: Some(0) },
        Operation::DeleteZone {
            zone: "z".into(),
            cause: RemovalCause::Purged,
        },
        Operation::DeleteZone {
            zone: "z".into(),
            cause: RemovalCause::EncryptedDataReset,
        },
    ] {
        cases.push((
            serde_json::to_vec(&envelope(operation)).unwrap(),
            "unsupported-operation",
        ));
    }
    for (mut bytes, kind) in cases {
        let mut client = Client::open(directory.path());
        bytes.push(b'\n');
        client.write(&bytes);
        assert_eq!(client.line(), format!("{{\"protocol_error\":\"{kind}\"}}\n"));
        assert_eq!(client.finish(2), format!("cloud service protocol-error: {kind}\n"));
    }
    let mut client = Client::open(directory.path());
    // No LF and no EOF: the reader must reject without waiting for either.
    let mut oversized = vec![b' '; FRAME_BYTES + 1];
    oversized[..SENTINEL.len()].copy_from_slice(SENTINEL.as_bytes());
    client.write(&oversized);
    assert_eq!(client.line(), "{\"protocol_error\":\"frame-too-large\"}\n");
    assert_eq!(client.finish(2), "cloud service protocol-error: frame-too-large\n");

    let mut client = Client::open(directory.path());
    client.write(SENTINEL.as_bytes());
    drop(client.input.take());
    assert_eq!(client.line(), "{\"protocol_error\":\"truncated-frame\"}\n");
    assert_eq!(client.finish(2), "cloud service protocol-error: truncated-frame\n");

    // None of the refusals even reached Service's durable request log.
    let service = Service::open(directory.path(), Config::default(), SeededRandom::new(3)).unwrap();
    assert_eq!(service.request_log().unwrap().len(), 2);
    drop(service);
    let mut client = Client::open(directory.path());
    // The exact cap is accepted; the LF is outside the cap.
    let mut boundary = serde_json::to_vec(&envelope(read_operation("keep"))).unwrap();
    boundary.resize(FRAME_BYTES, b' ');
    boundary.push(b'\n');
    client.write(&boundary);
    let response: Response = serde_json::from_str(&client.line()).unwrap();
    assert_eq!(response.result, Ok(Reply::Record(baseline)));
    assert!(client.finish(0).is_empty());
}

#[test]
fn startup_errors_are_classified_without_paths_or_input_and_command_is_internal() {
    let directory = tempfile::tempdir().unwrap();
    for seed in ["", "-1", "+1", "1.0", "18446744073709551616", SENTINEL] {
        let output = command().arg(directory.path()).arg(seed).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr.as_slice(), b"cloud service startup-error: arguments\n");
    }
    for args in [vec![], vec![SENTINEL], vec![SENTINEL, "1", SENTINEL]] {
        let output = command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr.as_slice(), b"cloud service startup-error: arguments\n");
    }
    let output = command()
        .arg(directory.path().join(SENTINEL))
        .arg("1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr.as_slice(), b"cloud service startup-error: storage\n");
    let output = Command::new(env!("CARGO_BIN_EXE_meguri-cli")).output().unwrap();
    assert!(
        !String::from_utf8(output.stderr)
            .unwrap()
            .contains("internal-cloud-service")
    );
}

#[test]
fn stdio_all_zone_inventory_returns_original_complete_service_response() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::open(directory.path());
    client.create("z");
    client.create("a");
    let scoped_response = |client: &mut Client, scope: &Scope, operation: Operation| -> Response {
        let mut bytes =
            serde_json::to_vec(&json!({"scope": scope, "caller": caller(), "operation": operation})).unwrap();
        bytes.push(b'\n');
        client.write(&bytes);
        serde_json::from_str(&client.line()).unwrap()
    };
    let mut other = scope();
    other.account = "fixture-other-account".into();
    let created = scoped_response(&mut client, &other, Operation::CreateZone { zone: "z".into() });
    assert_eq!(created.result, Ok(Reply::Done));
    let original = client.response(Operation::FetchAllZones);
    assert_eq!(original.result, Ok(Reply::AllZones(vec!["a".into(), "z".into()])));
    let deleted = client.response(Operation::DeleteZone {
        zone: "z".into(),
        cause: RemovalCause::Deleted,
    });
    assert_eq!(deleted.result, Ok(Reply::Done));
    let absent = client.response(Operation::FetchAllZones);
    assert_eq!(absent.result, Ok(Reply::AllZones(vec!["a".into()])));
    let foreign = scoped_response(&mut client, &other, Operation::FetchAllZones);
    assert_eq!(foreign.result, Ok(Reply::AllZones(vec!["z".into()])));
    let recreated = client.response(Operation::CreateZone { zone: "z".into() });
    assert_eq!(recreated.result, Ok(Reply::Done));
    let fresh = client.response(Operation::FetchAllZones);
    assert_eq!(fresh.result, Ok(Reply::AllZones(vec!["a".into(), "z".into()])));
    assert!(client.finish(0).is_empty());

    let service = Service::open(directory.path(), Config::default(), SeededRandom::new(3)).unwrap();
    let logs = service.request_log().unwrap();
    let outcomes = service.outcomes().unwrap();
    assert_eq!(logs.len(), 9);
    assert_eq!(outcomes.len(), logs.len());
    assert!(logs.windows(2).all(|pair| pair[0].request_id < pair[1].request_id));
    for (response, scope, kind, zone, expected) in [
        (
            created,
            other.clone(),
            RequestKind::CreateZone,
            Some("z"),
            ReplyReference::Done,
        ),
        (
            original,
            scope(),
            RequestKind::FetchAllZones,
            None,
            ReplyReference::AllZones(vec!["a".into(), "z".into()]),
        ),
        (
            deleted,
            scope(),
            RequestKind::DeleteZone,
            Some("z"),
            ReplyReference::Done,
        ),
        (
            absent,
            scope(),
            RequestKind::FetchAllZones,
            None,
            ReplyReference::AllZones(vec!["a".into()]),
        ),
        (
            foreign,
            other,
            RequestKind::FetchAllZones,
            None,
            ReplyReference::AllZones(vec!["z".into()]),
        ),
        (
            recreated,
            scope(),
            RequestKind::CreateZone,
            Some("z"),
            ReplyReference::Done,
        ),
        (
            fresh,
            scope(),
            RequestKind::FetchAllZones,
            None,
            ReplyReference::AllZones(vec!["a".into(), "z".into()]),
        ),
    ] {
        assert!(response.request_id > 0);
        let entry = logs
            .iter()
            .find(|entry| entry.request_id == response.request_id)
            .unwrap();
        assert_eq!(entry.scope, scope);
        assert_eq!(entry.caller, caller());
        assert_eq!(entry.kind, kind);
        assert_eq!(entry.zone.as_deref(), zone);
        assert_eq!(entry.outcome, LogOutcome::Success);
        let outcome = outcomes
            .iter()
            .find(|entry| entry.request_id == response.request_id)
            .unwrap();
        assert_eq!(outcome.result, Ok(expected));
    }
}
