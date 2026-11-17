//! Disposable CoreHost bootstrap against the packaged service, never routine sync.
use meguri_engine::{CancelToken, EventKind, OpSource, OpSpec, Subscription};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask, MeguriAccess,
    MeguriBootstrapDirection, MeguriBootstrapError, ProcessCore};
use meguri_model::{card::{CardQueue, CardType}, clock::{FixedClock, UtcOffset},
    ids::{DeckId, NotetypeId}, rng::SeededRandom};
use meguri_storage::{ReadRows, ReaderRole, Table, space::StatvfsSpace};
use meguri_sync::{HostKey, JobOptions, ServerKind, oneway::TransferError,
    policy::{Operation, Phase, Scope}, provider::{self, MeguriConnection, MeguriReadiness, Selection},
    transport::http::HttpTransport};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{env, fs, path::{Path, PathBuf}, sync::Arc, time::Duration};

mod timeout_clock {
    /// Monotonic time only for the task's absolute timeout; scheduling uses FixedClock.
    #[allow(clippy::disallowed_methods)]
    pub(super) fn now() -> std::time::Instant { std::time::Instant::now() }
}

const WAIT: Duration = Duration::from_secs(120);
const NAMES: [&str; 2] = ["bootstrap-binary.bin", "bootstrap-text.txt"];
const CONTENT: [&[u8]; 2] = [b"\0\xff\x80Meguri\0bootstrap", b"Public synthetic bootstrap media\n"];
const FIELDS: [(&str, &str); 2] = [
    ("Bootstrap first question", "First answer [sound:bootstrap-binary.bin]"),
    ("Bootstrap second question", "Second answer bootstrap-text.txt"),
];

