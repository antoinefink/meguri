//! Validation (`specs/fixtures/anonymizer.md` A17): V1 on every field, V2
//! on the tables, V4's leak scan and V5 on the media folder. V3 (the
//! oracle) is `tools/mg anonymize`'s.

use std::collections::{BTreeMap, HashMap, HashSet};

use meguri_storage::raw::{PlainDatabase, RawValue};
use meguri_text::{entities, field_empty, fold, media_refs};
use sha2::{Digest, Sha256};

use super::chars::{Class, is_replaced};
use super::media::{CloneNames, NameMap, Placeholder, Written, is_well_formed, key_of};
use super::text::{self, Names, Reference, Sink};
use super::{AnonymizeError, keys};

// ------------------------------------------------------------------- V1

/// One event of a walk, as V1 compares them.
#[derive(Clone, Debug, PartialEq)]
enum Unit {
    Keep(char),
    Name(String),
    Replace(char),
    Entity(char),
}

#[derive(Default)]
struct Units(Vec<Unit>);

impl Sink for Units {
    fn keep(&mut self, text: &str) {
        self.0.extend(text.chars().map(Unit::Keep));
    }
    fn name(&mut self, text: &str) {
        self.0.push(Unit::Name(text.to_owned()));
    }
    fn replace(&mut self, c: char) {
        self.0.push(Unit::Replace(c));
    }
    fn entity(&mut self, _raw: &str, c: char) {
        self.0.push(Unit::Entity(c));
    }
    fn g3_begin(&mut self) {}
    fn g3_end(&mut self) {}
}

/// The source side of V1: names resolve as the rewrite resolved them,
/// without counting anything again.
struct SourceNames<'m, 's> {
    map: &'m NameMap<'s>,
}

impl SourceNames<'_, '_> {
    fn clone_of(&self, key: &str) -> Option<String> {
        self.map.clone_name(key)
    }
}

impl Names for SourceNames<'_, '_> {
    fn reference(&mut self, raw: &str) -> Reference {
        if media_refs::is_remote(raw) {
            return Reference::Remote;
        }
        let decoded = entities::decode_text(raw);
        if !is_well_formed(&decoded) {
            return Reference::Malformed;
        }
        if decoded.starts_with('_') {
            return Reference::Local(raw.to_owned());
        }
        Reference::Local(self.clone_of(&key_of(&decoded)).unwrap_or_else(|| raw.to_owned()))
    }

    fn file(&mut self, candidate: &str, decode: bool, need_extension: bool) -> Option<String> {
        if need_extension && !candidate.contains('.') {
            return None;
        }
        let decoded = if decode {
            entities::decode_text(candidate)
        } else {
            candidate.into()
        };
        let key = key_of(&decoded);
        if need_extension && super::media::extension(&key).is_none() {
            return None;
        }
        self.map.files.get(&key).map(|f| f.clone.clone())
    }
}

/// V1 for one field: `Ok` when the clone has the source's structure.
pub fn structure(source: &str, clone: &str, map: &NameMap<'_>, clone_index: &HashMap<String, String>) -> bool {
    let mut src_units = Units::default();
    text::walk_field(source, &mut SourceNames { map }, &mut src_units);
    let mut clone_units = Units::default();
    text::walk_field(clone, &mut CloneNames { by_key: clone_index }, &mut clone_units);
    if src_units.0.len() != clone_units.0.len() {
        return false;
    }
    let units_match = src_units.0.iter().zip(&clone_units.0).all(|pair| match pair {
        (Unit::Keep(a), Unit::Keep(b)) => a == b,
        (Unit::Name(a), Unit::Name(b)) => a == b,
        (Unit::Replace(a) | Unit::Entity(a), Unit::Replace(b)) => Class::of(*a).pool_contains(*b),
        _ => false,
    });
    if !units_match {
        return false;
    }
    // The opener shapes, E1 emptiness and the references, mapped.
    if openers(source) != openers(clone) || field_empty::counts_as_empty(source) != field_empty::counts_as_empty(clone)
    {
        return false;
    }
    let expected: Vec<String> = media_refs::local_references(source)
        .into_iter()
        .filter(|name| is_well_formed(name))
        .map(|name| {
            if name.starts_with('_') {
                name
            } else {
                map.clone_name(&key_of(&name)).unwrap_or(name)
            }
        })
        .collect();
    let found: Vec<String> = media_refs::local_references(clone)
        .into_iter()
        .filter(|name| is_well_formed(name))
        .collect();
    expected == found
}

