//! F11.2 delivery/persistence expectations, derived BEFORE native launch.
//! This deliberately trusts existing Rust import/review semantics. Backend,
//! parity, recovery and native proof remain separate requirements.
use meguri_engine::{
    CancelToken, EventKind, JobKind, ProgressSink,
    session::{AnswerOutcome, StartResult},
};
use meguri_host::{CoreHost, HostConfig, ProcessCore};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::{CardId, DeckId},
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_package::{CollectionPackageWriteContext, Finalisation, entry};
use meguri_storage::{
    ReadRows, ReaderRole, Table,
    atomic_file::{self, Purpose},
    space::{SpaceProvider, StatvfsSpace},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const NOW: i64 = 1_894_698_000_000;
const START: &str = "2030-01-15T10:00:00+01:00";
const MEDIA: [&str; 3] = ["_meguri-unused.txt", "meguri-review.png", "meguri-review.wav"];
const LIMIT: u64 = 8 * 1024 * 1024;
fn require(ok: bool, reason: &str) -> Result<()> {
    if ok { Ok(()) } else { Err(reason.to_owned().into()) }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub sha256: String,
    pub bytes: u64,
}
fn identities<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<BTreeMap<String, Identity>, D::Error> {
    struct Unique;
    impl<'de> serde::de::Visitor<'de> for Unique {
        type Value = BTreeMap<String, Identity>;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("unique file identities")
        }
        fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> std::result::Result<Self::Value, M::Error> {
            let mut out = BTreeMap::new();
            while let Some((name, value)) = map.next_entry::<String, Identity>()? {
                if out.insert(name, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate file identity"));
                }
                if out.len() > 18 {
                    return Err(serde::de::Error::custom("file identity bound"));
                }
            }
            Ok(out)
        }
    }
    d.deserialize_map(Unique)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundFile {
    pub path: String,
    pub identity: Identity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seeds {
    pub source_open: u64,
    pub package_write: u64,
    /// One SeededRandom retained across inspect_entry + import_initial, as
    /// start_collection_inspection retains its HostConfig for activation.
    pub inspection_and_initial_activation: u64,
    pub reopen: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wall {
    pub now: String,
    pub tz: String,
    pub now_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub deck_id: i64,
    pub deck_name: String,
    pub card_id: i64,
    #[serde(deserialize_with = "required_resume")]
    pub resume_card_id: Option<i64>,
}
fn required_resume<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Option<i64>, D::Error> {
    Option::<i64>::deserialize(d)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    InspectAndActivateInitial,
    SelectDeck,
    StartSession,
    RevealAnswer,
    AnswerEasy,
    EndSession,
    Close,
    Reopen,
    ExportAllMedia,
}
fn actions() -> Vec<Action> {
    vec![
        Action::InspectAndActivateInitial,
        Action::SelectDeck,
        Action::StartSession,
        Action::RevealAnswer,
        Action::AnswerEasy,
        Action::EndSession,
        Action::Close,
        Action::Reopen,
        Action::ExportAllMedia,
    ]
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rating {
    Easy,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub rating: Rating,
    pub elapsed_ms: u64,
    pub count: u8,
    pub preset_cap_answer_time_secs: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Code {
    pub revision: String,
    pub build_profile: String,
    pub executable: Identity,
    pub replay_source: Identity,
}
/// Actual Rust schema for the later development-only native owner. All members
/// are required; no flattened maps, optional defaults, ambient seeds or clocks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema_version: u32,
    pub purpose: String,
    pub run: String,
    pub platform: String,
    pub preparation: BoundFile,
    pub parent_source: String,
    #[serde(deserialize_with = "identities")]
    pub parent_files: BTreeMap<String, Identity>,
    pub prepared_source: String,
    #[serde(deserialize_with = "identities")]
    pub prepared_files: BTreeMap<String, Identity>,
    pub package: BoundFile,
    pub generator_seed: u64,
    pub wall: Wall,
    pub seeds: Seeds,
    pub chosen: Choice,
    pub answer: Answer,
    pub no_old_database: bool,
    pub actions: Vec<Action>,
    pub code: Code,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frozen {
    schema_version: u32,
    run: String,
    platform: String,
    stage: String,
    #[serde(deserialize_with = "identities")]
    files: BTreeMap<String, Identity>,
}
// Source preparation is independently authored by the pinned decoder. Retain
// its evidence verbatim, but consume only these actual fields for replay.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preparation {
    schema_version: u32,
    operation: String,
    stage: String,
    run: String,
    platform: String,
    source: String,
    prepared_source: String,
    clock: Wall,
    generator_request: Generator,
    #[serde(deserialize_with = "identities")]
    parent_files: BTreeMap<String, Identity>,
    #[serde(deserialize_with = "identities")]
    prepared_files: BTreeMap<String, Identity>,
    source_facts: SourceFacts,
    preset_cap_answer_time_secs: u32,
    #[serde(rename = "media")]
    _media: serde_json::Value,
    #[serde(rename = "row_changes")]
    _row_changes: serde_json::Value,
    #[serde(rename = "selected_parent_deck")]
    _selected_parent_deck: serde_json::Value,
    #[serde(rename = "preserved_card")]
    _preserved_card: serde_json::Value,
    #[serde(rename = "preserved_history")]
    _preserved_history: serde_json::Value,
    #[serde(rename = "media_pattern")]
    _media_pattern: serde_json::Value,
    #[serde(rename = "derivation")]
    _derivation: serde_json::Value,
    #[serde(rename = "remaining")]
    _remaining: serde_json::Value,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generator {
    recipe: serde_json::Value,
    seed: u64,
    generator_version: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFacts {
    kind: String,
    #[serde(rename = "parent_manifest")]
    _parent_manifest: serde_json::Value,
    parent_manifest_identity: Identity,
    #[serde(rename = "note_id")]
    _note_id: i64,
    card_id: i64,
    deck_id: i64,
    deck_name: String,
    row_counts: BTreeMap<String, u64>,
    media_refs: u64,
    media_files: u64,
    media_bytes: u64,
}
fn wall() -> Wall {
    Wall {
        now: START.into(),
        tz: "Europe/Paris".into(),
        now_ms: NOW,
    }
}
fn component(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 120
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
fn platform(s: &str) -> bool {
    matches!(s, "mac" | "iphone" | "ipad")
}
fn hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn signature(m: &fs::Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
// No canonicalization fallback: check every ancestor, reject links and foreign
// owners beneath the admitted checkout. Read handles pin each file's inode.
fn scoped(root: &Path, relative: &str) -> Result<PathBuf> {
    require(
        root.is_absolute() && root.canonicalize()? == root,
        "checkout must be absolute without links",
    )?;
    let rel = Path::new(relative);
    require(
        !relative.is_empty() && rel.components().all(|c| matches!(c, Component::Normal(_))),
        "relative path required",
    )?;
    require(
        rel.components().all(|c| {
            !matches!(c.as_os_str().to_str(), Some("refs" | "oracle" | "local-data"))
                && !c.as_os_str().to_string_lossy().starts_with("similarity")
        }),
        "excluded path",
    )?;
    let mut path = root.to_owned();
    for c in rel.components() {
        path.push(c);
        let m = path.symlink_metadata()?;
        require(
            !m.file_type().is_symlink() && m.uid() == root.symlink_metadata()?.uid(),
            "link or foreign owner",
        )?;
        require(m.mode() & 0o022 == 0, "ambient writable entry")?;
    }
    Ok(path)
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let before = path.symlink_metadata()?;
    require(
        before.is_file() && before.nlink() == 1 && before.len() <= limit,
        "file shape/size/link",
    )?;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    require(signature(&f.metadata()?) == signature(&before), "file rebound")?;
    let mut bytes = Vec::new();
    (&f).take(limit + 1).read_to_end(&mut bytes)?;
    require(
        bytes.len() as u64 == before.len()
            && signature(&f.metadata()?) == signature(&before)
            && signature(&path.symlink_metadata()?) == signature(&before),
        "file changed",
    )?;
    Ok(bytes)
}
fn identity(bytes: &[u8]) -> Identity {
    Identity {
        sha256: Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect(),
        bytes: bytes.len() as u64,
    }
}
fn file_identity(path: &Path) -> Result<Identity> {
    Ok(identity(&read(path, LIMIT)?))
}
fn freeze(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?;
    f.write_all(bytes)?;
    f.set_permissions(fs::Permissions::from_mode(0o400))?;
    atomic_file::full_sync(&f)?;
    atomic_file::sync_directory(path.parent().ok_or("missing parent")?)?;
    Ok(())
}
fn json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    freeze(path, &bytes)
}
fn names(path: &Path) -> Result<Vec<String>> {
    let mut out = fs::read_dir(path)?
        .map(|e| Ok(e?.file_name().into_string().map_err(|_| "non UTF-8 entry")?))
        .collect::<Result<Vec<_>>>()?;
    out.sort();
    Ok(out)
}
fn fixture(root: &Path, dir: &str, expected: &BTreeMap<String, Identity>, with_media: bool) -> Result<()> {
    let path = scoped(root, dir)?;
    require(
        names(&path)? == ["collection.anki2", "collection.media", "manifest.json"],
        "fixture layout",
    )?;
    let media = scoped(root, &format!("{dir}/collection.media"))?;
    require(
        names(&media)?
            == if with_media {
                MEDIA.map(str::to_owned).to_vec()
            } else {
                vec![]
            },
        "media layout",
    )?;
    let keys = ["collection.anki2".to_owned(), "manifest.json".to_owned()]
        .into_iter()
        .chain(if with_media {
            MEDIA.iter().map(|n| format!("collection.media/{n}")).collect()
        } else {
            vec![]
        })
        .collect::<std::collections::BTreeSet<_>>();
    require(
        expected.keys().cloned().collect::<std::collections::BTreeSet<_>>() == keys,
        "fixture identity keys",
    )?;
    for (name, bound) in expected {
        require(
            hex(&bound.sha256, 64) && *bound == file_identity(&scoped(root, &format!("{dir}/{name}"))?)?,
            "fixture changed",
        )?;
    }
    Ok(())
}
impl Input {
    pub fn parse(bytes: &[u8], run: &str, platform_name: &str) -> Result<Self> {
        require(bytes.len() <= 128 * 1024, "input too large")?;
        let value: Self = serde_json::from_slice(bytes)?;
        value.validate(run, platform_name)?;
        Ok(value)
    }
    fn validate(&self, run: &str, platform_name: &str) -> Result<()> {
        require(
            self.schema_version == 1
                && self.purpose == "f11.2-trusted-rust-delivery-persistence"
                && self.run == run
                && platform(&self.platform)
                && self.platform == platform_name
                && self.wall == wall()
                && self.no_old_database
                && self.actions == actions(),
            "unadmitted input/action/run",
        )?;
        require(
            self.answer.rating == Rating::Easy
                && self.answer.count == 1
                && self.answer.elapsed_ms == 1000
                && self
                    .answer
                    .preset_cap_answer_time_secs
                    .checked_mul(1000)
                    .is_some_and(|cap| u64::from(cap) >= self.answer.elapsed_ms),
            "answer/cap",
        )?;
        require(
            self.chosen.deck_id == 1
                && self.chosen.deck_name == "Default"
                && self.chosen.card_id > 0
                && self.chosen.card_id < NOW
                && self.chosen.resume_card_id.is_none()
                && hex(&self.code.revision, 40)
                && matches!(self.code.build_profile.as_str(), "dev" | "release"),
            "choice/code",
        )?;
        require(
            self.preparation.path == format!("{run}/source-preparation/preparation.json")
                && self.prepared_source == format!("{run}/source-preparation/fixture")
                && self.package.path == format!("{run}/expected-replay/input.colpkg")
                && self.parent_source.starts_with("build/")
                && self.parent_source != "build"
                && !self.parent_source.starts_with(&format!("{run}/")),
            "input paths",
        )
    }
    pub fn load_frozen(dir: &Path, id: &str, os: &str, pin: &Identity) -> Result<Self> {
        require(component(id) && platform(os), "invalid case or platform")?;
        let run = format!("build/mg/ui-test/{id}");
        let base = format!("{run}/expected-replay");
        let data = read(&scoped(dir, &format!("{base}/freeze.json"))?, 128 * 1024)?;
        require(identity(&data) == *pin, "freeze identity differs")?;
        let (root, r, b, buf, platform_name) = (dir, run, base, data, os);
        let f: Frozen = serde_json::from_slice(&buf)?;
        require(
            f.schema_version == 1
                && f.run == r
                && f.platform == platform_name
                && f.stage == "expected-states-before-app"
                && f.files.len() == 18,
            "invalid freeze header",
        )?;
        let mut ks = vec![
            "input.json".to_owned(),
            "input.colpkg".to_owned(),
            "replay.json".to_owned(),
        ];
        for s in ["source", "import", "review"] {
            ks.extend([format!("{s}/collection.anki2"), format!("{s}/manifest.json")]);
            ks.extend(MEDIA.iter().map(|n| format!("{s}/collection.media/{n}")));
        }
        ks.sort();
        require(
            f.files.keys().cloned().collect::<Vec<_>>() == ks,
            "unexpected artifact list",
        )?;
        for (n, want) in &f.files {
            require(
                file_identity(&scoped(root, &format!("{b}/{n}"))?)? == *want,
                "artifact identity mismatch",
            )?;
        }
        let v = Self::parse(
            &read(&scoped(root, &format!("{b}/input.json"))?, 128 * 1024)?,
            &r,
            platform_name,
        )?;
        v.recheck(root, &r, platform_name)?;
        Ok(v)
    }
    /// Recheck before launch and after native evidence collection. The caller
    /// also binds input.json itself to freeze.json's SHA256/size.
    pub fn recheck(&self, root: &Path, run: &str, platform_name: &str) -> Result<()> {
        self.validate(run, platform_name)?;
        fixture(root, &self.parent_source, &self.parent_files, false)?;
        fixture(root, &self.prepared_source, &self.prepared_files, true)?;
        for bound in [&self.preparation, &self.package] {
            require(
                file_identity(&scoped(root, &bound.path)?)? == bound.identity,
                "bound file changed",
            )?;
        }
        let p: Preparation = serde_json::from_slice(&read(&scoped(root, &self.preparation.path)?, LIMIT)?)?;
        require(
            p.run == self.run
                && p.platform == self.platform
                && p.source == self.parent_source
                && p.prepared_source == self.prepared_source
                && p.parent_files == self.parent_files
                && p.prepared_files == self.prepared_files
                && p.clock == self.wall
                && p.generator_request.seed == self.generator_seed
                && p.source_facts.card_id == self.chosen.card_id
                && p.source_facts.deck_id == self.chosen.deck_id
                && p.source_facts.deck_name == self.chosen.deck_name
                && p.preset_cap_answer_time_secs == self.answer.preset_cap_answer_time_secs,
            "preparation binding",
        )
    }
}
fn config(base: &Path, collection_name: &str, seed: u64) -> HostConfig {
    HostConfig::new(
        base.join(collection_name),
        base.join("backups"),
        base.join("widget"),
        Arc::new(FixedClock::new(NOW, UtcOffset::from_seconds(3600).unwrap()).in_zone("Europe/Paris")),
        Arc::new(SeededRandom::new(seed)),
        Arc::new(StatvfsSpace),
    )
}
fn copy_fixture(root: &Path, source: &str, files: &BTreeMap<String, Identity>, target: &Path) -> Result<()> {
    fs::create_dir(target)?;
    fs::create_dir(target.join("collection.media"))?;
    for (name, bound) in files {
        let bytes = read(&scoped(root, &format!("{source}/{name}"))?, LIMIT)?;
        require(identity(&bytes) == *bound, "copy source changed")?;
        freeze(&target.join(name), &bytes)?;
    }
    // The only writable DB is the separate replay-owned copy.
    fs::set_permissions(target.join("collection.anki2"), fs::Permissions::from_mode(0o600))?;
    Ok(())
}
fn media(host: &CoreHost, root: &Path, input: &Input) -> Result<()> {
    let service = host.media()?;
    require(names(service.store().media_dir())? == MEDIA, "media names")?;
    for name in MEDIA {
        require(
            file_identity(&service.store().media_dir().join(name))?
                == input.prepared_files[&format!("collection.media/{name}")],
            "media bytes",
        )?;
    }
    {
        let store = service.store();
        let baseline = store.sync_baseline()?;
        require(
            baseline.owner.is_none()
                && baseline.usn == 0
                && store.download_intents()?.is_empty()
                && store.trash_entries()?.is_empty(),
            "media journal baseline/intents/trash",
        )?;
        let entries = store.entries()?;
        require(entries.len() == 3, "media journal count")?;
        for e in entries {
            let bytes = read(
                &scoped(root, &format!("{}/collection.media/{}", input.prepared_source, e.name))?,
                LIMIT,
            )?;
            use sha1::Digest;
            require(
                e.live
                    && !e.pending_download
                    && e.dirty == e.syncable()
                    && e.size == bytes.len() as u64
                    && e.sha1.as_deref() == Some(format!("{:x}", sha1::Sha1::digest(&bytes)).as_str())
                    && e.sync_state == meguri_storage::media::MediaSyncState::Live,
                "media journal initialization",
            )?;
        }
    }
    Ok(())
}
fn capture(host: &CoreHost, output: &Path, input: &Input, state: &str) -> Result<()> {
    fs::create_dir(output)?;
    let c = host.collection()?;
    let reader = c.reader(ReaderRole::Maintenance)?;
    let (_, size) = reader.live_snapshot_source()?;
    require(
        size <= LIMIT * 2
            && StatvfsSpace.available_bytes(output)? >= size.checked_mul(3).ok_or("snapshot budget overflow")?,
        "snapshot capacity",
    )?;
    let stage = meguri_storage::leftovers::staging_directory(c.path().parent().ok_or("collection parent")?)?;
    let snapshot = reader.snapshot_live(stage.path())?;
    drop(reader);
    freeze(&output.join("collection.anki2"), &read(&snapshot, LIMIT)?)?;
    fs::create_dir(output.join("collection.media"))?;
    for name in MEDIA {
        let data = read(&host.media()?.store().media_dir().join(name), LIMIT)?;
        require(
            identity(&data) == input.prepared_files[&format!("collection.media/{name}")],
            "snapshot media changed",
        )?;
        freeze(&output.join("collection.media").join(name), &data)?;
    }
    json(
        &output.join("manifest.json"),
        &serde_json::json!({"kind":"meguri-f11.2-rust-expected", "state":state}),
    )
}
fn history(host: &CoreHost) -> Result<Vec<meguri_model::revlog::RevlogEntry>> {
    let c = host.collection()?;
    let r = c.reader(ReaderRole::Query)?;
    let rows = r.revlog_after(None, 30)?;
    require(rows.len() < 30, "history bound")?;
    Ok(rows)
}
fn check_answer(host: &CoreHost, before: &[meguri_model::revlog::RevlogEntry], input: &Input) -> Result<()> {
    let after = history(host)?;
    let new = after
        .iter()
        .filter(|r| !before.iter().any(|old| old.id == r.id))
        .collect::<Vec<_>>();
    require(
        after.len() == before.len() + 1 && before.iter().all(|r| after.contains(r)) && new.len() == 1,
        "exact one history append",
    )?;
    let row = new[0];
    require(
        row.card_id == CardId(input.chosen.card_id) && row.id.0 == NOW && row.button == 4 && row.answer_millis == 1000,
        "actual answer values",
    )
}
/// Does not delete an uncertain import, package, backup or evidence artifact.
/// The caller must supply a fresh run; any collision is an error.
pub fn derive(
    root: &Path,
    case: &str,
    platform_name: &str,
    seeds: Seeds,
    revision: &str,
    build_profile: &str,
) -> Result<PathBuf> {
    require(
        component(case) && platform(platform_name) && hex(revision, 40) && matches!(build_profile, "dev" | "release"),
        "case/platform/code",
    )?;
    let run = format!("build/mg/ui-test/{case}");
    let run_path = scoped(root, &run)?;
    let preparation_path = format!("{run}/source-preparation/preparation.json");
    let prepared_bytes = read(&scoped(root, &preparation_path)?, LIMIT)?;
    let p: Preparation = serde_json::from_slice(&prepared_bytes)?;
    require(
        p.schema_version == 1
            && p.operation == "prepare-native-review"
            && p.stage == "source-only-before-app"
            && p.run == run
            && p.platform == platform_name
            && p.clock == wall()
            && p.generator_request.recipe
                == serde_json::json!({
            "recipe":"local-review", "description":"Four due text cards for local review, undo and process restart.",
            "now":START, "learn_ahead_secs":0, "decks":[{"name":"Local Review::Cards", "new":0,
                "learning":0, "due":4, "later":0, "tags":["local-review"],
                "fields":["Question {n}","Answer {n}"], "media":{"images":0,"sounds":0}}]})
            && p.generator_request.generator_version == 5
            && p.source_facts.kind == "meguri-f11.2-source"
            && p.source_facts.media_refs == 2
            && p.source_facts.media_files == 3
            && p.source_facts.media_bytes < 64 * 1024
            && p.source_facts.row_counts.get("notes") == Some(&1)
            && p.source_facts.row_counts.get("cards") == Some(&1)
            && p.source_facts.row_counts.get("decks") == Some(&1)
            && p.source_facts.parent_manifest_identity
                == *p.parent_files.get("manifest.json").ok_or("parent manifest")?,
        "preparation shape",
    )?;
    let output = run_path.join("expected-replay");
    // Validate every source before the first mutation. The scope/path binding
    // below is rechecked after writing the package and at the final freeze.
    let mut input = Input {
        schema_version: 1,
        purpose: "f11.2-trusted-rust-delivery-persistence".into(),
        run: run.clone(),
        platform: platform_name.into(),
        preparation: BoundFile {
            path: preparation_path,
            identity: identity(&prepared_bytes),
        },
        parent_source: p.source,
        parent_files: p.parent_files,
        prepared_source: p.prepared_source,
        prepared_files: p.prepared_files,
        package: BoundFile {
            path: format!("{run}/expected-replay/input.colpkg"),
            identity: identity(&[]),
        },
        generator_seed: p.generator_request.seed,
        wall: wall(),
        seeds,
        chosen: Choice {
            deck_id: p.source_facts.deck_id,
            deck_name: p.source_facts.deck_name,
            card_id: p.source_facts.card_id,
            resume_card_id: None,
        },
        answer: Answer {
            rating: Rating::Easy,
            elapsed_ms: 1000,
            count: 1,
            preset_cap_answer_time_secs: p.preset_cap_answer_time_secs,
        },
        no_old_database: true,
        actions: actions(),
        code: Code {
            revision: revision.into(),
            build_profile: build_profile.into(),
            executable: identity(&read(&std::env::current_exe()?, 512 * 1024 * 1024)?),
            replay_source: identity(include_bytes!("native_review.rs")),
        },
    };
    input.validate(&run, platform_name)?;
    fixture(root, &input.parent_source, &input.parent_files, false)?;
    fixture(root, &input.prepared_source, &input.prepared_files, true)?;
    let parent_manifest: serde_json::Value = serde_json::from_slice(&read(
        &scoped(root, &format!("{}/manifest.json", input.parent_source))?,
        LIMIT,
    )?)?;
    require(
        parent_manifest["seed"].as_u64() == Some(input.generator_seed)
            && parent_manifest == p.source_facts._parent_manifest,
        "actual generator request binding",
    )?;
    fs::create_dir(&output)?;
    atomic_file::sync_directory(&run_path)?;
    let work = output.join("work");
    fs::create_dir(&work)?;
    copy_fixture(
        root,
        &input.prepared_source,
        &input.prepared_files,
        &output.join("source"),
    )?;
    fs::set_permissions(
        output.join("source/collection.anki2"),
        fs::Permissions::from_mode(0o400),
    )?;
    copy_fixture(
        root,
        &input.prepared_source,
        &input.prepared_files,
        &work.join("source"),
    )?;
    let source_config = config(&work, "source", input.seeds.source_open);
    let source = CoreHost::open(source_config.clone())?;
    {
        let c = source.collection()?;
        let r = c.reader(ReaderRole::Query)?;
        require(
            r.ids_after(Table::Notes, None, 2)?.len() == 1
                && r.cards_after(None, 2)?.len() == 1
                && r.deck_rows()?.len() == 1,
            "actual source shape",
        )?;
        let card = r.card(CardId(input.chosen.card_id))?.ok_or("chosen card absent")?;
        require(card.deck_id == DeckId(input.chosen.deck_id), "actual deck")?;
        let presets = r.deck_config_rows()?;
        require(
            presets.len() == 1 && presets[0].config.cap_answer_time_to_secs == input.answer.preset_cap_answer_time_secs,
            "actual preset cap",
        )?;
    }
    let package_random = SeededRandom::new(input.seeds.package_write);
    let collection = source.collection()?;
    let reader = collection.reader(ReaderRole::Maintenance)?;
    let context = CollectionPackageWriteContext {
        collection_directory: &source_config.collection_directory,
        purpose: Purpose::Export,
        finalisation: Finalisation::ExclusiveCreate,
        clock: source_config.clock.as_ref(),
        random: &package_random,
        space: source_config.disk_space_provider.as_ref(),
        is_cancelled: &|| false,
    };
    let package_path = output.join("input.colpkg");
    let plan = meguri_package::preflight_media_collection_export_from_live(&reader, &package_path, &context)?;
    let written = meguri_package::write_media_collection_export_from_live(&reader, &package_path, &context, plan)?;
    input.package.identity = file_identity(&package_path)?;
    written.source_validation.validate(&|| false)?;
    drop(reader);
    source.close()?;
    require(
        file_identity(&source_config.collection_directory.join("collection.anki2"))?
            == input.prepared_files["collection.anki2"],
        "source packaging open changed prepared database",
    )?;
    fs::set_permissions(&package_path, fs::Permissions::from_mode(0o400))?;
    input.recheck(root, &run, platform_name)?;
    json(&output.join("input.json"), &input)?;
    let options = config(&work, "target", input.seeds.inspection_and_initial_activation);
    fs::create_dir(&options.collection_directory)?;
    require(
        names(&options.collection_directory)?.is_empty(),
        "no old database or journal",
    )?;
    let process = ProcessCore::new(options.engine.clone())?;
    let inspection = entry::inspect_entry(
        process.engine(),
        &package_path,
        &options.entry_inputs(),
        CancelToken::new(),
        &ProgressSink::new(),
    )?;
    let facts = inspection.facts();
    require(
        inspection.is_initial()
            && !facts.replacing_existing
            && facts.source.notes == 1
            && facts.source.cards == 1
            && facts.media_count == 3
            && facts.media_decoded_bytes == p.source_facts.media_bytes
            && facts
                .source_sha256
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                == input.package.identity.sha256
            && identity(&read(&package_path, LIMIT)?).sha256 == input.package.identity.sha256,
        "P16 inspection facts",
    )?;
    let (tx, rx) = mpsc::channel();
    let importer = process.clone();
    let imported_options = options.clone();
    let events = process.engine().stream().subscribe();
    let job = process.engine().start_job(JobKind::ColpkgImport, move |ctx| {
        let result = importer.import_initial(
            imported_options,
            inspection,
            ctx.slot().ok_or_else(|| meguri_engine::EngineError::Internal {
                message: "missing import slot".into(),
            })?,
            &ctx.cancel,
            &ctx.progress,
        );
        tx.send(result).map_err(|_| meguri_engine::EngineError::Internal {
            message: "replay receiver gone".into(),
        })?;
        Ok(())
    })?;
    let completion = rx.recv_timeout(Duration::from_secs(60))??;
    // Retain actual host/paths even if the committed outcome is uncertain.
    let host = completion.host;
    let activated = completion.outcome?;
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(60))
            .ok_or("timed out waiting for import JobFinished")?;
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == job.id) {
            break;
        }
    }
    require(
        activated.backup.is_none()
            && activated.media.incoming == 3
            && activated.media.added == 3
            && activated.media.reused == 0
            && activated.media.trashed == 0,
        "P16 initial import outcome",
    )?;
    require(
        !options.backups_directory.exists() || names(&options.backups_directory)?.is_empty(),
        "fabricated backup",
    )?;
    media(&host, root, &input)?;
    {
        let c = host.collection()?;
        let r = c.reader(ReaderRole::Query)?;
        let col = r.col()?;
        require(
            col.modified.0 == NOW && col.schema_modified.0 == NOW,
            "actual R7 import stamps",
        )?;
    }
    let before = history(&host)?;
    require(
        before.len() as u64 == *p.source_facts.row_counts.get("revlog").ok_or("source revlog count")?,
        "import history count",
    )?;
    capture(&host, &output.join("import"), &input, "post-r7-import")?;
    meguri_engine::decks::select(&host.collection()?, DeckId(input.chosen.deck_id))?;
    let StartResult::Started { session_id, head, .. } = host
        .review()
        .start(DeckId(input.chosen.deck_id), input.chosen.resume_card_id.map(CardId))?
    else {
        return Err("review start refused".into());
    };
    let current = head.items.first().ok_or("no actual current card")?;
    require(
        current.card_id == CardId(input.chosen.card_id),
        "actual current card mismatch",
    )?;
    require(
        matches!(
            host.review().answer(
                session_id,
                current.card_id,
                meguri_sched::answer::Rating::Easy,
                current.state_token,
                EpochMillis(NOW),
                input.answer.elapsed_ms
            )?,
            AnswerOutcome::Accepted { .. }
        ),
        "answer was not Accepted",
    )?;
    host.review().end(session_id)?;
    check_answer(&host, &before, &input)?;
    host.close()?;
    let reopened = process.open(config(&work, "target", input.seeds.reopen))?;
    check_answer(&reopened, &before, &input)?;
    media(&reopened, root, &input)?;
    capture(
        &reopened,
        &output.join("review"),
        &input,
        "post-answer-end-close-reopen",
    )?;
    reopened.close()?;
    input.recheck(root, &run, platform_name)?;
    let input_id = file_identity(&output.join("input.json"))?;
    let mut artifacts = BTreeMap::new();
    artifacts.insert("input.json".to_owned(), input_id.clone());
    artifacts.insert("input.colpkg".to_owned(), input.package.identity.clone());
    for state in ["source", "import", "review"] {
        for name in ["collection.anki2".to_owned(), "manifest.json".to_owned()]
            .into_iter()
            .chain(MEDIA.iter().map(|n| format!("collection.media/{n}")))
        {
            let path = output.join(state).join(&name);
            fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
            artifacts.insert(format!("{state}/{name}"), file_identity(&path)?);
        }
    }
    json(
        &output.join("replay.json"),
        &serde_json::json!({"schema_version":1, "run":run,
        "platform":platform_name, "input":input_id, "code":input.code, "accepted_answers":1,
        "import_media":{"incoming":activated.media.incoming,"added":activated.media.added,"reused":activated.media.reused,"trashed":activated.media.trashed},
        "backup":null, "media_retention_reference":activated.media_retention_reference, "journal_initialized":true, "reopened":true,
        "scope":"trusted-existing-rust delivery/persistence derivation; backend/parity/native proof required"}),
    )?;
    artifacts.insert("replay.json".into(), file_identity(&output.join("replay.json"))?);
    json(
        &output.join("freeze.json"),
        &serde_json::json!({"schema_version":1,"run":run,"platform":platform_name,
        "stage":"expected-states-before-app", "files":artifacts}),
    )?;
    Ok(output)
}
