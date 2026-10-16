//! The anonymized clone (`specs/fixtures/anonymizer.md`, R1941, R1915): a
//! copy of a collection that holds none of its text, deck names, tag names
//! or media file names, and that Anki schedules exactly as the original.
//!
//! [`anonymize`] reads a source directory (`collection.anki2`, its `-wal`,
//! `collection.media/`) and writes the clone and its report into an output
//! directory, nothing else. Errors and the report carry kinds, counts and
//! ids, never collection text: `tools/mg` puts them where agents read them.
//! The one exception is V4's config keys, which the spec lists by name.

pub mod chars;
mod checks;
pub mod keys;
mod kind;
pub mod media;
mod names;
mod rewrite;
mod search;
pub mod text;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use meguri_model::proto::ProtoBlob;
use meguri_model::proto::notetype::NotetypeConfig;
use meguri_storage::ConfigValue;
use meguri_storage::raw::{PlainDatabase, RawValue};
use meguri_text::{checksum, fold, normalize};
use serde_json::json;

pub use keys::Salt;
use media::NameMap;
use names::{SEPARATOR, STORED_SEPARATOR, TagMap};
use rewrite::CharCounts;
use search::{SearchCounts, SearchRewriter, Targets};

/// The clone's database, media folder and report.
pub const CLONE_COLLECTION: &str = "collection.anki2";
pub const CLONE_MEDIA: &str = "collection.media";
pub const REPORT: &str = "anonymize.json";
const SOURCE_WAL: &str = "collection.anki2-wal";
/// The work directory inside the output (A14.3), deleted at the end.
const WORK: &str = ".anonymize-work";
/// `notetypes.config` `original_stock_kind` of image occlusion.
const IMAGE_OCCLUSION_KIND: i32 = 6;
/// SQLite's result code for a file that is not a database.
const SQLITE_NOTADB: i32 = 26;
/// Notes read and written per batch.
const NOTE_BATCH: i64 = 2_000;

/// A18. Messages hold no collection text; a note id where the spec names
/// one.
#[derive(Debug)]
pub enum AnonymizeError {
    Usage(String),
    OutNotEmpty,
    NoCollection,
    OpenFailed(String),
    UnsupportedSchema,
    InvalidUtf8 {
        note: i64,
    },
    Salt,
    MediaNameCollision,
    HashCollision,
    StructureChanged {
        note: i64,
    },
    /// A file-system operation failed (not an A18 kind: the run could not
    /// write its output).
    Io(String),
    /// SQLite failed on the work copy or the clone (reported as `io`).
    Sqlite(String),
}

impl AnonymizeError {
    /// The A18 kind (`io` for a failed read or write).
    pub fn kind(&self) -> &'static str {
        match self {
            AnonymizeError::Usage(_) => "usage",
            AnonymizeError::OutNotEmpty => "out_not_empty",
            AnonymizeError::NoCollection => "no_collection",
            AnonymizeError::OpenFailed(_) => "open_failed",
            AnonymizeError::UnsupportedSchema => "unsupported_schema",
            AnonymizeError::InvalidUtf8 { .. } => "invalid_utf8",
            AnonymizeError::Salt => "salt",
            AnonymizeError::MediaNameCollision => "media_name_collision",
            AnonymizeError::HashCollision => "hash_collision",
            AnonymizeError::StructureChanged { .. } => "structure_changed",
            AnonymizeError::Io(_) | AnonymizeError::Sqlite(_) => "io",
        }
    }

    /// An I/O failure: what failed and the error kind, never a path.
    pub fn io(what: &str, e: &io::Error) -> AnonymizeError {
        AnonymizeError::Io(format!("{what}: {}", e.kind()))
    }
}

