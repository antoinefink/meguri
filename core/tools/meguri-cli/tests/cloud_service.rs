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
    binary: bool,
}
impl Client {
    fn open(directory: &Path) -> Self {
        Self::open_transport(directory, false)
    }
    fn open_binary(directory: &Path) -> Self {
        Self::open_transport(directory, true)
    }
    fn open_transport(directory: &Path, binary: bool) -> Self {
        Self::open_with_page_size(directory, binary, None, false)
    }
    fn open_with_page_size(directory: &Path, binary: bool, page_size: Option<usize>, page_size_first: bool) -> Self {
        let mut command = command();
        command.arg(directory).arg("18446744073709551615");
        if page_size_first && let Some(size) = page_size {
            command.arg("--page-size").arg(size.to_string());
        }
        if binary {
            command.arg("--binary-assets-v1");
        }
        if !page_size_first && let Some(size) = page_size {
            command.arg("--page-size").arg(size.to_string());
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        Self {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            binary,
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
        if self.binary {
            return self.binary_response(operation);
        }
        let mut bytes = serde_json::to_vec(&envelope(operation)).unwrap();
        bytes.push(b'\n');
        self.write(&bytes);
        serde_json::from_str(&self.line()).unwrap()
    }
    fn binary_response(&mut self, operation: Operation) -> Response {
        let mut message = envelope(operation);
        let mut bodies = Vec::new();
        if let Some(changes) = message
            .get_mut("operation")
            .and_then(|operation| operation.get_mut("Modify"))
            .and_then(|modify| modify.get_mut("changes"))
            .and_then(Value::as_array_mut)
        {
            for change in changes {
                if let Some(fields) = change
                    .get_mut("Save")
                    .and_then(|save| save.get_mut("record"))
                    .and_then(|record| record.get_mut("fields"))
                    .and_then(Value::as_object_mut)
                {
                    for field in fields.values_mut() {
                        if let Some(asset) = field.get_mut("Asset").and_then(Value::as_object_mut) {
                            let bytes = asset.get_mut("bytes").unwrap();
                            let body: Vec<u8> = serde_json::from_value(bytes.take()).unwrap();
                            *bytes = json!({"attachment": bodies.len()});
                            bodies.push(body);
                        }
                    }
                }
            }
        }
        let mut header = serde_json::to_vec(&json!({"version": 1,
            "lengths": bodies.iter().map(Vec::len).collect::<Vec<_>>(), "message": message}))
        .unwrap();
        assert!(header.len() <= FRAME_BYTES);
        header.push(b'\n');
        self.write(&header);
        for body in bodies {
            self.write(&body);
        }
        let line = self.line();
        assert!(line.len() <= FRAME_BYTES + 1);
        let mut header: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(header["version"], 1);
        let lengths: Vec<usize> = serde_json::from_value(header["lengths"].take()).unwrap();
        assert!(lengths.len() <= 16 && lengths.iter().sum::<usize>() <= 8 * FRAME_BYTES);
        let mut bodies: Vec<Option<Vec<u8>>> = lengths
            .into_iter()
            .map(|length| {
                assert!(length <= 4 * FRAME_BYTES);
                let mut body = vec![0; length];
                self.output.read_exact(&mut body).unwrap();
                Some(body)
            })
            .collect();
        fn restore(value: &mut Value, bodies: &mut [Option<Vec<u8>>]) {
            if let Some(index) = value.get("attachment").and_then(Value::as_u64) {
                assert_eq!(value.as_object().unwrap().len(), 1);
                *value = json!(bodies[index as usize].take().unwrap());
            } else {
                match value {
                    Value::Array(values) => {
                        for value in values {
                            restore(value, bodies);
                        }
                    }
                    Value::Object(values) => {
                        for value in values.values_mut() {
                            restore(value, bodies);
                        }
                    }
                    _ => {}
                }
            }
        }
        restore(&mut header["message"], &mut bodies);
        assert!(bodies.iter().all(Option::is_none));
        serde_json::from_value(header["message"].take()).unwrap()
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
fn explicit_page_size_preserves_original_zone_pages_tokens_and_reopen() {
    for binary in [false, true] {
        for page_size_first in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let mut client = Client::open_with_page_size(directory.path(), binary, Some(32), page_size_first);
            client.create("z");
            let sent: Vec<_> = (0..41)
                .map(|i| {
                    let mut record = input(&format!("r{i:04}"), &[i, 0, 255]);
                    // Keep real assets on both pages within the binary attachment budget.
                    if i == 0 || i == 40 {
                        record
                            .fields
                            .insert("pa".into(), Field::Asset(Asset::synthetic(vec![255, i, 10, 0])));
                    }
                    record
                })
                .collect();
            let expected: Vec<_> = client
                .modify(true, sent.iter().cloned().map(|r| save(r, None)).collect())
                .iter()
                .map(saved)
                .collect();
            assert_eq!(expected.len(), 41);
            for (record, sent) in expected.iter().zip(&sent) {
                matches_input(record, sent);
            }
            let first = client.page(None);
            assert!(first.more);
            assert_eq!(
                first.changes,
                expected[..32]
                    .iter()
                    .cloned()
                    .map(RecordChange::Saved)
                    .collect::<Vec<_>>()
            );
            let last = expected.last().unwrap();
            let mut replacement = sent.last().unwrap().clone();
            replacement
                .fields
                .insert("e".into(), Field::EncryptedBytes(vec![99, 128, 0]));
            let updated =
                saved(&client.modify(true, vec![save(replacement.clone(), Some(last.system_fields.clone()))])[0]);
            matches_input(&updated, &replacement);
            assert_eq!(client.read(&last.name), updated);
            assert!(client.finish(0).is_empty());

            // No new override: the persisted size and original snapshot survive.
            let mut client = Client::open_transport(directory.path(), binary);
            let second = client.page(Some(first.token.clone()));
            assert!(!second.more);
            assert_eq!(
                second.changes,
                expected[32..]
                    .iter()
                    .cloned()
                    .map(RecordChange::Saved)
                    .collect::<Vec<_>>()
            );
            assert_eq!(client.page(Some(first.token)), second);
            let delta = client.page(Some(second.token));
            assert!(!delta.more);
            assert_eq!(delta.changes, vec![RecordChange::Saved(updated.clone())]);
            // A fresh snapshot also proves reopening retained 32 rather than 200.
            let fresh = client.page(None);
            assert!(fresh.more);
            assert_eq!(fresh.changes.len(), 32);
            assert!(client.finish(0).is_empty());

            // A conflicting initial option also must not reconfigure this store.
            let mut client = Client::open_with_page_size(directory.path(), binary, Some(1), !page_size_first);
            let tail = client.page(Some(fresh.token));
            assert!(!tail.more);
            let mut current_tail = expected[32..].to_vec();
            *current_tail.last_mut().unwrap() = updated;
            assert_eq!(
                tail.changes,
                current_tail.into_iter().map(RecordChange::Saved).collect::<Vec<_>>()
            );
            let fresh = client.page(None);
            assert!(fresh.more);
            assert_eq!(fresh.changes.len(), 32);
            assert!(client.finish(0).is_empty());
        }
    }
}

#[test]
fn page_size_arguments_are_bounded_and_classified_before_storage() {
    for binary in [false, true] {
        for page_size_first in [false, true] {
            for size in [1, 200] {
                let directory = tempfile::tempdir().unwrap();
                let mut client = Client::open_with_page_size(directory.path(), binary, Some(size), page_size_first);
                client.create("z");
                let expected: Vec<_> = client
                    .modify(
                        true,
                        (0..=size)
                            .map(|i| save(input(&format!("r{i:04}"), &[0, 255]), None))
                            .collect(),
                    )
                    .iter()
                    .map(saved)
                    .collect();
                let first = client.page(None);
                assert!(first.more);
                assert_eq!(
                    first.changes,
                    expected[..size]
                        .iter()
                        .cloned()
                        .map(RecordChange::Saved)
                        .collect::<Vec<_>>()
                );
                let last = client.page(Some(first.token));
                assert!(!last.more);
                assert_eq!(last.changes, vec![RecordChange::Saved(expected[size].clone())]);
                assert!(client.finish(0).is_empty());
            }
        }
    }
    let mut invalid = vec![
        vec!["--page-size"],
        vec!["--page-size", "32", "--page-size", "32"],
        vec!["--binary-assets-v1", "--binary-assets-v1"],
        vec!["--page-size", "32", "--binary-assets-v1", "--page-size", "1"],
        vec!["--binary-assets-v1", "--page-size", "32", "--binary-assets-v1"],
        vec!["--page-size", "32", SENTINEL],
        vec!["--page-size", "32", "--unknown-option"],
        vec!["--page-size", "--binary-assets-v1"],
    ];
    for value in [
        "",
        "0",
        "201",
        "-1",
        "+1",
        "1.0",
        " 1",
        "1 ",
        "1\n",
        "\t1",
        "１",
        "١",
        "184467440737095516160000",
        SENTINEL,
    ] {
        invalid.push(vec!["--page-size", value]);
        invalid.push(vec!["--binary-assets-v1", "--page-size", value]);
        invalid.push(vec!["--page-size", value, "--binary-assets-v1"]);
    }
    for options in invalid {
        let directory = tempfile::tempdir().unwrap();
        // Even an existing directory must remain empty: Service::open must not run.
        let output = command()
            .arg(directory.path())
            .arg("1")
            .args(&options)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr.as_slice(), b"cloud service startup-error: arguments\n");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        // A nonexistent sentinel path must classify as arguments, never storage.
        let missing = directory.path().join(SENTINEL);
        let output = command().arg(&missing).arg("1").args(&options).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr.as_slice(), b"cloud service startup-error: arguments\n");
        assert!(!missing.exists());
    }
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

#[test]
fn binary_assets_v1_large_arbitrary_pa_conditional_save_read_fetch_and_request_ids() {
    let directory = tempfile::tempdir().unwrap();
    let mut client = Client::open_binary(directory.path());
    client.create("z");
    // Every byte value, including zero and LF; validity cannot depend on compression.
    let mut record = input("large", &[0, 255, 10]);
    record.fields.insert(
        "pa".into(),
        Field::Asset(Asset::synthetic((0..960_000).map(|i| (i % 256) as u8).collect())),
    );
    let first = saved(&client.modify(true, vec![save(record.clone(), None)])[0]);
    matches_input(&first, &record);
    assert_eq!(client.read("large"), first);
    record.fields.insert(
        "pa".into(),
        Field::Asset(Asset::synthetic(
            (0..1_000_000).map(|i| (255 - i % 256) as u8).collect(),
        )),
    );
    let updated = saved(&client.modify(true, vec![save(record.clone(), Some(first.system_fields.clone()))])[0]);
    matches_input(&updated, &record);
    let conflict = client.modify(true, vec![save(record, Some(first.system_fields))]);
    assert_eq!(
        conflict[0].result.as_ref().unwrap_err().server_record.as_deref(),
        Some(&updated)
    );
    assert_eq!(client.read("large"), updated);
    assert_eq!(client.page(None).changes, vec![RecordChange::Saved(updated.clone())]);
    let response = client.response(read_operation("large"));
    assert_eq!(response.result, Ok(Reply::Record(updated.clone())));
    assert!(client.finish(0).is_empty());
    let service = Service::open(directory.path(), Config::default(), SeededRandom::new(3)).unwrap();
    let logs = service.request_log().unwrap();
    assert_eq!(logs.len(), 8); // Exactly one dispatch per original frame, including conflict.
    assert!(logs.windows(2).all(|pair| pair[0].request_id < pair[1].request_id));
    assert_eq!(logs.last().unwrap().request_id, response.request_id);
    drop(service);
    // Binary is opt-in; the same durable state still opens with legacy framing.
    let mut legacy = Client::open(directory.path());
    assert_eq!(legacy.call(Operation::FetchAllZones), Reply::AllZones(vec!["z".into()]));
    assert!(legacy.finish(0).is_empty());
}

#[test]
fn binary_assets_v1_refuses_invalid_headers_limits_and_truncation_before_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    let mut baseline = Client::open(directory.path());
    baseline.create("z");
    let kept = saved(&baseline.modify(true, vec![save(input("keep", &[42]), None)])[0]);
    assert!(baseline.finish(0).is_empty());
    let mut message = envelope(Operation::Modify {
        zone: "z".into(),
        atomic: true,
        changes: vec![save(
            input("keep", SENTINEL.as_bytes()),
            Some(kept.system_fields.clone()),
        )],
    });
    let fields = &mut message["operation"]["Modify"]["changes"][0]["Save"]["record"]["fields"];
    fields["pa"] = json!({"Asset": {"bytes": {"attachment": 0}, "sha256": vec![0; 32]}});
    let base = json!({"version": 1, "lengths": [3], "message": message});
    let mut cases = Vec::new();
    for index in [json!(-1), json!(1), json!(0.5), json!(true)] {
        let mut h = base.clone();
        h["message"]["operation"]["Modify"]["changes"][0]["Save"]["record"]["fields"]["pa"]["Asset"]["bytes"]["attachment"] =
            index;
        cases.push((h, "malformed-attachments"));
    }
    let mut duplicate = base.clone();
    duplicate["message"]["operation"]["Modify"]["changes"][0]["Save"]["record"]["fields"]["a"] =
        duplicate["message"]["operation"]["Modify"]["changes"][0]["Save"]["record"]["fields"]["pa"].clone();
    cases.push((duplicate, "malformed-attachments"));
    let mut escaped = base.clone();
    escaped["message"]["caller"]["instance"] = json!({"attachment": 0});
    cases.push((escaped, "malformed-attachments"));
    for (lengths, kind) in [
        (json!([1, 1]), "malformed-attachments"),
        (json!([4 * FRAME_BYTES + 1]), "attachment-limit"),
        (json!(vec![0; 17]), "attachment-limit"),
        (json!([4 * FRAME_BYTES, 4 * FRAME_BYTES, 1]), "attachment-limit"),
    ] {
        let mut h = base.clone();
        h["lengths"] = lengths;
        cases.push((h, kind));
    }
    let mut version = base.clone();
    version["version"] = json!(2);
    cases.push((version, "malformed-attachments"));
    for (header, kind) in cases {
        let mut client = Client::open_binary(directory.path());
        let mut bytes = serde_json::to_vec(&header).unwrap();
        bytes.push(b'\n');
        client.write(&bytes); // No body/EOF: validation must refuse without waiting for either.
        assert_eq!(client.line(), format!("{{\"protocol_error\":\"{kind}\"}}\n"));
        assert_eq!(client.finish(2), format!("cloud service protocol-error: {kind}\n"));
    }
    let mut truncated = Client::open_binary(directory.path());
    let mut bytes = serde_json::to_vec(&base).unwrap();
    bytes.push(b'\n');
    truncated.write(&bytes);
    truncated.write(&[10]);
    drop(truncated.input.take());
    assert_eq!(truncated.line(), "{\"protocol_error\":\"truncated-attachment\"}\n");
    assert_eq!(
        truncated.finish(2),
        "cloud service protocol-error: truncated-attachment\n"
    );
    let mut oversized = Client::open_binary(directory.path());
    oversized.write(&vec![b' '; FRAME_BYTES + 1]);
    assert_eq!(oversized.line(), "{\"protocol_error\":\"frame-too-large\"}\n");
    assert_eq!(oversized.finish(2), "cloud service protocol-error: frame-too-large\n");
    let service = Service::open(directory.path(), Config::default(), SeededRandom::new(3)).unwrap();
    assert_eq!(service.request_log().unwrap().len(), 2);
    drop(service);
    let mut client = Client::open_binary(directory.path());
    let mut exact =
        serde_json::to_vec(&json!({"version": 1, "lengths": [], "message": envelope(read_operation("keep"))})).unwrap();
    exact.resize(FRAME_BYTES, b' ');
    exact.push(b'\n');
    client.write(&exact);
    let h: Value = serde_json::from_str(&client.line()).unwrap();
    let response: Response = serde_json::from_value(h["message"].clone()).unwrap();
    assert_eq!(response.result, Ok(Reply::Record(kept)));
    assert!(client.finish(0).is_empty());
}
