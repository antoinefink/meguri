//! The synthetic collection generator (U11, R1804, R1805): seeded, valid
//! collections whose shape follows a [`Profile`], the worst-case shapes,
//! and the small named UI fixtures ([`recipe`]).
//!
//! Every row goes through meguri-storage's storage-only `CollectionBuilder`
//! and meguri-model's records, in `Raw` scopes (rows as given, historical
//! ids and stamps); the generator issues no SQL of its own and needs no
//! executor. Randomness comes from the seam through [`rng`] (one ChaCha
//! stream per part), time from the request's pinned instant, so the same
//! request always gives the same rows: [`dump::logical_sha256`] is the
//! check, recorded in the manifest.
//!
//! The output directory holds `collection.anki2`, `collection.media/` and
//! `manifest.json`.

pub mod cards;
pub mod decks;
pub mod dump;
pub mod media;
pub mod notes;
pub mod notetypes;
pub mod profile;
pub mod recipe;
pub mod revlog;
pub mod rng;
pub mod worst_case;

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::col::Col;
use meguri_model::config::ConfigEntry;
use meguri_model::rng::SeededRandom;
use meguri_model::tag::Tag;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::{CollectionBuilder, OpenOptions, ReadRows, Record, StampMode, StorageError, Table, WriteScope};
use serde::{Deserialize, Serialize};

pub use profile::{Histogram, Profile};
pub use worst_case::WorstCase;

use cards::Target;
use rng::{Rng, Stream};

/// Bumped whenever the same request would give other rows; cached outputs
/// carry it in their manifest. Version 4 is `fixtures/synthetic-collections`
/// as specified (F10.4): a field's drawn length is its whole stored length,
/// markup included, and media references are placed by field, only in
/// fields long enough to hold them (F4.8, F3.2).
pub const GENERATOR_VERSION: u32 = 4;

/// The default pinned instant: 2030-01-15T10:00:00Z, the scenarios' clock.
pub const DEFAULT_NOW_MS: i64 = 1_894_701_600_000;

/// `now` must be before this: 2038-01-19T03:14:07Z minus one day, so every
/// `due` in epoch seconds fits `i32` (F4.3).
pub const LATEST_NOW_MS: i64 = (2_147_483_647 - 86_400) * 1000;

/// The desired retention every generated preset uses and every card was
/// scheduled with.
pub const DESIRED_RETENTION: f32 = 0.9;

/// Notes per write transaction.
const CHUNK_NOTES: usize = 2000;

/// The most notes a request may need: positions stay below 1,000,000
/// (F6.6).
pub const MAX_NOTES: u64 = 999_999;

#[derive(Debug)]
pub enum GenError {
    Io {
        path: PathBuf,
        detail: String,
    },
    Storage(String),
    Profile(String),
    Recipe(String),
    Request(String),
    /// fsrs refused a generated history (F5.3): a generator bug, never
    /// patched over.
    Fsrs(String),
}

impl GenError {
    pub(crate) fn io(path: &Path, e: impl fmt::Display) -> GenError {
        GenError::Io {
            path: path.to_path_buf(),
            detail: e.to_string(),
        }
    }

    pub(crate) fn storage(e: StorageError) -> GenError {
        GenError::Storage(e.to_string())
    }

    pub(crate) fn open(e: impl fmt::Display) -> GenError {
        GenError::Storage(e.to_string())
    }
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::Io { path, detail } => write!(f, "{}: {detail}", path.display()),
            GenError::Storage(detail) => write!(f, "storage: {detail}"),
            GenError::Profile(detail) => write!(f, "profile: {detail}"),
            GenError::Recipe(detail) => write!(f, "recipe: {detail}"),
            GenError::Request(detail) => write!(f, "request: {detail}"),
            GenError::Fsrs(detail) => write!(f, "fsrs refused a history: {detail}"),
        }
    }
}

impl std::error::Error for GenError {}

/// Whether media files are written or only referenced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaMode {
    /// Fields reference media; no file is written (sizes at 100k cards and
    /// above would reach tens of gigabytes).
    References,
    /// Every referenced file is written, with deterministic bytes of its
    /// sampled size; images are valid PNGs.
    Files,
}