struct Failure { stage: &'static str, category: String }
type Result<T> = std::result::Result<T, Failure>;
fn failure(stage: &'static str, category: impl Into<String>) -> Failure { Failure { stage, category: category.into() } }
fn require(value: bool, stage: &'static str) -> Result<()> {
    if value { Ok(()) } else { Err(failure(stage, "assertion")) }
}
// Never format error objects that might carry request bodies, credentials or paths.
fn checked<T, E>(value: std::result::Result<T, E>, stage: &'static str) -> Result<T> {
    value.map_err(|_| failure(stage, "operation"))
}
fn bootstrap<T>(value: std::result::Result<T, MeguriBootstrapError>, stage: &'static str) -> Result<T> {
    value.map_err(|e| failure(stage, match e {
        MeguriBootstrapError::NotSelected => "NotSelected".into(),
        MeguriBootstrapError::ConnectionChanged => "ConnectionChanged".into(),
        MeguriBootstrapError::Busy => "Busy".into(),
        MeguriBootstrapError::AlreadyBound => "AlreadyBound".into(),
        MeguriBootstrapError::Username => "Username".into(),
        MeguriBootstrapError::Engine(_) => "Engine".into(),
        MeguriBootstrapError::Transfer(_) => "Transfer".into(),
        // Its formatter emits only a closed category, never account data.
        MeguriBootstrapError::Ready(e) => format!("Ready/{e:?}"),
    }))
}
fn transfer(e: TransferError, stage: &'static str) -> Failure {
    // This formatter is explicitly category-only; Server retains JobError.kind.id().
    failure(stage, format!("{e:?}"))
}
fn finish<T>(events: Subscription, task: ManualTransferTask<T>, stage: &'static str) -> Result<T> {
    let id = task.job.id;
    let deadline = timeout_clock::now() + WAIT;
    loop {
        let remaining = deadline.checked_duration_since(timeout_clock::now())
            .ok_or_else(|| failure(stage, "JobFinishedDeadline"))?;
        let event = events.recv_timeout(remaining).ok_or_else(|| failure(stage, "JobFinishedUnavailable"))?;
        if matches!(event.kind, EventKind::JobFinished { id: finished, .. } if finished == id) { break; }
    }
    task.take_result().ok_or_else(|| failure(stage, "MissingJoinedResult"))?
        .map_err(|e| transfer(e, stage))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture { format: u32, origin: String, username: String, password: String, now_ms: i64 }
struct Args { phase: String, fixture: PathBuf, root: PathBuf, expected: Option<PathBuf>, output: PathBuf }
fn args() -> Result<Args> {
    let mut input = env::args().skip(1);
    let phase = input.next().ok_or_else(|| failure("arguments", "mode"))?;
    require(matches!(phase.as_str(), "upload" | "download" | "reopen"), "arguments")?;
    let mut fixture = None; let mut root = None; let mut expected = None; let mut output = None;
    while let Some(key) = input.next() {
        let slot = match key.as_str() {
            "--fixture" => &mut fixture, "--root" => &mut root,
            "--expected" => &mut expected, "--output" => &mut output,
            _ => return Err(failure("arguments", "unknown")),
        };
        require(slot.is_none(), "arguments")?;
        *slot = Some(PathBuf::from(input.next().ok_or_else(|| failure("arguments", "missing"))?));
    }
    let mut paths = Vec::new();
    for path in [&mut fixture, &mut root, &mut expected, &mut output].into_iter().flatten() {
        *path = checked(fs::canonicalize(&*path), "argument-path")?;
        paths.push(path.clone());
    }
    for (i, a) in paths.iter().enumerate() {
        for b in &paths[i + 1..] { require(!a.starts_with(b) && !b.starts_with(a), "path-alias")?; }
    }
    let get = |p: Option<PathBuf>| p.ok_or_else(|| failure("arguments", "missing"));
    require((phase == "upload") == expected.is_none(), "arguments")?;
    Ok(Args { phase, fixture: get(fixture)?, root: get(root)?, expected, output: get(output)? })
}
fn read_json(path: &Path) -> Result<Value> {
    checked(serde_json::from_slice(&checked(fs::read(path), "read-json")?), "decode-json")
}
fn write_json(path: &Path, value: &Value) -> Result<()> {
    checked(fs::write(path, checked(serde_json::to_vec(value), "encode-json")?), "write-json")
}
fn hash(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect() }
fn config(args: &Args, fixture: &Fixture) -> Result<HostConfig> {
    let collection = args.root.join("collection");
    checked(fs::create_dir_all(&collection), "collection-directory")?;
    Ok(HostConfig::new(collection, args.root.join("backups"), args.root.join("widget"),
        Arc::new(FixedClock::new(fixture.now_ms, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(if args.phase == "upload" { 418 } else { 919 })), Arc::new(StatvfsSpace)))
}
fn settle(host: &CoreHost) -> Result<()> {
    checked(checked(host.collection(), "collection")?.run(
        OpSpec::named("provider fixture barrier", OpSource::Rollover), |_| Ok(())), "rollover-barrier")?;
    require(host.derived().wait_idle(Duration::from_secs(20)), "derived-idle")
}
fn seed(host: &CoreHost) -> Result<()> {
    require(host.derived().wait_idle(Duration::from_secs(20)), "seed-derived-idle")?;
    let collection = checked(host.collection(), "seed-collection")?;
    let kind = {
        let reader = checked(collection.reader(ReaderRole::Query), "seed-reader")?;
        let mut basic = None;
        for id in checked(reader.ids_after(Table::Notetypes, None, 100), "seed-notetypes")? {
            if let Some(nt) = checked(reader.notetype(NotetypeId(id)), "seed-notetype")? {
                if nt.name == "Basic" { basic = Some(nt.id); }
            }
        }
        basic.ok_or_else(|| failure("seed", "native-basic-missing"))?
    };
    let deck = checked(meguri_engine::decks::add(&collection, "Provider Bootstrap".into()), "seed-deck")?.value.id;
    for (front, back) in FIELDS {
        checked(meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
            notetype_id: kind, fields: vec![front.into(), back.into()], tags: vec![], deck_id: deck,
            guid: None, update_add_defaults: false,
        }, OpSource::UserUndoable), "seed-note")?;
    }
    let media = checked(host.media(), "seed-media")?;
    for (name, bytes) in NAMES.into_iter().zip(CONTENT) {
        require(checked(media.add(name, bytes), "seed-media-add")? == name, "seed-media-name")?;
    }
    drop(media); drop(collection);
    settle(host)
}
fn facts(host: &CoreHost) -> Result<Value> {
    let collection = checked(host.collection(), "facts-collection")?;
    let reader = checked(collection.reader(ReaderRole::Query), "facts-reader")?;
    let notes = checked(reader.notes_after(None, 100), "facts-notes")?;
    let cards = checked(reader.cards_after(None, 100), "facts-cards")?;
    require(notes.len() == 2 && cards.len() == 2, "fixture-counts")?;
    require(checked(reader.revlog_after(None, 1), "facts-revlogs")?.is_empty(), "zero-revlogs")?;
    for c in &cards {
        require(c.reps == 0 && c.lapses == 0 && c.card_type == CardType::New && c.queue == CardQueue::New, "studyable-new-cards")?;
    }
    for ((front, back), n) in FIELDS.into_iter().zip(&notes) {
        require(n.field_values().collect::<Vec<_>>() == vec![front, back], "known-fields")?;
    }
    let mut decks = Vec::new();
    for id in checked(reader.ids_after(Table::Decks, None, 100), "facts-decks")? {
        let d = checked(reader.deck(DeckId(id)), "facts-deck")?.ok_or_else(|| failure("facts-deck", "missing"))?;
        decks.push(json!({"id":d.id.0,"name":d.name,"mtime":d.mtime.0,
            "common":d.common.to_bytes().as_ref(),"kind":d.kind.to_bytes().as_ref()}));
    }
    // All logical row fields except sync USN. In particular, retain every card
    // scheduling field and the complete card-data codec, including unknown keys.
    let notes: Vec<_> = notes.iter().map(|n| json!({"id":n.id.0,"guid":n.guid,
        "notetype_id":n.notetype_id.0,"mtime":n.mtime.0,"tags":n.tags,"fields":n.fields,
        "sort_field":n.sort_field,"checksum":n.checksum,"flags":n.flags,"data":n.data})).collect();
    let cards: Vec<_> = cards.iter().map(|c| json!({"id":c.id.0,"note_id":c.note_id.0,
        "deck_id":c.deck_id.0,"ord":c.ord,"mtime":c.mtime.0,"type":c.card_type.stored(),
        "queue":c.queue.stored(),"due":c.due,"interval":c.interval,"ease_factor":c.ease_factor,
        "reps":c.reps,"lapses":c.lapses,"remaining_steps":c.remaining_steps,"original_due":c.original_due,
        "original_deck_id":c.original_deck_id.0,"flags":c.flags,"data":c.data.to_text()})).collect();
    drop(reader);
    let media = checked(host.media(), "facts-media")?;
    let mut actual_names = Vec::new();
    for entry in checked(fs::read_dir(media.store().media_dir()), "media-directory")? {
        let entry = checked(entry, "media-entry")?;
        require(checked(entry.file_type(), "media-entry-type")?.is_file(), "media-entry-file")?;
        actual_names.push(entry.file_name().into_string().map_err(|_| failure("media-name", "utf8"))?);
    }
    actual_names.sort();
    require(actual_names == NAMES, "exact-media-names")?;
    let mut files = Vec::new();
    for (name, expected) in NAMES.into_iter().zip(CONTENT) {
        let bytes = checked(fs::read(media.store().media_dir().join(name)), "media-bytes")?;
        require(bytes == expected, "exact-media-bytes")?;
        files.push(json!({"name":name,"bytes":bytes,"sha256":hash(expected)}));
    }
    Ok(json!({"format":1,"notes":notes,"cards":cards,"decks":decks,"revlogs":0,"media":files}))
}
fn transport(connection: &MeguriConnection, ca: &[u8]) -> Result<HttpTransport> {
    let endpoint = checked(provider::native_endpoint(&connection.endpoint), "endpoint")?;
    checked(HttpTransport::new(&JobOptions::new(endpoint, ServerKind::SelfHosted).trust_root(ca.to_vec())), "http-transport")
}
fn access(connection: &MeguriConnection, fixture: &Fixture, key: &HostKey, ca: &[u8]) -> Result<MeguriAccess> {
    Ok(bootstrap(MeguriAccess::new(connection.clone(), &fixture.username, key.clone()), "access")?
        .with_transport_for_testing(Box::new(transport(connection, ca)?)))
}
fn unbound(host: &CoreHost) -> Result<()> {
    require(matches!(bootstrap(host.meguri_readiness(), "unbound-readiness")?, MeguriReadiness::Unbound { .. }), "unbound")?;
    let doc = checked(host.provider_authority().snapshot(), "unbound-settings")?;
    require(!doc.settings.automatic && !doc.meguri_settings.automatic, "automatic-off")
}
fn ready(host: &CoreHost) -> Result<Value> {
    require(bootstrap(host.meguri_readiness(), "ready-readiness")? == MeguriReadiness::Ready, "ready")?;
    let doc = checked(host.provider_authority().snapshot(), "ready-settings")?;
    require(doc.settings.enabled && doc.settings.automatic && doc.settings.media
        && doc.meguri_settings.enabled && doc.meguri_settings.automatic && doc.meguri_settings.media
        && doc.meguri_bootstrap.is_none(), "automatic-enabled")?;
    let binding = doc.meguri_binding.ok_or_else(|| failure("binding", "missing"))?;
    let baseline = checked(checked(host.media(), "baseline-media")?.store().sync_baseline(), "baseline")?;
    require(binding.media_epoch().matches(&baseline) && baseline.owner == Some(binding.media_owner()), "binding-baseline")?;
    // Equality witness only, never read back as admission input. No endpoint,
    // username, key, account reference or raw provider document is exported.
    Ok(json!({"format":1,"ready":true,"epoch":baseline.epoch,
        "binding_sha256":hash(&checked(serde_json::to_vec(&binding), "binding-witness")?)}))
}
fn retained(config: &HostConfig, op: &Operation, upload: bool) -> Result<()> {
    require(op.collection_complete && op.receipt.is_none(), "collection-complete")?;
    if upload {
        require(op.safety_backups.len() == 1, "safety-backup")?;
        for backup in &op.safety_backups { artifact(&config.backups_directory, backup)?; }
        let copy = op.server_copy.as_ref().ok_or_else(|| failure("server-copy", "missing"))?;
        artifact(&config.backups_directory, &copy.artifact)?;
    } else { require(op.safety_backups.is_empty(), "initial-no-backup")?; }
    Ok(())
}
fn artifact(root: &Path, artifact: &meguri_sync::policy::Artifact) -> Result<()> {
    let path = root.join(&artifact.name);
    let canonical = checked(fs::canonicalize(&path), "artifact-path")?;
    require(canonical.starts_with(checked(fs::canonicalize(root), "artifact-root")?), "artifact-owned")?;
    let bytes = checked(fs::read(path), "artifact-read")?;
    require(bytes.len() as u64 == artifact.identity.length
        && <[u8; 32]>::from(Sha256::digest(&bytes)) == artifact.identity.sha256, "artifact-identity")
}
fn run_bootstrap(host: &CoreHost, config: &HostConfig, fixture: &Fixture, ca: &[u8], upload: bool) -> Result<()> {
    let connection = checked(host.configure_meguri_connection(&fixture.origin), "configure")?;
    let handoff = checked(host.begin_provider_handoff(Selection::Meguri), "handoff-begin")?;
    checked(host.finish_provider_handoff(&handoff), "handoff-finish")?;
    unbound(host)?;
    let authenticated = provider::authenticate(host.provider_authority(), &connection, host.engine().threads(),
        config.random.as_ref(), &fixture.username, &fixture.password, CancelToken::new(),
        Some(Box::new(transport(&connection, ca)?))).map_err(|e| failure("authenticate", match e {
            provider::AuthenticationFailure::ConnectionChanged => "ConnectionChanged".into(),
            provider::AuthenticationFailure::Username => "Username".into(),
            provider::AuthenticationFailure::Cancelled => "Cancelled".into(),
            provider::AuthenticationFailure::Transport(e) => format!("Transport/{}", e.category()),
        }))?;
    let key = HostKey::new(authenticated.hostkey);
    unbound(host)?;
    let events = host.engine().stream().subscribe();
    let preview = finish(events, bootstrap(host.prepare_meguri_bootstrap(access(&connection, fixture, &key, ca)?,
        if upload { MeguriBootstrapDirection::UseDeviceCollection } else { MeguriBootstrapDirection::UseServerCollection }), "prepare")?, "prepare")?;
    require(preview.operation.scope == Scope::CollectionAndMedia && preview.operation.phase == Phase::Preview, "actual-preview")?;
    let id = preview.operation.id.clone();
    let events = host.engine().stream().subscribe();
    let op = finish(events, bootstrap(host.confirm_meguri_bootstrap(access(&connection, fixture, &key, ca)?, preview), "confirm")?, "confirm")?;
    require(op.id == id, "collection-operation")?;
    retained(config, &op, upload)?;
    unbound(host)?;
    let events = host.engine().stream().subscribe();
    let media = finish(events, bootstrap(host.prepare_meguri_bootstrap_media(access(&connection, fixture, &key, ca)?, op.id.clone()), "media-prepare")?, "media-prepare")?;
    let events = host.engine().stream().subscribe();
    let done = finish(events, bootstrap(host.run_meguri_bootstrap_media(access(&connection, fixture, &key, ca)?, media.confirmation), "media-run")?, "media-run")?;
    require(done.id == id && done.phase == Phase::Complete && done.collection_complete, "media-complete")?;
    unbound(host)?;
    let accounting = bootstrap(host.complete_meguri_bootstrap(&connection, &fixture.username, &done.id), "complete")?;
    let mut names: Vec<_> = accounting.files().iter().map(|f| f.name.as_str()).collect(); names.sort();
    require(accounting.is_complete() && accounting.verified() && accounting.remote_pending() == 0
        && accounting.affected().count() == 0 && accounting.transferred() == 2 && names == NAMES, "all-media-accounted")?;
    ready(host)?;
    Ok(())
}
fn execute(args: &Args) -> Result<()> {
    require(checked(fs::read_dir(&args.output), "output-directory")?.next().is_none(), "fresh-output")?;
    let fixture: Fixture = checked(serde_json::from_slice(&checked(fs::read(&args.fixture), "fixture-input")?), "fixture-format")?;
    require(fixture.format == 1 && fixture.origin == "https://localhost:18443" && fixture.now_ms > 0, "fixture-contract")?;
    let upload = args.phase == "upload";
    if args.phase != "reopen" { require(checked(fs::read_dir(&args.root), "client-root")?.next().is_none(), "fresh-client")?; }
    let config = config(args, &fixture)?;
    let mut process = None;
    let host = if upload {
        checked(meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"),
            config.clock.as_ref(), config.random.as_ref()), "create")?;
        checked(CoreHost::open(config.clone()), "open-upload")?
    } else if args.phase == "download" {
        let p = checked(ProcessCore::new(config.engine.clone()), "process")?;
        let h = checked(p.open_for_manual_import(config.clone()), "open-initial")?;
        process = Some(p);
        require(h.collection().is_err(), "initial-no-collection")?;
        h
    } else { checked(CoreHost::open(config.clone()), "reopen")? };
    let expected = if upload { seed(&host)?; facts(&host)? } else {
        read_json(&args.expected.as_ref().ok_or_else(|| failure("expected", "missing"))?.join("expected-facts.json"))?
    };
    if args.phase != "reopen" {
        let ca = checked(fs::read(args.fixture.with_file_name("ca.der")), "fixture-ca")?;
        run_bootstrap(&host, &config, &fixture, &ca, upload)?;
    }
    settle(&host)?;
    require(facts(&host)? == expected, "exact-logical-and-media-equality")?;
    let witness = ready(&host)?;
    if args.phase == "reopen" { require(read_json(&args.root.join("reopen-witness.json"))? == witness, "persisted-binding-epoch")?; }
    checked(host.close(), "close")?;
    drop(host);
    drop(process);
    if upload { write_json(&args.output.join("expected-facts.json"), &expected)?; }
    if args.phase == "download" { write_json(&args.root.join("reopen-witness.json"), &witness)?; }
    let checks = if upload { vec!["fresh-upload-client", "explicit-collection-and-media", "retained-safety-artifacts", "all-media-accounted", "ready-automatic", "exact-fixture", "closed"] }
        else if args.phase == "download" { vec!["initial-no-collection", "explicit-collection-and-media", "initial-no-backup", "all-media-accounted", "ready-automatic", "exact-fixture", "closed"] }
        else { vec!["cold-ready", "persisted-binding-epoch", "exact-fixture", "closed"] };
    write_json(&args.output.join("result.json"), &json!({"format":1,"phase":args.phase,"status":"pass",
        "checks":checks,"counts":{"notes":2,"cards":2,"media":2,"revlogs":0,"reps":0}}))
}
fn main() {
    // Even an unexpected panic must not print credential-bearing assertion data.
    std::panic::set_hook(Box::new(|_| eprintln!("provider-client: panic")));
    let args = match args() { Ok(v) => v, Err(_) => { eprintln!("provider-client: arguments/invalid"); std::process::exit(2); } };
    if let Err(error) = execute(&args) {
        // The phase output is private to MG; only the fixed stage is public.
        let _ = write_json(&args.output.join("failure.private.json"), &json!({"format":1,"stage":error.stage,"category":error.category}));
        eprintln!("provider-client: {}/failed", error.stage);
        std::process::exit(1);
    }
}