impl fmt::Display for AnonymizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnonymizeError::Usage(m) => write!(f, "usage: {m}"),
            AnonymizeError::OutNotEmpty => write!(f, "out_not_empty: the output directory exists and is not empty"),
            AnonymizeError::NoCollection => write!(f, "no_collection: the source has no collection.anki2"),
            AnonymizeError::OpenFailed(m) => write!(f, "open_failed: the copy is not an SQLite database ({m})"),
            AnonymizeError::UnsupportedSchema => write!(f, "unsupported_schema: col.ver is not 18"),
            AnonymizeError::InvalidUtf8 { note } => {
                write!(
                    f,
                    "invalid_utf8: note {note}'s fields are not UTF-8 (run Check Database first)"
                )
            }
            AnonymizeError::Salt => write!(f, "salt: the salt file is not 64 lower-case hex digits"),
            AnonymizeError::MediaNameCollision => {
                write!(
                    f,
                    "media_name_collision: two media files have names equal under case folding"
                )
            }
            AnonymizeError::HashCollision => write!(f, "hash_collision: two names hash to the same clone name"),
            AnonymizeError::StructureChanged { note } => {
                write!(f, "structure_changed: a field of note {note} lost its structure")
            }
            AnonymizeError::Io(m) => write!(f, "io: {m}"),
            AnonymizeError::Sqlite(m) => write!(f, "io: SQLite: {m}"),
        }
    }
}

impl std::error::Error for AnonymizeError {}

/// A16 `media`.
#[derive(Clone, Copy, Debug, Default)]
pub struct MediaCounts {
    pub bare_names: u64,
    pub files: u64,
    pub io_images: u64,
    pub io_images_larger: u64,
    pub io_images_unknown_size: u64,
    pub kept: u64,
    pub refs: u64,
    pub refs_malformed: u64,
    pub refs_missing: u64,
    pub refs_remote: u64,
    pub skipped_entries: u64,
}

/// A17's statuses as A16 reports them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

/// `anonymize.json` (A16): counts only.
#[derive(Clone, Debug)]
pub struct Report {
    pub json: serde_json::Value,
    /// V4: `fail` when a name column, a `sqlite_stat4` sample or a media
    /// name holds a token, `warn` when only review items remain.
    pub leak: Status,
    /// V2.
    pub rows: Status,
    /// V5.
    pub media: Status,
}

impl Report {
    /// The worst of the checks.
    pub fn status(&self) -> Status {
        [self.leak, self.rows, self.media]
            .into_iter()
            .max_by_key(|s| match s {
                Status::Pass => 0,
                Status::Warn => 1,
                Status::Fail => 2,
            })
            .unwrap_or(Status::Pass)
    }

    /// The report in the goldens' text layout (D11).
    pub fn text(&self) -> String {
        crate::observe::golden_text(&self.json)
    }
}

/// Test hooks.
#[derive(Clone, Debug, Default)]
pub struct Hooks {
    /// Leaves this deck's name as the source has it (V4's failing case,
    /// A20).
    pub keep_deck_name: Option<i64>,
    /// Leaves the `sqlite_stat4` samples of the `tags` table (V4's failing
    /// case, A20).
    pub keep_tag_samples: bool,
}

/// A2.1: anonymizes the source directory `source` into `out` (which must
/// not exist or be empty, and lie outside the source) with `salt`. On
/// failure everything written under `out` is deleted.
pub fn anonymize(source: &Path, out: &Path, salt: &Salt) -> Result<Report, AnonymizeError> {
    anonymize_with(source, out, salt, &Hooks::default())
}

/// [`anonymize`] with test hooks.
pub fn anonymize_with(source: &Path, out: &Path, salt: &Salt, hooks: &Hooks) -> Result<Report, AnonymizeError> {
    let existed = check_out(source, out)?;
    if !source.join(CLONE_COLLECTION).is_file() {
        return Err(AnonymizeError::NoCollection);
    }
    fs::create_dir_all(out).map_err(|e| AnonymizeError::io("creating the output directory", &e))?;
    let result = Run::new(source, out, salt, hooks).and_then(|mut run| run.all());
    if result.is_err() {
        clean(out, existed);
    }
    let _ = fs::remove_dir_all(out.join(WORK));
    result
}