/// What to generate.
#[derive(Clone, Debug)]
pub struct Request {
    pub profile: Profile,
    /// Cards wanted; the result has about this many (the cards per note of
    /// cloze note types are drawn).
    pub cards: u64,
    pub seed: u64,
    /// The pinned "now": every id and stamp is at or before it.
    pub now_ms: i64,
    pub media: MediaMode,
    pub worst_case: Option<WorstCase>,
}

impl Request {
    pub fn new(profile: Profile, cards: u64, seed: u64) -> Request {
        Request {
            profile,
            cards,
            seed,
            now_ms: DEFAULT_NOW_MS,
            media: MediaMode::References,
            worst_case: None,
        }
    }
}

/// `10k`, `100k`, `150k`, `500k`, `1m`, or a number of cards.
pub fn parse_size(text: &str) -> Option<u64> {
    let lower = text.trim().to_ascii_lowercase();
    let (digits, factor) = if let Some(d) = lower.strip_suffix('k') {
        (d, 1_000)
    } else if let Some(d) = lower.strip_suffix('m') {
        (d, 1_000_000)
    } else {
        (lower.as_str(), 1)
    };
    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(factor))
        .filter(|&n| n > 0)
}

/// Rows written, for the manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub notes: u64,
    pub cards: u64,
    pub revlog: u64,
    pub notetypes: u64,
    pub decks: u64,
    pub presets: u64,
    pub tags: u64,
    pub media_refs: u64,
    pub media_files: u64,
    pub media_bytes: u64,
}

/// `manifest.json` of an output directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub generator_version: u32,
    /// `profile`, `worst-case` or `recipe`.
    pub kind: String,
    /// The worst case or recipe name; the size for a profile run.
    pub name: String,
    pub seed: u64,
    pub cards_requested: u64,
    pub now_ms: i64,
    /// The UTC offset the collection was written in, seconds east of UTC
    /// (F10.2): the oracle check picks its zone from it.
    pub utc_offset_secs: i32,
    pub profile_sha256: Option<String>,
    /// [`WorstCase::sha256`] of a worst case's definition; `None` for
    /// profiles and recipes. An output is reused only for the same
    /// definition, so an edited stress value makes it again.
    pub worst_case_sha256: Option<String>,
    pub media: MediaMode,
    pub counts: Counts,
    /// [`dump::logical_sha256`] of the collection.
    pub dump_sha256: String,
}

impl Manifest {
    pub fn load(dir: &Path) -> Result<Manifest, GenError> {
        let path = dir.join("manifest.json");
        let text = std::fs::read_to_string(&path).map_err(|e| GenError::io(&path, e))?;
        serde_json::from_str(&text).map_err(|e| GenError::io(&path, e))
    }

    fn write(&self, dir: &Path) -> Result<(), GenError> {
        let path = dir.join("manifest.json");
        let text = crate::observe::golden_text(&serde_json::to_value(self).expect("a manifest serializes"));
        std::fs::write(&path, text).map_err(|e| GenError::io(&path, e))
    }
}

/// `core/crates/meguri-testkit/profiles`.
pub fn profiles_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("profiles")
}

/// The committed shape of the user's collection, `profiles/user-shape.json`.
pub fn user_shape() -> Result<Profile, GenError> {
    Profile::load(&profiles_dir().join("user-shape.json"))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The directory holding `dir` (`.` for a bare name) and `dir`'s own name.
/// A run's lock, staging and trash directories sit there, named
/// `.<name>.…`, so they share `dir`'s file system and a rename publishes.
fn parent_and_name(dir: &Path) -> Result<(PathBuf, String), GenError> {
    let name = dir
        .file_name()
        .ok_or_else(|| GenError::io(dir, "an output directory needs a name"))?
        .to_string_lossy()
        .into_owned();
    let parent = match dir.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&parent).map_err(|e| GenError::io(&parent, e))?;
    Ok((parent, name))
}

