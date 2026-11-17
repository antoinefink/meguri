//! Original, test-only public HTTP action worker. The parent owns every
//! independent Desktop/fault action and the complete eight-case verdict.
mod clock;
mod contract;
mod fixture;

use meguri_engine::{EventKind, JobKind, Subscription};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask};
use meguri_model::{clock::Clock, rng::{RandomSource, SeededRandom}};
use meguri_storage::{media::MediaBaselineOwner, space::StatvfsSpace};
use meguri_sync::{Endpoint, HostKey, Job, JobOptions, Method, Payload, ServerKind, login_blocking,
    media::{self, MediaSyncResult}, normal, oneway::{TransferAccess, TransferError},
    policy::{Acceptance, Boundary, Direction, Phase, Scope}, status::{CheckResult, StatusMemory}, transport::runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, io::{Read, Write}, path::Path, sync::{Arc, Mutex, OnceLock, mpsc}};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const ENDPOINT: &str = "http://127.0.0.1:8080/";
const LOST_REPLY_PROXY: &str = "http://127.0.0.1:18080";
const CASES: [&str; 8] = ["authentication", "full-upload", "full-download", "normal-no-change",
    "normal-bidirectional", "media-bidirectional", "media-no-change", "manual-uncertain-recovery"];

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Action { PrepareInitial, PrepareSnapshot, PrepareForward, PrepareReverseMedia, Authenticate, Upload, Download,
    EstablishBaseline, ImportBaseline, SettleNormal, MediaBaseline, NormalSend, NormalReceive, NormalUnchanged, MediaReceive, MediaSend, MediaUnchanged, UncertainUpload }
