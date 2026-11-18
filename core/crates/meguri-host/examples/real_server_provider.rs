//! Disposable synthetic CoreHost bootstrap and opt-in daily round against the packaged service.
use meguri_engine::{CancelToken, EventKind, OpSource, OpSpec, Subscription};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask, MeguriAccess,
    MeguriBootstrapDirection, MeguriBootstrapError, MeguriCollectionState, MeguriRoundStart,
    MeguriRoundTask, MeguriRoundResult, ProcessCore};
use meguri_model::{card::{CardQueue, CardType}, clock::{FixedClock, UtcOffset},
    ids::{DeckId, NotetypeId}, rng::SeededRandom, time::EpochMillis};
use meguri_storage::{Cell, Record, ReadRows, ReaderRole, Table, space::StatvfsSpace};
use meguri_engine::{rendering::Platform, sched::fsrs_policy::{self, PolicyStatus},
    session::{AnswerOutcome, StartResult}};
use meguri_sched::answer::Rating;
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
const DAILY_CONTENT: &[u8] = b"Public synthetic daily review replacement\n";
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
struct Args { phase: String, review_round: bool, fixture: PathBuf, root: PathBuf, expected: Option<PathBuf>, output: PathBuf }
fn args() -> Result<Args> {
    let mut input = env::args().skip(1);
    let phase = input.next().ok_or_else(|| failure("arguments", "mode"))?;
    require(matches!(phase.as_str(), "upload" | "download" | "reopen" | "review-send" | "review-receive" | "review-reopen"), "arguments")?;
    let mut review_round = false;
    let mut fixture = None; let mut root = None; let mut expected = None; let mut output = None;
    while let Some(key) = input.next() {
        if key == "--review-round" { require(!review_round, "arguments")?; review_round = true; continue; }
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
    require(!phase.starts_with("review-") || review_round, "arguments")?;
    Ok(Args { phase, review_round, fixture: get(fixture)?, root: get(root)?, expected, output: get(output)? })
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
        Arc::new(FixedClock::new(if args.phase.starts_with("review-") { daily_time(fixture)? } else { fixture.now_ms }, UtcOffset::UTC)),
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
    let files = media_facts(host, false)?;
    Ok(json!({"format":1,"notes":notes,"cards":cards,"decks":decks,"revlogs":0,"media":files}))
}
fn media_facts(host: &CoreHost, daily: bool) -> Result<Value> {
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
    for (name, expected) in NAMES.into_iter().zip(if daily { [DAILY_CONTENT, CONTENT[1]] } else { CONTENT }) {
        let bytes = checked(fs::read(media.store().media_dir().join(name)), "media-bytes")?;
        require(bytes == expected, "exact-media-bytes")?;
        files.push(json!({"name":name,"bytes":bytes,"sha256":hash(expected)}));
    }
    Ok(json!(files))
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
        && doc.meguri_settings.automatic && doc.meguri_settings.media
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
fn authenticate(host: &CoreHost, config: &HostConfig, connection: &MeguriConnection,
    fixture: &Fixture, ca: &[u8]) -> Result<HostKey> {
    let authenticated = provider::authenticate(host.provider_authority(), connection, host.engine().threads(),
        config.random.as_ref(), &fixture.username, &fixture.password, CancelToken::new(),
        Some(Box::new(transport(connection, ca)?))).map_err(|e| failure("authenticate", match e {
            provider::AuthenticationFailure::ConnectionChanged => "ConnectionChanged".into(),
            provider::AuthenticationFailure::Username => "Username".into(),
            provider::AuthenticationFailure::Cancelled => "Cancelled".into(),
            provider::AuthenticationFailure::Transport(e) => format!("Transport/{}", e.category()),
        }))?;
    Ok(HostKey::new(authenticated.hostkey))
}
fn run_bootstrap(host: &CoreHost, config: &HostConfig, fixture: &Fixture, ca: &[u8], upload: bool) -> Result<()> {
    let connection = checked(host.configure_meguri_connection(&fixture.origin), "configure")?;
    let handoff = checked(host.begin_provider_handoff(Selection::Meguri), "handoff-begin")?;
    checked(host.finish_provider_handoff(&handoff), "handoff-finish")?;
    unbound(host)?;
    let key = authenticate(host, config, &connection, fixture, ca)?;
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
fn daily_time(fixture: &Fixture) -> Result<i64> {
    // Let the production one-second row clock distinguish this answer from seed.
    fixture.now_ms.checked_add(60_000).ok_or_else(|| failure("daily-clock", "overflow"))
}
fn fsrs_ok(host: &CoreHost) -> Result<()> {
    require(checked(fsrs_policy::policy_status(&checked(host.collection(), "fsrs-collection")?),
        "fsrs-policy")? == PolicyStatus::Ok, "fsrs-ok")
}
fn row(record: &impl Record) -> Value {
    // Preserve storage classes, arbitrary text/blob bytes and every float bit.
    json!(record.to_cells().into_iter().map(|cell| match cell {
        Cell::Null => json!(["null"]),
        Cell::Integer(v) => json!(["integer", v]),
        Cell::Real(v) => json!(["real-bits", v.to_bits()]),
        Cell::Text(v) => json!(["text", v]),
        Cell::Blob(v) => json!(["blob", v]),
    }).collect::<Vec<_>>())
}
fn daily_facts(host: &CoreHost, fixture: &Fixture, initial: &Value) -> Result<Value> {
    fsrs_ok(host)?;
    require(host.review().active_session().is_none(), "no-active-review")?;
    let collection = checked(host.collection(), "daily-collection")?;
    let reader = checked(collection.reader(ReaderRole::Query), "daily-reader")?;
    let notes = checked(reader.notes_after(None, 100), "daily-notes")?;
    let cards = checked(reader.cards_after(None, 100), "daily-cards")?;
    let logs = checked(reader.revlog_after(None, 100), "daily-revlogs")?;
    require(notes.len() == 2 && cards.len() == 2 && logs.len() == 1, "daily-counts")?;
    let log = &logs[0];
    require(log.button == 3 && log.answer_millis == 321 && log.id.0 == daily_time(fixture)?, "one-good-revlog")?;
    require(cards.iter().map(|c| u64::from(c.reps)).sum::<u64>() == 1
        && cards.iter().all(|c| c.lapses == 0), "one-repetition-no-lapse")?;
    let answered = cards.iter().find(|c| c.id == log.card_id).ok_or_else(|| failure("daily-card", "missing"))?;
    require(answered.reps == 1 && answered.card_type != CardType::New
        && answered.data.memory_state.is_some() && answered.mtime.0 == daily_time(fixture)? / 1000, "actual-fsrs-scheduling")?;
    for c in &cards {
        let original = initial["cards"].as_array().and_then(|rows| rows.iter().find(|v| v["id"] == json!(c.id.0)))
            .ok_or_else(|| failure("daily-card", "original-missing"))?;
        require(original["note_id"] == json!(c.note_id.0) && original["deck_id"] == json!(c.deck_id.0), "original-card-identity")?;
        if c.id != log.card_id {
            require(c.reps == 0 && c.lapses == 0 && c.card_type == CardType::New && c.queue == CardQueue::New
                && original["mtime"] == json!(c.mtime.0) && original["data"] == json!(c.data.to_text()), "second-card-unreviewed")?;
        }
    }
    for (n, original) in notes.iter().zip(initial["notes"].as_array().ok_or_else(|| failure("daily-notes", "original-missing"))?) {
        require(original == &json!({"id":n.id.0,"guid":n.guid,"notetype_id":n.notetype_id.0,
            "mtime":n.mtime.0,"tags":n.tags,"fields":n.fields,"sort_field":n.sort_field,
            "checksum":n.checksum,"flags":n.flags,"data":n.data}), "unchanged-notes")?;
    }
    let decks = checked(reader.deck_rows(), "daily-decks")?;
    let original_decks = initial["decks"].as_array().ok_or_else(|| failure("daily-decks", "original-missing"))?;
    require(decks.len() == original_decks.len() && decks.iter().all(|d| original_decks.iter().any(|v|
        v["id"] == json!(d.id.0) && v["name"] == json!(d.name) && v["kind"] == json!(d.kind.to_bytes().as_ref()))), "original-decks")?;
    // Answering changes real deck statistics. Retain those resulting full rows,
    // along with every actual card/revlog USN; never compare collection next-USN.
    let fsrs = checked(reader.config_entry("fsrs"), "daily-fsrs-config")?
        .ok_or_else(|| failure("daily-fsrs-config", "missing"))?;
    require(fsrs.is_blob, "daily-fsrs-config-blob")?;
    let result = json!({"format":1,"notes":notes.iter().map(row).collect::<Vec<_>>(),
        "cards":cards.iter().map(row).collect::<Vec<_>>(),"revlogs":logs.iter().map(row).collect::<Vec<_>>(),
        "decks":decks.iter().map(row).collect::<Vec<_>>(),"fsrs":row(&fsrs.entry),"fsrs_ok":true});
    drop(reader); drop(collection);
    let mut result = result;
    result["media"] = media_facts(host, true)?;
    Ok(result)
}
// PUBLIC provider-client-equality-v1 is one compact line, below 8KiB by its
// fixed schema (only booleans, fixed labels, usize counts and fixed-width masks).
// Masks are ordered left to right in Record::to_cells order:
// notes: id,guid,notetype_id,mtime,usn,tags,fields,sort_field,checksum,flags,data
// cards: id,note_id,deck_id,ord,mtime,usn,type,queue,due,interval,ease_factor,
//        reps,lapses,remaining_steps,original_due,original_deck_id,flags,data
// revlogs: id,card_id,usn,button,interval,last_interval,ease_factor,answer_millis,kind
// decks: id,name,mtime,usn,common,kind; fsrs: key,usn,mtime,value.
// A true bit observes unequal whole cells at corresponding positions. No identity
// alignment occurs. position_key_mismatch may mean reordered or different rows;
// mask_valid must be true before zero bits imply complete column equality.
fn daily_equality_diagnostic(actual: &Value, expected: &Value) {
    use std::io::Write;
    const KEYS: [&str; 8] = ["format", "notes", "cards", "revlogs", "decks", "fsrs", "fsrs_ok", "media"];
    const LIMIT: usize = 100;
    fn exact_keys(v: &Value) -> bool {
        v.as_object().is_some_and(|o| o.len() == KEYS.len() && KEYS.iter().all(|k| o.contains_key(*k)))
    }
    fn bytes(v: &Value) -> bool {
        v.as_array().is_some_and(|a| a.iter().all(|b| b.as_u64().is_some_and(|n| n <= 255)))
    }
    fn cell(v: &Value) -> bool {
        let Some(c) = v.as_array() else { return false; };
        match c.first().and_then(Value::as_str) {
            Some("null") => c.len() == 1,
            Some("integer") => c.len() == 2 && c[1].as_i64().is_some(),
            Some("real-bits") => c.len() == 2 && c[1].as_u64().is_some(),
            Some("text" | "blob") => c.len() == 2 && bytes(&c[1]),
            _ => false,
        }
    }
    fn row_shape(v: &Value, width: usize) -> &'static str {
        let Some(r) = v.as_array() else { return "wrong-type"; };
        if r.len() != width { return "wrong-width"; }
        if !r.iter().all(cell) { return "invalid-cell"; }
        "valid"
    }
    fn kind(v: Option<&Value>) -> &'static str {
        match v { None => "missing", Some(v) if v.is_array() => "array", _ => "not-array" }
    }
    fn scalar(a: Option<&Value>, b: Option<&Value>, boolean: bool) -> Value {
        let valid = |v: Option<&Value>| v.is_some_and(|v| if boolean { v.is_boolean() } else { v.as_i64().is_some() });
        json!({"equal":a == b,"actual":{"present":a.is_some(),"shape_valid":valid(a)},
            "expected":{"present":b.is_some(),"shape_valid":valid(b)}})
    }
    fn rows(a: Option<&Value>, b: Option<&Value>, width: usize, single_fsrs: bool) -> Value {
        let aa = a.and_then(Value::as_array);
        let bb = b.and_then(Value::as_array);
        let count = |r: Option<&Vec<Value>>| if single_fsrs { usize::from(r.is_some()) } else { r.map_or(0, Vec::len) };
        let an = count(aa); let bn = count(bb);
        let capped = [an > LIMIT, bn > LIMIT];
        // Flags accumulate only over existing rows; cardinality is independent.
        let mut bad_type = [false; 2]; let mut bad_width = [false; 2]; let mut bad_cell = [false; 2];
        let mut different = vec![false; width];
        let mut position_key_mismatch = false;
        // FSRS is a single row, never four rows. Even a non-array FSRS value is
        // classified below; no columns are accessed until both rows are valid.
        let n = if single_fsrs { 1 } else { an.max(bn).min(LIMIT) };
        for i in 0..n {
            let ar = if single_fsrs { a } else { aa.and_then(|r| r.get(i)) };
            let br = if single_fsrs { b } else { bb.and_then(|r| r.get(i)) };
            let mut valid = [false; 2];
            for (side, row) in [ar, br].into_iter().enumerate() {
                if let Some(row) = row {
                    let shape = row_shape(row, width);
                    bad_type[side] |= shape == "wrong-type";
                    bad_width[side] |= shape == "wrong-width";
                    bad_cell[side] |= shape == "invalid-cell";
                    valid[side] = shape == "valid";
                }
            }
            if valid[0] && valid[1] {
                if let (Some(ar), Some(br)) = (ar.and_then(Value::as_array), br.and_then(Value::as_array)) {
                    for col in 0..width { different[col] |= ar[col] != br[col]; }
                    position_key_mismatch |= ar[0] != br[0];
                }
            }
        }
        let shape_valid = [aa.is_some() && !capped[0] && !bad_type[0] && !bad_width[0] && !bad_cell[0],
            bb.is_some() && !capped[1] && !bad_type[1] && !bad_width[1] && !bad_cell[1]];
        let cardinality_equal = aa.zip(bb).map(|(a, b)| if single_fsrs { true } else { a.len() == b.len() });
        let mask_valid = shape_valid[0] && shape_valid[1] && cardinality_equal == Some(true);
        let side = |v: Option<&Value>, r: Option<&Vec<Value>>, i: usize| {
            let mut facts = json!({"kind":kind(v),"shape_valid":shape_valid[i],"wrong_type":bad_type[i],
                "wrong_width":bad_width[i],"invalid_cell":bad_cell[i],"capped":capped[i]});
            // Counts are null for missing/non-array values, never invented zero.
            if single_fsrs { facts["cell_count"] = json!(r.map(Vec::len)); }
            else { facts["array_count"] = json!(r.map(Vec::len)); }
            facts
        };
        let mut facts = json!({"equal":a == b,"actual":side(a, aa, 0),"expected":side(b, bb, 1),
            "different_columns":different,"mask_valid":mask_valid,"incomplete":!mask_valid});
        if !single_fsrs {
            facts["cardinality_equal"] = json!(cardinality_equal);
            facts["position_key_mismatch"] = json!(position_key_mismatch);
            facts["position_keys_complete"] = json!(mask_valid);
        }
        facts
    }
    fn media(a: Option<&Value>, b: Option<&Value>) -> Value {
        const FIELDS: [&str; 3] = ["name", "bytes", "sha256"];
        let aa = a.and_then(Value::as_array); let bb = b.and_then(Value::as_array);
        let an = aa.map_or(0, Vec::len); let bn = bb.map_or(0, Vec::len);
        let capped = [an > LIMIT, bn > LIMIT];
        let mut wrong_type = [false; 2]; let mut wrong_keys = [false; 2]; let mut invalid_fields = [false; 2];
        let mut different = [false; 3];
        for i in 0..an.max(bn).min(LIMIT) {
            let ar = aa.and_then(|r| r.get(i)); let br = bb.and_then(|r| r.get(i));
            let mut valid = [false; 2];
            for (side, row) in [ar, br].into_iter().enumerate() {
                if let Some(row) = row {
                    if let Some(o) = row.as_object() {
                        let keys = o.len() == 3 && FIELDS.iter().all(|k| o.contains_key(*k));
                        wrong_keys[side] |= !keys;
                        // Inspect only the three literal fields, never input keys.
                        let fields = keys && o.get("name").is_some_and(Value::is_string)
                            && o.get("bytes").is_some_and(bytes)
                            && o.get("sha256").and_then(Value::as_str).is_some_and(|s|
                                s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()));
                        invalid_fields[side] |= keys && !fields;
                        valid[side] = fields;
                    } else { wrong_type[side] = true; }
                }
            }
            if valid[0] && valid[1] {
                if let (Some(ar), Some(br)) = (ar, br) {
                    for (col, key) in FIELDS.into_iter().enumerate() { different[col] |= ar.get(key) != br.get(key); }
                }
            }
        }
        let shape_valid = [aa.is_some() && !capped[0] && !wrong_type[0] && !wrong_keys[0] && !invalid_fields[0],
            bb.is_some() && !capped[1] && !wrong_type[1] && !wrong_keys[1] && !invalid_fields[1]];
        let cardinality_equal = aa.zip(bb).map(|(a, b)| a.len() == b.len());
        let mask_valid = shape_valid[0] && shape_valid[1] && cardinality_equal == Some(true);
        let side = |v: Option<&Value>, r: Option<&Vec<Value>>, i: usize| json!({"kind":kind(v),
            "shape_valid":shape_valid[i],"array_count":r.map(Vec::len),"wrong_type":wrong_type[i],
            "wrong_keys":wrong_keys[i],"invalid_fields":invalid_fields[i],"capped":capped[i]});
        json!({"equal":a == b,"actual":side(a, aa, 0),"expected":side(b, bb, 1),
            "cardinality_equal":cardinality_equal,"different_fields":{"name":different[0],"bytes":different[1],"sha256":different[2]},
            "mask_valid":mask_valid,"incomplete":!mask_valid})
    }
    // Only literal lookups enter these constructors. No input Value, key, cell,
    // payload, tag, filename, digest, scalar value or error is serialized.
    let facts = json!({"actual_object":actual.is_object(),"expected_object":expected.is_object(),
        "actual_exact_keys":exact_keys(actual),"expected_exact_keys":exact_keys(expected),"equal":false,
        "format":scalar(actual.get("format"), expected.get("format"), false),
        "notes":rows(actual.get("notes"), expected.get("notes"), 11, false),
        "cards":rows(actual.get("cards"), expected.get("cards"), 18, false),
        "revlogs":rows(actual.get("revlogs"), expected.get("revlogs"), 9, false),
        "decks":rows(actual.get("decks"), expected.get("decks"), 6, false),
        "fsrs":rows(actual.get("fsrs"), expected.get("fsrs"), 4, true),
        "fsrs_ok":scalar(actual.get("fsrs_ok"), expected.get("fsrs_ok"), true),
        "media":media(actual.get("media"), expected.get("media"))});
    // Best effort only: stderr failure must not replace the equality assertion.
    let _ = writeln!(std::io::stderr().lock(), "provider-client-equality-v1:{facts}");
}
fn round_witness(host: &CoreHost) -> Result<Value> {
    let witness = ready(host)?;
    let doc = checked(host.provider_authority().snapshot(), "round-settings")?;
    require(doc.selection == Selection::Meguri && doc.handoff.is_none(), "round-selected")?;
    Ok(json!({"ready":witness,"settings":doc.settings,"meguri_settings":doc.meguri_settings,
        "connection_sha256":hash(&checked(serde_json::to_vec(&doc.meguri), "connection-witness")?)}))
}
fn owned_result(host: &CoreHost, events: Subscription, start: MeguriRoundStart) -> Result<Arc<MeguriRoundResult>> {
    let task: Arc<MeguriRoundTask> = match start {
        MeguriRoundStart::Started(task) => task,
        MeguriRoundStart::Refused(_) => return Err(failure("daily-admission", "refused")),
    };
    let id = task.handle().id;
    let deadline = timeout_clock::now() + WAIT;
    loop {
        let remaining = deadline.checked_duration_since(timeout_clock::now())
            .ok_or_else(|| failure("daily-round", "JobFinishedDeadline"))?;
        let event = events.recv_timeout(remaining).ok_or_else(|| failure("daily-round", "JobFinishedUnavailable"))?;
        if matches!(event.kind, EventKind::JobFinished { id: finished, .. } if finished == id) { break; }
    }
    checked(host.meguri_round_result(&task), "daily-finalization")?
        .ok_or_else(|| failure("daily-finalization", "missing"))
}
fn run_daily(host: &CoreHost, config: &HostConfig, fixture: &Fixture, ca: &[u8]) -> Result<()> {
    let connection = checked(host.provider_authority().snapshot(), "daily-connection")?.meguri
        .ok_or_else(|| failure("daily-connection", "missing"))?;
    let key = authenticate(host, config, &connection, fixture, ca)?;
    let events = host.engine().stream().subscribe();
    let result = owned_result(host, events, host.start_meguri_collection(access(&connection, fixture, &key, ca)?))?;
    let collection = result.collection.as_ref().ok_or_else(|| failure("daily-collection-result", "missing"))?;
    require(result.recovery.is_none() && result.media.is_none() && collection.state == MeguriCollectionState::Merged
        && collection.error.is_none() && collection.refusal.is_none() && !collection.acceptance_unknown
        && collection.skipped == 0, "daily-collection-merged")?;
    let events = host.engine().stream().subscribe();
    let result = owned_result(host, events, host.start_meguri_media(access(&connection, fixture, &key, ca)?))?;
    let media = result.media.as_ref().ok_or_else(|| failure("daily-media-result", "missing"))?;
    require(result.recovery.is_none() && result.collection.is_none() && media.complete && media.failure.is_none()
        && media.refusal.is_none() && media.accounting_error.is_none() && media.accounting.as_ref().is_some_and(|a|
            a.excluded == 0 && a.pending == 0 && a.awaiting == 0 && a.affected.is_empty()), "daily-media-complete")
}
fn review_once(host: &CoreHost, fixture: &Fixture) -> Result<()> {
    let deck = {
        let collection = checked(host.collection(), "review-collection")?;
        let reader = checked(collection.reader(ReaderRole::Query), "review-reader")?;
        checked(reader.deck_rows(), "review-decks")?.into_iter().find(|d| d.name == "Provider Bootstrap")
            .ok_or_else(|| failure("review-deck", "missing"))?.id
    };
    let StartResult::Started { session_id, head, .. } = checked(host.review().start(deck, None), "review-start")?
        else { return Err(failure("review-start", "refused")); };
    let item = head.items.first().ok_or_else(|| failure("review-head", "empty"))?;
    let fields = {
        let collection = checked(host.collection(), "review-note-collection")?;
        let reader = checked(collection.reader(ReaderRole::Query), "review-note-reader")?;
        let note = checked(reader.note(item.note_id), "review-note")?.ok_or_else(|| failure("review-note", "missing"))?;
        let values = note.field_values().map(str::to_owned).collect::<Vec<_>>();
        FIELDS.into_iter().find(|(front, back)| values == vec![*front, *back])
            .ok_or_else(|| failure("review-note", "unknown-faces"))?
    };
    let rendered = checked(host.review().render_head(session_id, &[item.render_key], Platform::Mac), "review-render")?;
    require(rendered.len() == 1 && rendered[0].card_id == item.card_id
        && rendered[0].question.as_ref().is_some_and(|s| s.contains(fields.0))
        && rendered[0].answer.as_ref().is_some_and(|s| s.contains(fields.1.split(" [sound:").next().unwrap_or(fields.1))),
        "rendered-question-and-answer")?;
    require(matches!(checked(host.review().answer(session_id, item.card_id, Rating::Good,
        item.state_token, EpochMillis(daily_time(fixture)?), 321), "review-answer")?, AnswerOutcome::Accepted { .. }), "review-accepted")?;
    checked(host.review().end(session_id), "review-end")?;
    require(host.review().active_session().is_none(), "review-ended")?;
    {
        let collection = checked(host.collection(), "answered-collection")?;
        let reader = checked(collection.reader(ReaderRole::Query), "answered-reader")?;
        let logs = checked(reader.revlog_after(None, 100), "answered-revlogs")?;
        require(logs.len() == 1 && logs[0].card_id == item.card_id && logs[0].button == 3
            && logs[0].answer_millis == 321, "issued-card-revlog")?;
    }
    require(checked(checked(host.media(), "daily-write-media")?.write_exact(NAMES[0], DAILY_CONTENT), "daily-write-exact")? == NAMES[0], "daily-write-name")?;
    settle(host)
}
fn execute_daily(args: &Args, fixture: &Fixture, config: &HostConfig) -> Result<()> {
    let host = checked(CoreHost::open(config.clone()), "daily-cold-open")?;
    settle(&host)?;
    fsrs_ok(&host)?;
    require(host.review().active_session().is_none(), "daily-inactive")?;
    let witness = round_witness(&host)?;
    require(read_json(&args.root.join("round-witness.json"))? == witness, "original-binding-epoch-settings")?;
    let expected_dir = args.expected.as_ref().ok_or_else(|| failure("daily-expected", "missing"))?;
    let initial = read_json(&expected_dir.join("expected-facts.json"))?;
    let send = args.phase == "review-send";
    let reopen = args.phase == "review-reopen";
    if !reopen {
        require(facts(&host)? == initial, "daily-initial-unreviewed")?;
        if send { review_once(&host, fixture)?; daily_facts(&host, fixture, &initial)?; }
        let ca = checked(fs::read(args.fixture.with_file_name("ca.der")), "fixture-ca")?;
        run_daily(&host, config, fixture, &ca)?;
        settle(&host)?;
    }
    require(round_witness(&host)? == witness, "retained-binding-epoch-settings")?;
    let actual = daily_facts(&host, fixture, &initial)?;
    if !send {
        let expected = read_json(&expected_dir.join("expected-daily.json"))?;
        if args.phase == "review-receive" && actual != expected { daily_equality_diagnostic(&actual, &expected); }
        require(actual == expected, "exact-daily-row-and-media-equality")?;
    }
    if reopen { require(read_json(&args.root.join("daily-witness.json"))? == witness, "persisted-daily-witness")?; }
    checked(host.close(), "daily-close")?;
    drop(host);
    if send { write_json(&args.output.join("expected-daily.json"), &actual)?; }
    if args.phase == "review-receive" { write_json(&args.root.join("daily-witness.json"), &witness)?; }
    let checks = if send { vec!["cold-ready", "original-binding-epoch-settings", "initial-unreviewed", "fsrs-ok", "rendered-question-and-answer", "good-answer-ended", "one-good-revlog", "second-card-unreviewed", "media-replaced", "owned-collection-merged", "owned-media-complete", "retained-binding-epoch-settings", "exported-full-daily-facts", "closed"] }
        else if reopen { vec!["cold-ready", "original-binding-epoch-settings", "persisted-daily-witness", "fsrs-ok", "no-active-review", "exact-daily-row-and-media-equality", "closed"] }
        else { vec!["cold-ready", "original-binding-epoch-settings", "initial-unreviewed", "owned-collection-merged", "owned-media-complete", "retained-binding-epoch-settings", "fsrs-ok", "one-good-revlog", "second-card-unreviewed", "exact-daily-row-and-media-equality", "closed"] };
    write_json(&args.output.join("result.json"), &json!({"format":1,"phase":args.phase,"status":"pass",
        "checks":checks,"counts":{"notes":2,"cards":2,"media":2,"revlogs":1,"reps":1}}))
}
fn execute(args: &Args) -> Result<()> {
    require(checked(fs::read_dir(&args.output), "output-directory")?.next().is_none(), "fresh-output")?;
    let fixture: Fixture = checked(serde_json::from_slice(&checked(fs::read(&args.fixture), "fixture-input")?), "fixture-format")?;
    require(fixture.format == 1 && fixture.origin == "https://localhost:18443" && fixture.now_ms > 0, "fixture-contract")?;
    let upload = args.phase == "upload";
    if matches!(args.phase.as_str(), "upload" | "download") { require(checked(fs::read_dir(&args.root), "client-root")?.next().is_none(), "fresh-client")?; }
    let config = config(args, &fixture)?;
    if args.phase.starts_with("review-") { return execute_daily(args, &fixture, &config); }
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
    let expected = if upload {
        seed(&host)?;
        facts(&host)?; // Exact generated two-New-card/no-history admission.
        if args.review_round {
            checked(fsrs_policy::enable_fsrs(&checked(host.collection(), "enable-fixture-collection")?, &Default::default()), "enable-fixture-fsrs")?;
            fsrs_ok(&host)?;
        }
        facts(&host)? // Explicit enable must preserve all original New/history checks.
    } else {
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
    let round = if args.review_round { fsrs_ok(&host)?; Some(round_witness(&host)?) } else { None };
    checked(host.close(), "close")?;
    drop(host);
    drop(process);
    if args.phase != "reopen" && let Some(round) = round { write_json(&args.root.join("round-witness.json"), &round)?; }
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