/// A2.2's checks on `--out`; returns whether it exists.
fn check_out(source: &Path, out: &Path) -> Result<bool, AnonymizeError> {
    let source = source
        .canonicalize()
        .map_err(|_| AnonymizeError::Usage("the source directory does not exist".into()))?;
    let resolved = match out.canonicalize() {
        Ok(path) => path,
        Err(_) => {
            let parent = out
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let name = out
                .file_name()
                .ok_or_else(|| AnonymizeError::Usage("the output directory needs a name".into()))?;
            parent
                .canonicalize()
                .map_err(|_| AnonymizeError::Usage("the output directory's parent does not exist".into()))?
                .join(name)
        }
    };
    if resolved.starts_with(&source) {
        return Err(AnonymizeError::Usage(
            "the output directory is the source or inside it".into(),
        ));
    }
    match fs::metadata(out) {
        Ok(meta) if meta.is_dir() => {
            let empty = fs::read_dir(out)
                .map_err(|e| AnonymizeError::io("reading the output directory", &e))?
                .next()
                .is_none();
            if empty {
                Ok(true)
            } else {
                Err(AnonymizeError::OutNotEmpty)
            }
        }
        Ok(_) => Err(AnonymizeError::OutNotEmpty),
        Err(_) => Ok(false),
    }
}

/// Deletes what a failed run wrote: the whole directory when the run made
/// it, its contents otherwise.
fn clean(out: &Path, existed: bool) {
    if !existed {
        let _ = fs::remove_dir_all(out);
        return;
    }
    if let Ok(entries) = fs::read_dir(out) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = if entry.file_type().is_ok_and(|t| t.is_dir()) {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
        }
    }
}

fn sql(e: meguri_storage::StorageError) -> AnonymizeError {
    AnonymizeError::Sqlite(e.to_string())
}

fn text_of(value: &RawValue) -> String {
    match value {
        RawValue::Text(bytes) | RawValue::Blob(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        RawValue::Integer(v) => v.to_string(),
        RawValue::Real(v) => v.to_string(),
        RawValue::Null => String::new(),
    }
}

fn int_of(value: &RawValue) -> i64 {
    match value {
        RawValue::Integer(v) => *v,
        _ => 0,
    }
}

/// A note type's sort field and whether it is image occlusion.
#[derive(Clone, Copy)]
struct NotetypeInfo {
    sort_field: usize,
    image_occlusion: bool,
}

struct Deck {
    id: i64,
    /// The stored name (components separated by U+001F).
    name: RawValue,
    row: Vec<RawValue>,
}

struct Run<'a> {
    source: PathBuf,
    out: PathBuf,
    salt: &'a Salt,
    hooks: &'a Hooks,
    db: PlainDatabase,
    map: NameMap<'a>,
    tags: TagMap<'a>,
    chars: CharCounts,
    searches: SearchCounts,
    normalize_note_text: bool,
}