/// An exclusive lock on an output directory, held from a cache check to
/// the end of the generation it decides on, so runs into one directory
/// take turns. It is the file `.<name>.lock` next to the directory, which
/// stays; the lock is released when this is dropped (or the process ends).
#[derive(Debug)]
pub struct OutputLock {
    _file: std::fs::File,
}

impl OutputLock {
    /// Waits until no other holder has `dir`'s lock, then takes it.
    pub fn acquire(dir: &Path) -> Result<OutputLock, GenError> {
        let (parent, name) = parent_and_name(dir)?;
        let path = parent.join(format!(".{name}.lock"));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| GenError::io(&path, e))?;
        file.lock().map_err(|e| GenError::io(&path, e))?;
        Ok(OutputLock { _file: file })
    }
}

/// Replaces `dir` with the staged output in one rename. A `dir` already
/// there is first moved into a trash directory beside it, put back if the
/// rename fails, and deleted after it succeeds. Readers therefore find
/// one run's whole output or, for that instant, none.
fn publish(stage: tempfile::TempDir, dir: &Path) -> Result<(), GenError> {
    let (parent, name) = parent_and_name(dir)?;
    let staged = stage.keep();
    let discard = |e: GenError| {
        let _ = std::fs::remove_dir_all(&staged);
        e
    };
    let trash = if std::fs::symlink_metadata(dir).is_ok() {
        let trash = tempfile::Builder::new()
            .prefix(&format!(".{name}.old-"))
            .tempdir_in(&parent)
            .map_err(|e| discard(GenError::io(&parent, e)))?;
        std::fs::rename(dir, trash.path().join(&name)).map_err(|e| discard(GenError::io(dir, e)))?;
        Some(trash)
    } else {
        None
    };
    if let Err(e) = std::fs::rename(&staged, dir) {
        if let Some(trash) = &trash {
            let _ = std::fs::rename(trash.path().join(&name), dir);
        }
        return Err(discard(GenError::io(dir, e)));
    }
    // Dropping the trash deletes the previous output.
    drop(trash);
    Ok(())
}

/// The day structure of a generated collection: `col.crt` at the rollover
/// hour of the first day, `today` the day number of the pinned instant.
#[derive(Clone, Copy, Debug)]
pub struct Timeline {
    pub now_ms: i64,
    pub offset_secs: i64,
    pub rollover_hour: i64,
    /// `col.crt`, epoch seconds.
    pub crt: i64,
    /// Day number of `now_ms`.
    pub today: i64,
}

impl Timeline {
    pub fn new(now_ms: i64, offset_secs: i32, rollover_hour: u8, span_days: u64) -> Timeline {
        let offset = i64::from(offset_secs);
        let rollover = i64::from(rollover_hour);
        let local = now_ms.div_euclid(1000) + offset - rollover * 3600;
        let today_start = local.div_euclid(86_400) * 86_400 + rollover * 3600 - offset;
        let span = i64::try_from(span_days).unwrap_or(i64::MAX / 2).min(36_500);
        Timeline {
            now_ms,
            offset_secs: offset,
            rollover_hour: rollover,
            crt: today_start - span * 86_400,
            today: span,
        }
    }

    pub fn crt_ms(&self) -> i64 {
        self.crt * 1000
    }

    /// The day number holding an instant.
    pub fn day_of(&self, ms: i64) -> i64 {
        (ms.div_euclid(1000) - self.crt).div_euclid(86_400)
    }

    /// An instant in day `day` at local hour `hour`, uniform within the
    /// hour; `None` when it falls after now.
    pub fn instant(&self, day: i64, hour: i64, rng: &mut Rng) -> Option<i64> {
        let into_day = (hour - self.rollover_hour).rem_euclid(24) * 3600 + rng.below(3600) as i64;
        let ms = (self.crt + day * 86_400 + into_day) * 1000 + rng.below(1000) as i64;
        (ms <= self.now_ms).then_some(ms)
    }
}

/// Ids taken in one table. A candidate that is taken moves up by one until
/// it is free (F4.7, the generator's own rule).
#[derive(Debug, Default)]
pub struct Ids(HashSet<i64>);

impl Ids {
    pub fn claim(&mut self, candidate: i64) -> i64 {
        let mut id = candidate;
        while !self.0.insert(id) {
            id += 1;
        }
        id
    }
}