fn openers(field: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(at) = field[i..].find("{{c") {
        let p = i + at;
        match text::opener_len(&field[p..]) {
            Some(len) => {
                out.push(&field[p..p + len]);
                i = p + len;
            }
            None => i = p + 1,
        }
    }
    out
}

// ------------------------------------------------------------------- V2

/// Row count and SHA-256 of a query's rows, every value with its storage
/// class and exact bytes.
pub fn digest(db: &PlainDatabase, sql: &str) -> Result<(u64, String), AnonymizeError> {
    let mut hasher = Sha256::new();
    let mut rows = 0u64;
    db.for_each(sql, &[], |row| {
        rows += 1;
        for value in &row {
            match value {
                RawValue::Null => hasher.update([0u8]),
                RawValue::Integer(v) => {
                    hasher.update([1u8]);
                    hasher.update(v.to_le_bytes());
                }
                RawValue::Real(v) => {
                    hasher.update([2u8]);
                    hasher.update(v.to_bits().to_le_bytes());
                }
                RawValue::Text(bytes) | RawValue::Blob(bytes) => {
                    hasher.update([if matches!(value, RawValue::Text(_)) { 3u8 } else { 4u8 }]);
                    hasher.update((bytes.len() as u64).to_le_bytes());
                    hasher.update(bytes);
                }
            }
        }
        hasher.update([0xFFu8]);
    })
    .map_err(|e| AnonymizeError::Sqlite(format!("digesting rows: {e}")))?;
    Ok((rows, keys::hex(&hasher.finalize())))
}

/// The tables of a database, in name order.
pub fn tables(db: &PlainDatabase) -> Result<Vec<String>, AnonymizeError> {
    let rows = db
        .rows("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name", &[])
        .map_err(|e| AnonymizeError::Sqlite(format!("listing tables: {e}")))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match row.into_iter().next() {
            Some(RawValue::Text(name)) => String::from_utf8(name).ok(),
            _ => None,
        })
        .collect())
}

fn quoted(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The config keys A13.5 rewrites.
pub const REWRITTEN_CONFIG: &[&str] = &["defaultSearchText", "savedFilters", "flagLabels"];

/// The `sqlite_stat4` `idx` values whose samples hold names (A14.2). The
/// `tags` table is WITHOUT ROWID: SQLite files its key samples under the
/// table's own name, never under `sqlite_autoindex_tags_1`.
pub const STAT4_NAME_INDEXES: &[&str] = &["idx_decks_name", "ix_notes_csum", "tags"];

fn sql_list(items: &[&str]) -> String {
    items.iter().map(|i| sql_text(i)).collect::<Vec<_>>().join(", ")
}

/// V2's digests of a database: every table A4 keeps whole, and the kept
/// columns of `notes`, `decks` and `config`; `tags` by count;
/// `sqlite_stat4` without A14.2's rows.
pub fn row_digests(db: &PlainDatabase) -> Result<BTreeMap<String, (u64, String)>, AnonymizeError> {
    let mut out = BTreeMap::new();
    for table in tables(db)? {
        let sql = match table.as_str() {
            "notes" => "SELECT id, guid, mid, mod, usn, flags, data FROM notes ORDER BY id".to_owned(),
            "decks" => "SELECT id, mtime_secs, usn, common FROM decks ORDER BY id".to_owned(),
            "config" => {
                out.insert(
                    "config:keys".to_owned(),
                    digest(db, "SELECT KEY, usn, mtime_secs FROM config ORDER BY KEY")?,
                );
                format!(
                    "SELECT KEY, usn, mtime_secs, val FROM config WHERE KEY NOT IN ({}) ORDER BY KEY",
                    sql_list(REWRITTEN_CONFIG)
                )
            }
            "tags" => "SELECT count(*) FROM tags".to_owned(),
            "sqlite_stat4" => format!(
                "SELECT tbl, idx, neq, nlt, ndlt, sample FROM sqlite_stat4 WHERE idx NOT IN ({}) ORDER BY 1, 2, 3, 4, 5, 6",
                sql_list(STAT4_NAME_INDEXES)
            ),
            other => {
                let n = column_names(db, other)?.len().max(1);
                let order: Vec<String> = (1..=n).map(|i| i.to_string()).collect();
                format!("SELECT * FROM {} ORDER BY {}", quoted(other), order.join(", "))
            }
        };
        out.insert(table.clone(), digest(db, &sql)?);
    }
    Ok(out)
}

fn sql_text(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// A table's column names, in order.
fn column_names(db: &PlainDatabase, table: &str) -> Result<Vec<String>, AnonymizeError> {
    Ok(db
        .rows(&format!("SELECT name FROM pragma_table_info({})", sql_text(table)), &[])
        .map_err(|e| AnonymizeError::Sqlite(format!("reading a table: {e}")))?
        .into_iter()
        .filter_map(|row| match row.into_iter().next() {
            Some(RawValue::Text(name)) => String::from_utf8(name).ok(),
            _ => None,
        })
        .collect())
}

// ------------------------------------------------------------------- V4

/// A16 `checks.leak`.
#[derive(Clone, Debug, Default)]
pub struct Leak {
    /// `{table, column, key, hits}` with the hits per location.
    pub locations: BTreeMap<(String, String, Option<String>), u64>,
    pub failed: bool,
}

/// V4's runs: maximal runs of letters, marks and digits.
pub fn runs(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !is_replaced(c)).filter(|r| !r.is_empty())
}

/// V4's tokens from source strings: runs of at least 4 characters, without
/// runs of digits, of hex digits, or that occur in `structure_names`.
pub fn tokens<'a>(sources: impl Iterator<Item = &'a str>, structure_names: &HashSet<String>) -> HashSet<String> {
    let mut out = HashSet::new();
    for source in sources {
        for run in runs(source) {
            if run.chars().count() < 4 || run.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let folded = fold::folded(run);
            if !structure_names.contains(&folded) {
                out.insert(folded);
            }
        }
    }
    out
}