impl<'a> Run<'a> {
    /// A14.3 steps 1 and 2: the work copy, opened as a plain database.
    fn new(source: &Path, out: &Path, salt: &'a Salt, hooks: &'a Hooks) -> Result<Run<'a>, AnonymizeError> {
        let work = out.join(WORK);
        fs::create_dir_all(&work).map_err(|e| AnonymizeError::io("creating the work directory", &e))?;
        fs::copy(source.join(CLONE_COLLECTION), work.join(CLONE_COLLECTION))
            .map_err(|e| AnonymizeError::io("copying the collection", &e))?;
        match fs::copy(source.join(SOURCE_WAL), work.join(SOURCE_WAL)) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(AnonymizeError::io("copying the collection's log", &e)),
        }
        let db =
            PlainDatabase::open(&work.join(CLONE_COLLECTION)).map_err(|e| AnonymizeError::OpenFailed(e.to_string()))?;
        if let Err(e) = db.rows("SELECT count(*) FROM sqlite_master", &[]) {
            return Err(match e {
                meguri_storage::StorageError::Sqlite { code, .. } if code == SQLITE_NOTADB => {
                    AnonymizeError::OpenFailed("not a database".into())
                }
                other => AnonymizeError::OpenFailed(other.to_string()),
            });
        }
        match db.rows("SELECT ver FROM col", &[]).ok().as_deref() {
            Some([row]) if row.first() == Some(&RawValue::Integer(18)) => {}
            _ => return Err(AnonymizeError::UnsupportedSchema),
        }
        let normalize_note_text = db
            .rows("SELECT val FROM config WHERE KEY = 'normalize_note_text'", &[])
            .ok()
            .and_then(|rows| match rows.first().and_then(|r| r.first()) {
                Some(RawValue::Blob(bytes)) => ConfigValue::parse(bytes).and_then(|v| v.as_bool()),
                _ => None,
            })
            .unwrap_or(true);
        let map = NameMap::new(salt, &source.join(CLONE_MEDIA))?;
        Ok(Run {
            source: source.to_owned(),
            out: out.to_owned(),
            salt,
            hooks,
            db,
            map,
            tags: TagMap::new(salt),
            chars: CharCounts::default(),
            searches: SearchCounts::default(),
            normalize_note_text,
        })
    }

    fn all(&mut self) -> Result<Report, AnonymizeError> {
        let before = checks::row_digests(&self.db)?;
        let notetypes = self.notetypes()?;
        let structure_names = self.structure_names()?;
        let decks = self.decks()?;
        let tag_rows = self
            .db
            .rows("SELECT tag, usn, collapsed, config FROM tags", &[])
            .map_err(sql)?;
        let note_tags = self.distinct_note_tags()?;

        // Ranks (A11.1) and the search targets (A13.3, A13.4).
        let stored: Vec<(i64, String)> = decks.iter().map(|d| (d.id, text_of(&d.name))).collect();
        let ranks = names::deck_ranks(&stored);
        let human = |stored: &str| stored.split(STORED_SEPARATOR).collect::<Vec<_>>().join(SEPARATOR);
        let mut tag_targets: Vec<(String, String)> = Vec::new();
        let mut seen = HashSet::new();
        for tag in tag_rows.iter().map(|r| text_of(&r[0])).chain(note_tags.iter().cloned()) {
            if seen.insert(fold::folded(&tag)) {
                let new = self.tags.map(&tag);
                tag_targets.push((tag, new));
            }
        }
        let targets = Targets {
            decks: stored
                .iter()
                .map(|(id, name)| (human(name), human(&ranks[id])))
                .collect(),
            tags: tag_targets,
        };

        // Config values the rewrite and V4 read before they change.
        let config = self.config_rows()?;
        let mut token_sources: Vec<String> = Vec::new();
        for (_, name) in &stored {
            token_sources.extend(name.split(STORED_SEPARATOR).map(str::to_owned));
        }
        for tag in tag_rows.iter().map(|r| text_of(&r[0])).chain(note_tags.iter().cloned()) {
            for (i, component) in tag.split(SEPARATOR).enumerate() {
                if !(i == 0 && TagMap::kept_first(component)) {
                    token_sources.push(component.to_owned());
                }
            }
        }
        for (key, value) in &config {
            match key.as_str() {
                "savedFilters" => token_sources.extend(value.members().into_iter().map(|(n, _)| n.to_owned())),
                "flagLabels" => token_sources.extend(
                    value
                        .members()
                        .into_iter()
                        .filter_map(|(_, v)| v.as_str().map(str::to_owned)),
                ),
                _ => {}
            }
        }

        self.db.execute_batch("BEGIN").map_err(sql)?;
        let clone_index = self.map.clone_index();
        let (fields_total, fields_changed, io_keys, notes) = self.notes(&notetypes, &clone_index)?;
        let deck_counts = self.write_decks(&decks, &ranks, &targets)?;
        let tag_counts = self.write_tags(&tag_rows)?;
        self.write_config(&config, &targets)?;
        let stat4_rows_deleted = self.drop_stat4()?;
        self.db.execute_batch("COMMIT").map_err(sql)?;

        // A14.3 step 4: a new file holding only live rows.
        let clone_path = self.out.join(CLONE_COLLECTION);
        self.db
            .execute("VACUUM INTO ?1", &[RawValue::text(&clone_path.to_string_lossy())])
            .map_err(sql)?;

        // Media (A10).
        let written = media::write_files(
            &self.map,
            self.salt,
            &self.source.join(CLONE_MEDIA),
            &self.out.join(CLONE_MEDIA),
            &io_keys,
        )?;

        // V2, V4, V5 on the clone.
        let clone = PlainDatabase::open(&clone_path).map_err(sql)?;
        let after = checks::row_digests(&clone)?;
        let rows = if before == after { Status::Pass } else { Status::Fail };
        for name in self.map.domain.iter().filter(|n| !n.starts_with('_')) {
            token_sources.push(name.clone());
        }
        let tokens = checks::tokens(token_sources.iter().map(String::as_str), &structure_names);
        let leak = checks::leak_scan(&clone, &written, &tokens)?;
        clone.close().map_err(sql)?;
        let media_ok = checks::media(&self.map, &written, &self.out.join(CLONE_MEDIA));

        let mut counts = self.map.counts;
        counts.kept = self.map.files.values().filter(|f| f.name.starts_with('_')).count() as u64;
        for w in &written {
            if matches!(w.placeholder, media::Placeholder::Png(_) | media::Placeholder::Svg) {
                counts.io_images += 1;
                counts.io_images_larger += u64::from(w.larger);
            }
            if w.placeholder == media::Placeholder::Png(None) {
                counts.io_images_unknown_size += 1;
            }
        }
        let leak_status = if leak.failed {
            Status::Fail
        } else if leak.locations.is_empty() {
            Status::Pass
        } else {
            Status::Warn
        };
        let locations: Vec<serde_json::Value> = leak
            .locations
            .iter()
            .map(|((table, column, key), hits)| {
                let key = match key {
                    None => serde_json::Value::Null,
                    Some(k) if table != "config" => k.parse::<i64>().map_or_else(|_| json!(k), |n| json!(n)),
                    Some(k) => json!(k),
                };
                json!({"table": table, "column": column, "key": key, "hits": hits})
            })
            .collect();
        let media_status = if media_ok { Status::Pass } else { Status::Fail };
        let json = json!({
            "checks": {
                "leak": {"locations": locations, "status": leak_status.as_str()},
                "media": media_status.as_str(),
                "rows": rows.as_str(),
                "structure": "pass",
            },
            "chars_replaced": self.chars.chars,
            "decks": {
                "descriptions": deck_counts.descriptions,
                "renamed": deck_counts.renamed,
                "searches": deck_counts.searches,
            },
            "entities_replaced": self.chars.entities,
            "fields": {"changed": fields_changed, "total": fields_total},
            "format": 1,
            "media": {
                "bare_names": counts.bare_names,
                "files": counts.files,
                "io_images": counts.io_images,
                "io_images_larger": counts.io_images_larger,
                "io_images_unknown_size": counts.io_images_unknown_size,
                "kept": counts.kept,
                "refs": counts.refs,
                "refs_malformed": counts.refs_malformed,
                "refs_missing": counts.refs_missing,
                "refs_remote": counts.refs_remote,
                "skipped_entries": counts.skipped_entries,
            },
            "notes": notes,
            "salt_sha256": self.salt.sha256_hex(),
            "searches": {
                "inexact": self.searches.inexact,
                "rewritten": self.searches.rewritten,
                "unparsed": self.searches.unparsed,
            },
            "stat4_rows_deleted": stat4_rows_deleted,
            "tags": {"kept": tag_counts.0, "renamed": tag_counts.1},
        });
        let report = Report {
            json,
            leak: leak_status,
            rows,
            media: media_status,
        };
        fs::write(self.out.join(REPORT), report.text()).map_err(|e| AnonymizeError::io("writing the report", &e))?;
        Ok(report)
    }

    fn notetypes(&self) -> Result<HashMap<i64, NotetypeInfo>, AnonymizeError> {
        let rows = self.db.rows("SELECT id, config FROM notetypes", &[]).map_err(sql)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let config = match &row[1] {
                    RawValue::Blob(bytes) | RawValue::Text(bytes) => ProtoBlob::<NotetypeConfig>::decode(bytes).ok(),
                    _ => None,
                };
                let info = NotetypeInfo {
                    sort_field: config.as_ref().map_or(0, |c| c.sort_field_idx as usize),
                    image_occlusion: config.is_some_and(|c| c.original_stock_kind == IMAGE_OCCLUSION_KIND),
                };
                (int_of(&row[0]), info)
            })
            .collect())
    }

    /// V4: the folds of the runs in note type, field, template and preset
    /// names.
    fn structure_names(&self) -> Result<HashSet<String>, AnonymizeError> {
        let mut out = HashSet::new();
        for table in ["notetypes", "fields", "templates", "deck_config"] {
            let rows = self.db.rows(&format!("SELECT name FROM {table}"), &[]).map_err(sql)?;
            for row in rows {
                for run in checks::runs(&text_of(&row[0])) {
                    out.insert(fold::folded(run));
                }
            }
        }
        Ok(out)
    }

    fn decks(&self) -> Result<Vec<Deck>, AnonymizeError> {
        let rows = self
            .db
            .rows(
                "SELECT id, name, mtime_secs, usn, common, kind FROM decks ORDER BY id",
                &[],
            )
            .map_err(sql)?;
        Ok(rows
            .into_iter()
            .map(|row| Deck {
                id: int_of(&row[0]),
                name: row[1].clone(),
                row,
            })
            .collect())
    }

    /// The distinct tags of notes (A13.4's candidates), first seen first.
    fn distinct_note_tags(&self) -> Result<Vec<String>, AnonymizeError> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        self.db
            .for_each("SELECT DISTINCT tags FROM notes", &[], |row| {
                for tag in names::split_note_tags(&text_of(&row[0])) {
                    if seen.insert(fold::folded(tag)) {
                        out.push(tag.to_owned());
                    }
                }
            })
            .map_err(sql)?;
        Ok(out)
    }

    /// The config values A13.5 rewrites, by key, when they parse.
    fn config_rows(&self) -> Result<BTreeMap<String, ConfigValue>, AnonymizeError> {
        let mut out = BTreeMap::new();
        for key in checks::REWRITTEN_CONFIG {
            let rows = self
                .db
                .rows("SELECT val FROM config WHERE KEY = ?1", &[RawValue::text(key)])
                .map_err(sql)?;
            if let Some(RawValue::Blob(bytes)) = rows.first().and_then(|r| r.first())
                && let Some(value) = ConfigValue::parse(bytes)
            {
                out.insert((*key).to_owned(), value);
            }
        }
        Ok(out)
    }

    /// A4 `notes`: fields (A5–A9, V1), tags (A12.2), `sfld` and `csum`
    /// (A14.1). Returns the fields seen and changed, the image-occlusion
    /// images' keys and the number of notes.
    fn notes(
        &mut self,
        notetypes: &HashMap<i64, NotetypeInfo>,
        clone_index: &HashMap<String, String>,
    ) -> Result<(u64, u64, BTreeSet<String>, u64), AnonymizeError> {
        let (mut total, mut changed, mut notes) = (0u64, 0u64, 0u64);
        let mut io_keys = BTreeSet::new();
        let mut after = i64::MIN;
        loop {
            let batch = self
                .db
                .rows(
                    "SELECT id, mid, flds, tags FROM notes WHERE id > ?1 ORDER BY id LIMIT ?2",
                    &[RawValue::Integer(after), RawValue::Integer(NOTE_BATCH)],
                )
                .map_err(sql)?;
            let Some(last) = batch.last() else { break };
            after = int_of(&last[0]);
            for row in &batch {
                let id = int_of(&row[0]);
                let info = notetypes.get(&int_of(&row[1])).copied().unwrap_or(NotetypeInfo {
                    sort_field: 0,
                    image_occlusion: false,
                });
                let flds = match &row[2] {
                    RawValue::Text(bytes) | RawValue::Blob(bytes) => {
                        String::from_utf8(bytes.clone()).map_err(|_| AnonymizeError::InvalidUtf8 { note: id })?
                    }
                    other => text_of(other),
                };
                let mut stream = self.salt.stream(&format!("note:{id}"));
                let mut clone_fields = Vec::new();
                for field in flds.split(names::STORED_SEPARATOR) {
                    let new = rewrite::field(field, &mut self.map, &mut stream, &mut self.chars);
                    if let Some(e) = self.map.error.take() {
                        return Err(e);
                    }
                    if !checks::structure(field, &new, &self.map, clone_index) {
                        return Err(AnonymizeError::StructureChanged { note: id });
                    }
                    total += 1;
                    changed += u64::from(new != field);
                    clone_fields.push(new);
                }
                let referenced = self.map.take_referenced();
                if info.image_occlusion {
                    io_keys.extend(
                        referenced
                            .into_iter()
                            .filter(|key| media::is_image_key(key) && self.map.files.contains_key(key)),
                    );
                }
                let tags = self.tags.note_tags(&text_of(&row[3]));
                let saved: Vec<String> = clone_fields
                    .iter()
                    .map(|f| normalize::field_as_saved(f, self.normalize_note_text).into_owned())
                    .collect();
                let sfld = checksum::sort_text_of(&saved, info.sort_field).into_owned();
                let csum = checksum::note_checksum(saved.first().map_or("", String::as_str));
                self.db
                    .execute(
                        "UPDATE notes SET flds = ?1, tags = ?2, sfld = ?3, csum = ?4 WHERE id = ?5",
                        &[
                            RawValue::text(&clone_fields.join(&names::STORED_SEPARATOR.to_string())),
                            RawValue::text(&tags),
                            RawValue::text(&sfld),
                            RawValue::Integer(i64::from(csum)),
                            RawValue::Integer(id),
                        ],
                    )
                    .map_err(sql)?;
                notes += 1;
            }
        }
        Ok((total, changed, io_keys, notes))
    }

    /// A11: names, descriptions and searches; rows deleted and inserted
    /// again with the same ids, so the name index never holds two equal
    /// names between two updates.
    fn write_decks(
        &mut self,
        decks: &[Deck],
        ranks: &HashMap<i64, String>,
        targets: &Targets,
    ) -> Result<DeckCounts, AnonymizeError> {
        let mut counts = DeckCounts::default();
        let mut rows = Vec::with_capacity(decks.len());
        for deck in decks {
            let mut row = deck.row.clone();
            if self.hooks.keep_deck_name != Some(deck.id) {
                row[1] = RawValue::text(&ranks[&deck.id]);
            }
            counts.renamed += u64::from(row[1] != deck.name);
            let mut stream = self.salt.stream(&format!("deck:{}", deck.id));
            if let RawValue::Blob(kind) = &deck.row[5] {
                // The description, then the searches, from the deck's stream
                // (a deck has one or the other).
                let (map, chars) = (&mut self.map, &mut self.chars);
                let description = kind::rewrite_description(kind, &mut |description| {
                    counts.descriptions += u64::from(!description.is_empty());
                    rewrite::field(description, &mut *map, &mut stream, &mut *chars)
                });
                let base = description.as_deref().unwrap_or(kind);
                let mut rewriter = SearchRewriter {
                    targets,
                    names: &mut self.map,
                    stream: &mut stream,
                    chars: &mut self.chars,
                    counts: &mut self.searches,
                };
                let searches = kind::rewrite_searches(base, &mut |s| {
                    counts.searches += 1;
                    rewriter.rewrite(s)
                });
                if let Some(blob) = searches.or(description) {
                    row[5] = RawValue::Blob(blob);
                }
                if let Some(e) = self.map.error.take() {
                    return Err(e);
                }
                self.map.take_referenced();
            }
            rows.push(row);
        }
        self.db.execute("DELETE FROM decks", &[]).map_err(sql)?;
        for row in rows {
            self.db
                .execute(
                    "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    &row,
                )
                .map_err(sql)?;
        }
        Ok(counts)
    }

    /// A12.3: the `tags` table; returns the rows kept and renamed.
    fn write_tags(&mut self, rows: &[Vec<RawValue>]) -> Result<(u64, u64), AnonymizeError> {
        let mut seen = HashSet::new();
        let (mut kept, mut renamed) = (0u64, 0u64);
        let mut new_rows = Vec::with_capacity(rows.len());
        for row in rows {
            let old = text_of(&row[0]);
            let new = self.tags.map(&old);
            if !seen.insert(fold::folded(&new)) {
                return Err(AnonymizeError::HashCollision);
            }
            if new == old {
                kept += 1;
            } else {
                renamed += 1;
            }
            let mut row = row.clone();
            row[0] = RawValue::text(&new);
            new_rows.push(row);
        }
        self.db.execute("DELETE FROM tags", &[]).map_err(sql)?;
        for row in new_rows {
            self.db
                .execute(
                    "INSERT INTO tags (tag, usn, collapsed, config) VALUES (?1, ?2, ?3, ?4)",
                    &row,
                )
                .map_err(sql)?;
        }
        Ok((kept, renamed))
    }

    /// A13.5.
    fn write_config(
        &mut self,
        config: &BTreeMap<String, ConfigValue>,
        targets: &Targets,
    ) -> Result<(), AnonymizeError> {
        for (key, value) in config {
            let mut stream = self.salt.stream(&format!("config:{key}"));
            let mut rewriter = SearchRewriter {
                targets,
                names: &mut self.map,
                stream: &mut stream,
                chars: &mut self.chars,
                counts: &mut self.searches,
            };
            let new = match (key.as_str(), value) {
                ("defaultSearchText", ConfigValue::String(s)) => ConfigValue::String(rewriter.rewrite(s)),
                ("savedFilters", ConfigValue::Object(_)) => {
                    let mut members: Vec<(String, ConfigValue)> = Vec::new();
                    let mut names_used = HashSet::new();
                    for (name, search) in value.members() {
                        let mut new_name = rewrite::plain(name, rewriter.names, rewriter.stream, rewriter.chars);
                        let mut attempts = 0;
                        while !names_used.insert(new_name.clone()) {
                            attempts += 1;
                            if attempts > 1_000 {
                                return Err(AnonymizeError::HashCollision);
                            }
                            new_name = rewrite::plain(name, rewriter.names, rewriter.stream, rewriter.chars);
                        }
                        let new_search = match search {
                            ConfigValue::String(s) => ConfigValue::String(rewriter.rewrite(s)),
                            other => other.clone(),
                        };
                        members.push((new_name, new_search));
                    }
                    ConfigValue::Object(members)
                }
                ("flagLabels", ConfigValue::Object(_)) => ConfigValue::Object(
                    value
                        .members()
                        .into_iter()
                        .map(|(number, label)| {
                            let label = match label {
                                ConfigValue::String(s) => ConfigValue::String(rewrite::plain(
                                    s,
                                    rewriter.names,
                                    rewriter.stream,
                                    rewriter.chars,
                                )),
                                other => other.clone(),
                            };
                            (number.to_owned(), label)
                        })
                        .collect(),
                ),
                _ => continue,
            };
            self.map.take_referenced();
            self.db
                .execute(
                    "UPDATE config SET val = ?1 WHERE KEY = ?2",
                    &[RawValue::Blob(new.compact().into_bytes()), RawValue::text(key)],
                )
                .map_err(sql)?;
        }
        Ok(())
    }

    /// A14.2: the `sqlite_stat4` samples of the name indexes.
    fn drop_stat4(&mut self) -> Result<u64, AnonymizeError> {
        let present = self
            .db
            .rows(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'sqlite_stat4'",
                &[],
            )
            .map_err(sql)?;
        if present.is_empty() {
            return Ok(0);
        }
        let list = checks::STAT4_NAME_INDEXES
            .iter()
            .filter(|idx| !(self.hooks.keep_tag_samples && **idx == "tags"))
            .map(|idx| format!("'{idx}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let n = self
            .db
            .execute(&format!("DELETE FROM sqlite_stat4 WHERE idx IN ({list})"), &[])
            .map_err(sql)?;
        Ok(n as u64)
    }
}

#[derive(Default)]
struct DeckCounts {
    descriptions: u64,
    renamed: u64,
    searches: u64,
}