/// The rows of one note, ready to write.
#[derive(Debug, Default)]
pub struct NoteRows {
    pub note: Option<meguri_model::note::Note>,
    pub cards: Vec<meguri_model::card::Card>,
    pub revlog: Vec<meguri_model::revlog::RevlogEntry>,
    pub media: Vec<media::MediaFile>,
    pub tags: Vec<String>,
}

/// Rows waiting for the next write transaction.
#[derive(Default)]
pub(crate) struct Pending {
    notes: Vec<meguri_storage::RowImage>,
    cards: Vec<meguri_storage::RowImage>,
    revlog: Vec<meguri_storage::RowImage>,
    notes_in_batch: usize,
}

/// The output being written: the builder, the clock its scopes stamp
/// with, counts and the tags seen.
pub(crate) struct Output<'a> {
    pub builder: CollectionBuilder,
    /// Where the collection is built: the system's temporary directory.
    /// Random-key inserts into a file on a shared mount (Docker Desktop's
    /// bind mounts) cost a round trip per page, which made 1M cards take
    /// longer than their budget; the finished file is moved into `stage`.
    work: tempfile::TempDir,
    /// The output directory being assembled: `.<name>.staging-…` next to
    /// `dir`, renamed to `dir` once whole ([`Output::finish`]). Until then
    /// `dir` keeps the previous output; a failed run deletes the stage.
    stage: tempfile::TempDir,
    pub clock: FixedClock,
    pub dir: &'a Path,
    /// `collection.media` in the stage.
    pub media_dir: PathBuf,
    pub media: MediaMode,
    pub counts: Counts,
    /// Distinct tags on notes, in `unicase` order, spelled as first seen.
    tags: BTreeSet<UnicaseTag>,
    /// `tags.collapsed` (F6.7).
    tags_collapsed: bool,
    pending: Pending,
}

/// A tag ordered and compared by `unicase` (`text/unicode-primitives` U2),
/// as the `tags` table's key is.
#[derive(Debug)]
pub(crate) struct UnicaseTag(pub(crate) String);

impl PartialEq for UnicaseTag {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for UnicaseTag {}

impl PartialOrd for UnicaseTag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UnicaseTag {
    fn cmp(&self, other: &Self) -> Ordering {
        meguri_text::fold::compare_folded(&self.0, &other.0)
    }
}

impl<'a> Output<'a> {
    /// Creates the collection with its `col` row and config rows; with its
    /// media and manifest it replaces `dir` in [`Output::finish`], and `dir`
    /// is left as it was until then. `collapsed` is the tag rows'
    /// `collapsed` (F6.7).
    pub fn create(
        dir: &'a Path,
        timeline: &Timeline,
        config: &[ConfigEntry],
        media: MediaMode,
        collapsed: bool,
    ) -> Result<Output<'a>, GenError> {
        let (parent, name) = parent_and_name(dir)?;
        let stage = tempfile::Builder::new()
            .prefix(&format!(".{name}.staging-"))
            .tempdir_in(&parent)
            .map_err(|e| GenError::io(&parent, e))?;
        let work = tempfile::Builder::new()
            .prefix("meguri-generate-")
            .tempdir()
            .map_err(|e| GenError::io(&std::env::temp_dir(), e))?;
        let media_dir = stage.path().join("collection.media");
        std::fs::create_dir(&media_dir).map_err(|e| GenError::io(&media_dir, e))?;
        let clock = FixedClock::new(
            timeline.now_ms,
            UtcOffset::from_seconds(timeline.offset_secs as i32).unwrap_or(UtcOffset::UTC),
        );
        let random = SeededRandom::new(0);
        let col = Col {
            version: Col::SCHEMA_VERSION,
            created: EpochSecs(timeline.crt),
            modified: EpochMillis(timeline.now_ms),
            schema_modified: EpochMillis(timeline.crt_ms()),
            last_sync: EpochMillis(0),
            usn: Usn(0),
            dirty: 0,
            legacy: Default::default(),
        };
        let building = work.path().join("collection.anki2");
        let builder = CollectionBuilder::create(&building, &col, config, &OpenOptions::new(&clock, &random))
            .map_err(GenError::storage)?;
        Ok(Output {
            builder,
            work,
            stage,
            clock,
            dir,
            media_dir,
            media,
            counts: Counts::default(),
            tags: BTreeSet::new(),
            tags_collapsed: collapsed,
            pending: Pending::default(),
        })
    }