/// V4: scans the clone's values and media file names for tokens.
pub fn leak_scan(db: &PlainDatabase, media: &[Written], tokens: &HashSet<String>) -> Result<Leak, AnonymizeError> {
    let mut leak = Leak::default();
    let fail_at = |table: &str, column: &str| {
        matches!(
            (table, column),
            ("decks", "name") | ("tags", "tag") | ("notes", "tags") | ("sqlite_stat4", "sample")
        )
    };
    // deck_config is unchanged but scanned: a preset's param_search can
    // name decks or tags (A4).
    for table in tables(db)? {
        if matches!(table.as_str(), "notetypes" | "fields" | "templates") {
            continue;
        }
        let q = quoted(&table);
        let with_rowid = db.rows(&format!("SELECT rowid FROM {q} LIMIT 0"), &[]).is_ok();
        let columns = column_names(db, &table)?;
        let key_expr = if table == "config" {
            "KEY".to_owned()
        } else if with_rowid {
            "rowid".to_owned()
        } else {
            "NULL".to_owned()
        };
        let select: Vec<String> = columns.iter().map(|c| quoted(c)).collect();
        let sql = format!("SELECT {key_expr}, {} FROM {q}", select.join(", "));
        let mut error = None;
        db.for_each(&sql, &[], |row| {
            let key = match &row[0] {
                RawValue::Integer(v) => Some(v.to_string()),
                RawValue::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
                _ => None,
            };
            for (column, value) in columns.iter().zip(&row[1..]) {
                if table == "notes" && (column == "flds" || column == "sfld") {
                    continue;
                }
                let bytes = match value {
                    RawValue::Text(bytes) | RawValue::Blob(bytes) => bytes,
                    _ => continue,
                };
                let text = String::from_utf8_lossy(bytes);
                let hits = runs(&text).filter(|run| tokens.contains(&fold::folded(run))).count() as u64;
                if hits > 0 {
                    *leak
                        .locations
                        .entry((table.clone(), column.clone(), key.clone()))
                        .or_insert(0) += hits;
                    if fail_at(&table, column) {
                        leak.failed = true;
                    }
                }
            }
        })
        .unwrap_or_else(|e| error = Some(e));
        if let Some(e) = error {
            return Err(AnonymizeError::Sqlite(format!("scanning a table: {e}")));
        }
    }
    for file in media {
        let hits = runs(&file.clone)
            .filter(|run| tokens.contains(&fold::folded(run)))
            .count() as u64;
        if hits > 0 {
            *leak
                .locations
                .entry(("media".to_owned(), "name".to_owned(), None))
                .or_insert(0) += hits;
            if !file.clone.starts_with('_') {
                leak.failed = true;
            }
        }
    }
    Ok(leak)
}