// Payload-free diagnosis solely for the fixed offline PrepareForward action.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum PrepareForwardStage { InputAdmission, HostAdmission, FixtureLookup, DeckSave, PresetSnapshot, PresetEdit,
    PresetSave, ConfigSave, NoteSave, ReviewStart, ReviewStartPredicate, ReviewHeadPredicate, ReviewAnswer,
    ReviewAnswerPredicate, ReviewEnd, DeleteSave, TemplateSave, Snapshot, HostClose, StateInventory, Output }
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input { case: String, action: Action }
fn need(ok: bool, code: &'static str) -> Result<()> { if ok { Ok(()) } else { Err(code.into()) } }
fn hex(data: &[u8]) -> String { data.iter().map(|b| format!("{b:02x}")).collect() }
fn sha(data: &[u8]) -> String { use sha2::{Digest, Sha256}; hex(&Sha256::digest(data)) }
fn output(name: &str, value: &Value) -> Result<String> {
    let bytes = serde_json::to_vec_pretty(value)?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(Path::new("/out").join(name))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(sha(&bytes))
}
fn random() -> Result<Arc<SeededRandom>> {
    static RANDOM: OnceLock<Arc<SeededRandom>> = OnceLock::new();
    if let Some(random) = RANDOM.get() { return Ok(random.clone()); }
    let mut bytes = [0; 8];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let value = Arc::new(SeededRandom::new(u64::from_le_bytes(bytes)));
    let _ = RANDOM.set(value.clone());
    Ok(RANDOM.get().ok_or("random-init")?.clone())
}
fn allowed(input: &Input) -> bool {
    use Action::*;
    CASES.contains(&input.case.as_str()) && match input.action {
        PrepareInitial | PrepareSnapshot => true,
        PrepareForward | NormalSend | NormalReceive => input.case == "normal-bidirectional",
        PrepareReverseMedia | MediaReceive | MediaSend => input.case == "media-bidirectional",
        Authenticate => input.case == "authentication",
        Upload => input.case == "full-upload",
        ImportBaseline => matches!(input.case.as_str(), "full-upload" | "normal-no-change" | "normal-bidirectional"),
        Download => input.case == "full-download",
        SettleNormal | NormalUnchanged => input.case == "normal-no-change",
        MediaUnchanged => input.case == "media-no-change",
        UncertainUpload => input.case == "manual-uncertain-recovery",
        EstablishBaseline => matches!(input.case.as_str(), "normal-no-change" | "normal-bidirectional"),
        MediaBaseline => matches!(input.case.as_str(), "media-no-change" | "media-bidirectional"),
    }
}
fn options(_uncertain: bool) -> JobOptions {
    // The owned observer passes ordinary traffic through and arms reply loss
    // solely from the closed (case, action) admission. Every HTTP Job is observed.
    JobOptions::new(Endpoint::parse(ENDPOINT).expect("fixed endpoint"), ServerKind::SelfHosted)
        .proxy(meguri_sync::Proxy::new(LOST_REPLY_PROXY))
}
fn finish<T>(events: Subscription, task: ManualTransferTask<T>) -> std::result::Result<T, TransferError> {
    while let Some(event) = events.recv() {
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == task.job.id) {
            return task.take_result().unwrap_or(Err(TransferError::Interrupted));
        }
    }
    Err(TransferError::Interrupted)
}
fn access(credentials: &contract::Credentials, key: &HostKey, uncertain: bool) -> Result<TransferAccess> {
    Ok(TransferAccess::new(Endpoint::parse(ENDPOINT).expect("fixed endpoint"), &credentials.username, key.clone(), options(uncertain))?)
}
fn manual(host: &CoreHost, credentials: &contract::Credentials, key: &HostKey, download: bool, uncertain: bool) -> Result<Value> {
    let direction = if download { Direction::Import } else { Direction::Export };
    let events = host.engine().stream().subscribe();
    let preview = match finish(events, host.prepare_manual_transfer(access(credentials, key, uncertain)?, direction, Scope::CollectionOnly)?) {
        Ok(preview) => preview,
        Err(e) => return Ok(json!({"operation_complete":false,"stage":"prepare","transfer_error":format!("{e:?}")})),
    };
    let persisted = host.manual_transfer_history().get(&preview.operation.id).map_err(TransferError::from)?;
    need(persisted == preview.operation && persisted.phase == Phase::Preview, "persisted-preview-mismatch")?;
    let id = persisted.id.clone();
    let events = host.engine().stream().subscribe();
    let result = finish(events, host.confirm_manual_transfer(access(credentials, key, uncertain)?, preview.confirmation)?);
    let done = host.manual_transfer_history().get(&id).map_err(TransferError::from)?;
    let accepted = result.as_ref().is_ok_and(|op| op.phase == Phase::Complete && op.collection_complete
        && op.acceptance == if download { Acceptance::NotSent } else { Acceptance::ExactOk });
    let error = result.as_ref().err().map(|e| format!("{e:?}"));
    let mut observed = json!({"stage":"confirm", "explicit_authority":if download {"download"} else {"upload"},
        "persisted_preview":persisted,"operation":done,"operation_complete":accepted,"transfer_error":error});
    if uncertain {
        // No second confirm or upload is issued. The existing policy supplies
        // the conservative offline recovery decision after the finite proxy fault.
        let events = host.engine().stream().subscribe();
        let recovery = finish(events, host.prepare_manual_transfer_recovery(id.clone())?);
        let fenced = matches!(recovery, Err(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown)));
        observed["recovery"] = json!({"blind_upload_replay_prevented":fenced,
            "explicit_recovery_boundary":fenced,"operation":host.manual_transfer_history().get(&id).map_err(TransferError::from)?,
            "error":recovery.as_ref().err().map(|e| format!("{e:?}"))});
        observed["operation_complete"] = json!(!accepted && fenced && done.acceptance == Acceptance::PossiblyAccepted);
    }
    Ok(observed)
}
fn normal_action(host: &CoreHost, key: HostKey, clock: Arc<dyn Clock>, random: Arc<SeededRandom>) -> Result<Value> {
    // Caller-owned memory and production HTTP (None), never a stub transport.
    let memory = Arc::new(Mutex::new(StatusMemory::default()));
    let task = normal::start(host.engine(), host.collection()?, options(false), key, clock, random, memory, None)?;
    Ok(match task.wait() {
        Ok(outcome) => {
            let complete = match outcome.check.result {
                CheckResult::NoChanges => outcome.merge.is_none(),
                CheckResult::Normal { .. } => outcome.merge.as_ref().is_some_and(|m| !m.one_way && m.skipped.is_empty()),
                CheckResult::OneWay { .. } => false,
            };
            json!({"operation_complete":complete,"check_result":format!("{:?}",outcome.check.result),
                "merge":outcome.merge.as_ref().map(|m| json!({"one_way":m.one_way,"skipped_count":m.skipped.len()}))})
        },
        Err(error) => json!({"operation_complete":false,"error_kind":error.kind.id(),"method":error.method.map(|m| m.name())}),
    })
}
fn totals(v: &MediaSyncResult) -> Value {
    json!({"uploaded_files":v.uploaded_files,"uploaded_deletions":v.uploaded_deletions,
        "downloaded_files":v.downloaded_files,"downloaded_deletions":v.downloaded_deletions,
        "checked":v.checked,"bytes_downloaded":v.bytes_downloaded,"bytes_uploaded":v.bytes_uploaded,
        "resynced":v.resynced,"oversized":v.oversized,"empty":v.empty,"awaiting":v.awaiting})
}
fn media_action(host: &CoreHost, key: HostKey, random: Arc<SeededRandom>) -> Result<Value> {
    let service = host.media()?;
    let generation = host.collection()?.generation().0;
    let engine = host.engine().clone();
    let events = engine.stream().subscribe();
    let (send, receive) = mpsc::sync_channel(1);
    let task = host.engine().start_job(JobKind::MediaSync, move |context| {
        let options = options(false).cancel(context.cancel.clone());
        let result = match Job::new(&options, Some(key), random.as_ref()) {
            Ok(mut job) => match media::reconcile(engine.threads(), &mut job, &service, generation,
                &MediaBaselineOwner::new("retained-synthetic-owner"), &mut |_| {}) {
                Ok(result) => json!({"operation_complete":result.awaiting == 0 && result.oversized.is_empty() && result.empty.is_empty(),
                    "partial_failure_hidden":false,"totals":totals(&result)}),
                Err(failure) => json!({"operation_complete":false,"partial_failure_hidden":false,
                    "error_kind":failure.error.kind.id(),"method":failure.error.method.map(|m| m.name()),
                    "phase":format!("{:?}", failure.snapshot.phase),"totals":totals(&failure.snapshot.totals)}),
            },
            Err(_) => json!({"operation_complete":false,"stage":"job_creation"}),
        };
        let _ = send.send(result);
        Ok(())
    })?;
    let observed = receive.recv()?;
    while let Some(event) = events.recv() {
        if matches!(event.kind,EventKind::JobFinished{id,..} if id==task.id) { break; }
    }
    Ok(observed)
}
fn authenticate(host: &CoreHost, credentials: &contract::Credentials, random: &dyn RandomSource, key: &HostKey) -> Result<Value> {
    let mut job = Job::new(&options(false), Some(key.clone()), random)?;
    let response = runtime::block_on(host.engine().threads(), options(false).cancel_token(), async {
        job.request(Method::Meta, Payload::Json(meguri_sync::wire::meta::encode_meta_request())).await?.bytes().await
    });
    let authenticated = response.as_ref().is_ok_and(|bytes| meguri_sync::wire::meta::decode_meta(bytes).is_ok());
    let wrong = format!("{:016x}{:016x}",random.next_u64(),random.next_u64());
    let refusal = login_blocking(host.engine().threads(), &options(false), random, &credentials.username, &wrong);
    let refused = refusal.as_ref().is_err_and(|e| e.is_auth_failure());
    Ok(json!({"operation_complete":authenticated && refused,"authenticated_request_succeeded":authenticated,
        "wrong_credentials_refused":refused}))
}
fn run(diagnostic: &mut Option<PrepareForwardStage>) -> Result<bool> {
    let bytes = fs::read("/in/action.json")?;
    let input: Input = serde_json::from_slice(&bytes)?;
    if input.action == Action::PrepareForward && input.case == "normal-bidirectional" {
        *diagnostic = Some(PrepareForwardStage::InputAdmission);
    }
    need(std::env::args_os().count() == 1, "no-arguments")?;
    need(fs::read_dir("/out")?.next().is_none(), "output-not-empty")?;
    need(allowed(&input), "action-not-in-selection")?;
    let client = contract::client_build()?;
    let preparing = matches!(input.action, Action::PrepareInitial | Action::PrepareSnapshot | Action::PrepareForward | Action::PrepareReverseMedia);
    let mut binding = Value::Null;
    let mut expected = Value::Null;
    if !preparing {
        let peer = contract::preflight()?;
        let manifest = contract::json("/in/input-manifest.json")?;
        contract::keys(&manifest, &["format","case","action","action_sha256","state_inputs_sha256","before_sha256","expected_sha256",
            "review_sha256","client_build_sha256","preflight_peer_sha256"])?;
        need(manifest["format"] == "meguri-retained-input-v1" && manifest["case"] == input.case
            && manifest["action"] == serde_json::to_value(input.action)? && manifest["action_sha256"] == sha(&bytes)
            && manifest["client_build_sha256"] == sha(&fs::read("/in/client-build.json")?)
            && manifest["preflight_peer_sha256"] == sha(&fs::read("/in/preflight.json")?), "action-input-binding")?;
        contract::artifact(&manifest["review_sha256"])?;
        expected = serde_json::from_slice(&contract::artifact(&manifest["expected_sha256"])?)?;
        let invocation = contract::invocation(&peer, &manifest, &bytes)?;
        binding = json!({"preflight":peer,"input":manifest,"invocation":invocation});
    }
    if diagnostic.is_some() { *diagnostic = Some(PrepareForwardStage::HostAdmission); }
    let random = random()?;
    let clock: Arc<dyn Clock> = Arc::new(clock::WallClock);
    let started = clock.now_ms();
    let root = Path::new("/state").join(&input.case);
    if !preparing {
        let frozen: Value = serde_json::from_slice(&contract::artifact(&binding["input"]["state_inputs_sha256"])?)?;
        need(state_inputs(&root)? == frozen, "state-files-changed-since-preparation")?;
    }
    let config = HostConfig::new(root.join("collection"), root.join("backups"), root.join("widget"),
        clock.clone(), random.clone(), Arc::new(StatvfsSpace));
    if input.action == Action::PrepareInitial {
        need(!root.exists(), "initial-state-already-exists")?;
        fs::create_dir_all(&config.collection_directory)?;
        meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), clock.as_ref(), random.as_ref())?;
    }
    let host = CoreHost::open(config)?;
    let result = (|| -> Result<Value> {
        if preparing {
            let mut limitations = Vec::new();
            match input.action {
                Action::PrepareInitial => fixture::initialize(&host, input.case == "full-download")?,
                Action::PrepareSnapshot => {},
                Action::PrepareForward => limitations = fixture::forward(&host, clock.as_ref(),
                    diagnostic.as_mut().ok_or("forward-stage-required")?)?,
                Action::PrepareReverseMedia => fixture::set_media(&host, &fixture::recipes()["media_utf8_exact"]["reverse"] )?,
                _ => unreachable!(),
            }
            if diagnostic.is_some() { *diagnostic = Some(PrepareForwardStage::Snapshot); }
            let snapshot = fixture::snapshot(&host)?;
            if diagnostic.is_some() { *diagnostic = Some(PrepareForwardStage::Output); }
            let digest = output("snapshot.json", &snapshot)?;
            return Ok(json!({"preparation_only":true,"preparation_complete":limitations.is_empty(),
                "limitations":limitations,"snapshot_sha256":digest,"snapshot":snapshot}));
        }
        let before = fixture::snapshot(&host)?;
        let frozen: Value = serde_json::from_slice(&contract::artifact(&binding["input"]["before_sha256"])?)?;
        need(before == frozen, "local-state-changed-since-preparation")?;
        let credentials = contract::credentials()?;
        let key = match login_blocking(host.engine().threads(), &options(false), random.as_ref(), &credentials.username, &credentials.password) {
            Ok(key) => key,
            Err(e) => return Ok(json!({"operation_complete":false,"stage":"login","auth_failure":e.is_auth_failure(),"before":before})),
        };
        let mut observed = match input.action {
            Action::Authenticate => authenticate(&host, &credentials, random.as_ref(), &key)?,
            Action::Upload | Action::EstablishBaseline => manual(&host, &credentials, &key, false, false)?,
            Action::Download | Action::ImportBaseline => manual(&host, &credentials, &key, true, false)?,
            Action::UncertainUpload => manual(&host, &credentials, &key, false, true)?,
            Action::SettleNormal | Action::NormalSend | Action::NormalReceive | Action::NormalUnchanged => normal_action(&host, key, clock.clone(), random.clone())?,
            Action::MediaReceive | Action::MediaSend | Action::MediaUnchanged | Action::MediaBaseline => media_action(&host, key, random.clone())?,
            _ => unreachable!(),
        };
        if input.action == Action::SettleNormal { observed["preparation_only"] = json!(true); }
        let after = fixture::snapshot(&host)?;
        let equal = match input.action {
            Action::NormalUnchanged | Action::MediaUnchanged => before == after && after == expected,
            Action::MediaReceive | Action::MediaSend | Action::MediaBaseline => after["media_sha256"] == expected["media_sha256"],
            Action::Authenticate | Action::UncertainUpload => before == after,
            _ => fixture::supported(&after) == fixture::supported(&expected),
        };
        if input.action == Action::MediaUnchanged {
            let zero = ["uploaded_files","uploaded_deletions","downloaded_files","downloaded_deletions","bytes_downloaded","bytes_uploaded"]
                .iter().all(|key| observed["totals"][*key] == 0);
            observed["zero_mutations_and_file_bytes"] = json!(zero);
            if !zero { observed["operation_complete"] = json!(false); }
        }
        if input.action == Action::NormalUnchanged && observed["check_result"] != "NoChanges" {
            observed["operation_complete"] = json!(false);
        }
        if matches!(input.action, Action::Upload | Action::Download | Action::EstablishBaseline | Action::ImportBaseline) {
            let preserved = before["media_sha256"] == after["media_sha256"];
            observed["collection_transfer_preserved_local_media"] = json!(preserved);
            if !preserved { observed["operation_complete"] = json!(false); }
        }
        observed["expected_state_equal"] = json!(equal);
        observed["before"] = before;
        observed["after"] = after;
        if !equal { observed["operation_complete"] = json!(false); }
        Ok(observed)
    })();
    let closed = host.close();
    runtime::shut_down();
    // Propagate the operation's first failure before considering close/output.
    // Cleanup does not advance the marker on an already-failed operation.
    let observed = result?;
    if diagnostic.is_some() { *diagnostic = Some(PrepareForwardStage::HostClose); }
    closed?;
    if diagnostic.is_some() { *diagnostic = Some(PrepareForwardStage::StateInventory); }
    let state = state_inputs(&root)?;
    if diagnostic.is_some() { *diagnostic = Some(PrepareForwardStage::Output); }
    let state_sha = output("state-inputs.json", &state)?;
    let okay = if preparing { observed["preparation_complete"] == true } else { observed["operation_complete"] == true };
    output("action-result.json", &json!({"format":"meguri-retained-action-v1", "case":input.case,"action":input.action,
        "actor":"client_a","client_build":client,"binding":binding,"action_sha256":sha(&bytes),
        "recipe_sha256":sha(fixture::RECIPES),"selection_sha256":sha(fixture::SELECTION),
        "state_inputs_sha256":state_sha,"started_ms":started,"finished_ms":clock.now_ms(),"observed":observed,"case_credit":false}))?;
    Ok(okay)
}
fn state_inputs(root: &Path) -> Result<Value> {
    fn visit(root: &Path, directory: &Path, rows: &mut Vec<Value>) -> Result<()> {
        let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let kind = entry.file_type()?;
            if kind.is_dir() { visit(root, &entry.path(), rows)?; }
            else {
                need(kind.is_file(), "nonregular-state-input")?;
                let data = fs::read(entry.path())?;
                rows.push(json!({"path":entry.path().strip_prefix(root)?.to_str().ok_or("state-path")?,
                    "bytes":data.len(),"sha256":sha(&data)}));
            }
        }
        Ok(())
    }
    let mut rows = Vec::new();
    visit(root, root, &mut rows)?;
    Ok(json!({"format":"meguri-retained-state-inputs-v1","files":rows}))
}
fn main() {
    let mut diagnostic = None;
    match run(&mut diagnostic) {
        Ok(true) => {},
        Ok(false) => std::process::exit(1),
        Err(_) => {
            // Errors can contain server text, paths or invalid secret JSON.
            // Retain a closed-vocabulary failure, never the Display payload.
            let _ = output("worker-error.json", &json!({"format":"meguri-retained-error-v1", "operation_complete":false,
                "case_credit":false,"reason":"input_or_public_api_failure"}));
            if let Some(stage) = diagnostic {
                // Both writes are best effort; neither replaces the first failure.
                let _ = output("prepare-forward-diagnostic.json", &json!({
                    "format":"meguri-retained-prepare-forward-diagnostic-v1", "case":"normal-bidirectional",
                    "action":"prepare_forward", "stage":stage}));
            }
            std::process::exit(2);
        },
    }
}