    /// Runs `f` in one `Raw` scope and commits it.
    pub fn scope<T>(&self, f: impl FnOnce(&mut WriteScope<'_>) -> Result<T, StorageError>) -> Result<T, GenError> {
        let mut scope = self
            .builder
            .begin(StampMode::Raw, &self.clock)
            .map_err(GenError::storage)?;
        let value = f(&mut scope).map_err(GenError::storage)?;
        scope.commit().map_err(GenError::storage)?;
        Ok(value)
    }

    /// Queues a note's rows (and writes its media files); writes a batch
    /// when it is full.
    pub fn push(&mut self, rows: NoteRows) -> Result<(), GenError> {
        if let Some(note) = &rows.note {
            self.pending.notes.push(note.to_cells());
            self.counts.notes += 1;
        }
        for card in &rows.cards {
            self.pending.cards.push(card.to_cells());
        }
        for entry in &rows.revlog {
            self.pending.revlog.push(entry.to_cells());
        }
        self.counts.cards += rows.cards.len() as u64;
        self.counts.revlog += rows.revlog.len() as u64;
        for file in &rows.media {
            self.counts.media_refs += 1;
            if self.media == MediaMode::Files {
                media::write_file(&self.media_dir, file)?;
                self.counts.media_files += 1;
                self.counts.media_bytes += file.bytes;
            }
        }
        for tag in rows.tags {
            // `insert` keeps the spelling already there.
            self.tags.insert(UnicaseTag(tag));
        }
        self.pending.notes_in_batch += 1;
        if self.pending.notes_in_batch >= CHUNK_NOTES {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), GenError> {
        let mut pending = std::mem::take(&mut self.pending);
        if pending.notes.is_empty() && pending.cards.is_empty() && pending.revlog.is_empty() {
            return Ok(());
        }
        // In id order, so the batch's inserts touch neighbouring pages.
        for rows in [&mut pending.notes, &mut pending.cards, &mut pending.revlog] {
            rows.sort_unstable_by_key(|row| row[0].as_i64());
        }
        self.scope(|scope| {
            scope.insert_rows(Table::Notes, &pending.notes)?;
            scope.insert_rows(Table::Cards, &pending.cards)?;
            scope.insert_rows(Table::Revlog, &pending.revlog)?;
            Ok(())
        })
    }

    /// Writes the tag rows, the closing config rows and the manifest,
    /// closes the collection and publishes the whole output at `dir`.
    pub fn finish(
        mut self,
        closing_config: &[ConfigEntry],
        manifest: impl FnOnce(Counts) -> Manifest,
    ) -> Result<Manifest, GenError> {
        self.flush()?;
        let tags: Vec<_> = self
            .tags
            .iter()
            .map(|UnicaseTag(name)| {
                Tag {
                    name: name.clone(),
                    usn: Usn::PENDING,
                    collapsed: self.tags_collapsed,
                    config: None,
                }
                .to_cells()
            })
            .collect();
        self.counts.tags = tags.len() as u64;
        self.scope(|scope| {
            scope.insert_rows(Table::Tags, &tags)?;
            for entry in closing_config {
                scope.put_config_entry(entry)?;
            }
            Ok(())
        })?;
        let counts = self.counts.clone();
        let built = self.builder.finish().map_err(GenError::storage)?;
        let mut manifest = manifest(counts);
        manifest.dump_sha256 = dump::logical_sha256(&built)?;
        let staged = self.stage.path().join("collection.anki2");
        for suffix in ["", "-wal"] {
            let from = sibling(&built, suffix);
            if from.exists() {
                move_file(&from, &sibling(&staged, suffix))?;
            }
        }
        drop(self.work);
        manifest.write(self.stage.path())?;
        publish(self.stage, self.dir)?;
        Ok(manifest)
    }
}

/// Renames `from` to `to`, or copies it when they are on different file
/// systems.
fn move_file(from: &Path, to: &Path) -> Result<(), GenError> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to).map_err(|e| GenError::io(to, e))?;
    std::fs::remove_file(from).map_err(|e| GenError::io(from, e))
}