// ------------------------------------------------------------------- V5

/// V5: the clone media folder holds the clone names of the source files,
/// with their sizes, and valid PNG placeholders.
pub fn media(map: &NameMap<'_>, written: &[Written], out_media: &std::path::Path) -> bool {
    let expected: HashMap<&str, u64> = map.files.values().map(|f| (f.clone.as_str(), f.size)).collect();
    let by_name: HashMap<&str, &Written> = written.iter().map(|w| (w.clone.as_str(), w)).collect();
    let Ok(entries) = std::fs::read_dir(out_media) else {
        return false;
    };
    let mut seen = 0usize;
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let Ok(name) = entry.file_name().into_string() else {
            return false;
        };
        let (Some(&size), Some(w)) = (expected.get(name.as_str()), by_name.get(name.as_str())) else {
            return false;
        };
        let Ok(meta) = entry.metadata() else { return false };
        if meta.len() != size && !(w.larger && meta.len() == w.size) {
            return false;
        }
        if let Placeholder::Png(dims) = w.placeholder {
            let Ok(bytes) = std::fs::read(entry.path()) else {
                return false;
            };
            let (width, height) = dims.unwrap_or((1, 1));
            let ihdr_ok = bytes.len() >= 24
                && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && &bytes[12..16] == b"IHDR"
                && bytes[16..20] == width.to_be_bytes()
                && bytes[20..24] == height.to_be_bytes();
            if !ihdr_ok {
                return false;
            }
        }
        seen += 1;
    }
    seen == expected.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database whose ANALYZE samples A14.2's three indexes and one other.
    fn analyzed(dir: &std::path::Path) -> PlainDatabase {
        // An empty file is an empty database (PlainDatabase never creates).
        let path = dir.join("stat4.db");
        std::fs::write(&path, b"").unwrap();
        let db = PlainDatabase::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE d (name TEXT); CREATE INDEX idx_decks_name ON d (name);
             CREATE TABLE n (csum INTEGER); CREATE INDEX ix_notes_csum ON n (csum);
             CREATE TABLE tags (tag TEXT PRIMARY KEY) WITHOUT ROWID;
             CREATE TABLE c (nid INTEGER); CREATE INDEX ix_cards_nid ON c (nid);",
        )
        .unwrap();
        for i in 0..40 {
            let name = RawValue::text(&format!("name {i}"));
            db.execute("INSERT INTO d VALUES (?1)", std::slice::from_ref(&name))
                .unwrap();
            db.execute("INSERT INTO tags VALUES (?1)", &[name]).unwrap();
            db.execute("INSERT INTO n VALUES (?1)", &[RawValue::Integer(i)])
                .unwrap();
            db.execute("INSERT INTO c VALUES (?1)", &[RawValue::Integer(i % 7)])
                .unwrap();
        }
        db.execute_batch("ANALYZE").unwrap();
        db
    }

    fn count(db: &PlainDatabase, idx: &str) -> i64 {
        match db
            .rows(
                "SELECT count(*) FROM sqlite_stat4 WHERE idx = ?1",
                &[RawValue::text(idx)],
            )
            .unwrap()[0][0]
        {
            RawValue::Integer(n) => n,
            ref other => panic!("{other:?}"),
        }
    }

    #[test]
    fn v2_allows_only_the_name_samples_to_go() {
        // V2 (A17): sqlite_stat4 holds the source's rows minus those A14.2
        // removes, each unchanged.
        let dir = tempfile::tempdir().unwrap();
        let db = analyzed(dir.path());
        for idx in ["idx_decks_name", "ix_notes_csum", "tags", "ix_cards_nid"] {
            assert!(count(&db, idx) > 0, "ANALYZE sampled {idx}");
        }
        let source = row_digests(&db).unwrap();
        db.execute(
            "DELETE FROM sqlite_stat4 WHERE idx IN ('idx_decks_name', 'ix_notes_csum', 'tags')",
            &[],
        )
        .unwrap();
        assert_eq!(row_digests(&db).unwrap(), source, "the name samples may go");
        db.execute(
            "DELETE FROM sqlite_stat4 WHERE rowid = (SELECT min(rowid) FROM sqlite_stat4 WHERE idx = 'ix_cards_nid')",
            &[],
        )
        .unwrap();
        assert_ne!(row_digests(&db).unwrap(), source, "every other sample stays");
    }
}