/// A config row as the generator writes it: JSON text, pending usn, the
/// collection's creation second.
pub(crate) fn config_entry(key: &str, json: &str, timeline: &Timeline) -> ConfigEntry {
    ConfigEntry {
        key: key.to_owned(),
        usn: Usn::PENDING,
        mtime: EpochSecs(timeline.crt),
        value: json.as_bytes().to_vec(),
    }
}

/// The config rows a generated collection starts with (F6.2): the keys of
/// a new collection, FSRS and the load balancer on (R1804), the rollover
/// hour, and `lastUnburied` today, which keeps buried cards buried at now.
/// `curModel` and `nextPos` come with [`closing_config`].
pub(crate) fn base_config(timeline: &Timeline) -> Vec<ConfigEntry> {
    let creation_offset = -timeline.offset_secs / 60;
    vec![
        config_entry("activeDecks", "[1]", timeline),
        config_entry("addToCur", "true", timeline),
        config_entry("collapseTime", "1200", timeline),
        config_entry("creationOffset", &creation_offset.to_string(), timeline),
        config_entry("curDeck", "1", timeline),
        config_entry("dayLearnFirst", "false", timeline),
        config_entry("dueCounts", "true", timeline),
        config_entry("estTimes", "true", timeline),
        config_entry("fsrs", "true", timeline),
        config_entry("lastUnburied", &timeline.today.to_string(), timeline),
        config_entry("loadBalancerEnabled", "true", timeline),
        config_entry("newSpread", "0", timeline),
        config_entry("rollover", &timeline.rollover_hour.to_string(), timeline),
        config_entry("sched2021", "true", timeline),
        config_entry("schedVer", "2", timeline),
        config_entry("sortBackwards", "false", timeline),
        config_entry("sortType", "\"noteFld\"", timeline),
        config_entry("timeLim", "0", timeline),
    ]
}

/// The config rows written last (F6.2): the first note type, and the next
/// new-card position.
pub(crate) fn closing_config(timeline: &Timeline, first_notetype: i64, notes: u64) -> Vec<ConfigEntry> {
    vec![
        config_entry("curModel", &first_notetype.to_string(), timeline),
        config_entry("nextPos", &(notes + 1).to_string(), timeline),
    ]
}

/// Generates a collection into `dir`, replacing what is there. The output
/// is assembled beside `dir` and published whole by a rename, so a failed
/// run leaves `dir` as it was. Runs that may overlap on one `dir` hold its
/// [`OutputLock`] from their cache check to the end.
pub fn generate(request: &Request, dir: &Path) -> Result<Manifest, GenError> {
    request.profile.check()?;
    if request.cards == 0 {
        return Err(GenError::Request("no cards requested".to_owned()));
    }
    let profile = &request.profile;
    let worst = request.worst_case.as_ref();
    let media = match worst {
        Some(w) if w.media_files.is_some() => MediaMode::Files,
        _ => request.media,
    };
    let span = profile.totals.span_days.max(1);
    let timeline = Timeline::new(request.now_ms, profile.utc_offset_secs, profile.rollover_hour, span);
    if timeline.now_ms >= LATEST_NOW_MS {
        return Err(GenError::Request(format!(
            "now {} ms is too late: every due must fit i32 (F4.3)",
            timeline.now_ms
        )));
    }

    let scale = request.cards as f64 / profile.totals.cards.max(1) as f64;
    let mut structure = Rng::stream(request.seed, Stream::Structure, 0);
    let mut ids = AllIds::default();
    let presets = decks::presets(profile, worst, &timeline, &mut ids.presets);
    let deck_plan = decks::decks(profile, worst, &presets, &timeline, &mut ids.decks, &mut structure);
    let types = notetypes::build(profile, worst, &timeline, &mut ids.notetypes, &mut structure);
    let tag_pool = notes::TagPool::build(profile, worst, scale, &mut structure);
    let assignment = notes::assign_notetypes(profile, &types, request.cards, scale, &mut structure);
    if assignment.len() as u64 > MAX_NOTES {
        return Err(GenError::Request(format!(
            "{} notes: positions must stay below 1,000,000, so at most {MAX_NOTES} notes (F6.6)",
            assignment.len()
        )));
    }

    let mut out = Output::create(dir, &timeline, &base_config(&timeline), media, true)?;
    out.counts.presets = presets.len() as u64;
    out.counts.decks = deck_plan.decks.len() as u64;
    out.counts.notetypes = types.len() as u64;
    out.scope(|scope| {
        for preset in &presets {
            scope.put(preset)?;
        }
        for deck in &deck_plan.decks {
            scope.put(&deck.record)?;
        }
        for nt in &types {
            scope.put_notetype(&nt.record)?;
        }
        Ok(())
    })?;

    let fsrs = revlog::Fsrs::new();
    let ctx = notes::NoteContext {
        profile,
        worst,
        timeline: &timeline,
        types: &types,
        decks: &deck_plan,
        tags: &tag_pool,
        fsrs: &fsrs,
        seed: request.seed,
        total_notes: assignment.len() as u64,
    };
    for (index, &nt) in assignment.iter().enumerate() {
        let rows = notes::generate_note(&ctx, index as u64, nt, &mut ids)?;
        out.push(rows)?;
    }

    let closing = closing_config(&timeline, types[0].record.id.0, assignment.len() as u64);
    let (kind, name) = match worst {
        Some(w) => ("worst-case", w.name.clone()),
        None => ("profile", request.cards.to_string()),
    };
    let profile_sha256 = profile.sha256();
    out.finish(&closing, |counts| Manifest {
        generator_version: GENERATOR_VERSION,
        kind: kind.to_owned(),
        name,
        seed: request.seed,
        cards_requested: request.cards,
        now_ms: request.now_ms,
        utc_offset_secs: profile.utc_offset_secs,
        profile_sha256: Some(profile_sha256),
        worst_case_sha256: worst.map(WorstCase::sha256),
        media,
        counts,
        dump_sha256: String::new(),
    })
}

/// The ids of the notes of the collection at `path` that hold a field of at
/// least `bytes` UTF-8 bytes, in id order: the large-field worst case's
/// notes, which the oracle collection check renders (F11).
pub fn notes_with_field_of(path: &Path, bytes: u64) -> Result<Vec<i64>, GenError> {
    let clock = FixedClock::new(0, UtcOffset::UTC);
    let random = SeededRandom::new(0);
    let collection =
        meguri_storage::open_collection(path, &OpenOptions::new(&clock, &random)).map_err(GenError::open)?;
    let rows = collection.writer();
    let mut out = Vec::new();
    let mut after = None;
    loop {
        let page = rows.notes_after(after, 1000).map_err(GenError::storage)?;
        let Some(last) = page.last() else { break };
        after = Some(last.id);
        out.extend(
            page.iter()
                .filter(|note| note.field_values().any(|f| f.len() as u64 >= bytes))
                .map(|note| note.id.0),
        );
    }
    collection.close().map_err(GenError::storage)?;
    Ok(out)
}

/// The id sets of the tables the generator allocates ids in.
#[derive(Debug, Default)]
pub struct AllIds {
    pub notes: Ids,
    pub cards: Ids,
    pub revlog: Ids,
    pub decks: Ids,
    pub presets: Ids,
    pub notetypes: Ids,
}

/// The target state drawn for a card from the profile's card state mix.
pub(crate) fn draw_target(profile: &Profile, rng: &mut Rng) -> Target {
    match profile::sample_mix(&profile.cards.states, rng).unwrap_or("new") {
        "learning" => Target::Learning,
        "relearning" => Target::Relearning,
        "review" => Target::Review(cards::Due::Any),
        "suspended" => Target::Suspended,
        "buried" => Target::Buried,
        _ => Target::New,
    }
}
