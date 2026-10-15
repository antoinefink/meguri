//! Step 14 (`storage/schema-upgrade` U4): the tables `deck_config`, `config`
//! and `tags`; presets, config and tags move out of `col`.

use meguri_model::schema11::col::{conf_from_json, tags_from_json};
use meguri_model::schema11::deck_config_from_json;
use rusqlite::Connection;

use super::legacy_json::{self, compact, id_key};
use super::{StepError, StepResult, clear_col, col_text};
use crate::schema::ddl18;

/// The step-14 form of `tags`; step 17 rebuilds it with `collapsed` and
/// `config`.
const TAGS_14: &str =
    "CREATE TABLE tags ( tag text NOT NULL PRIMARY KEY COLLATE unicase, usn integer NOT NULL ) without rowid";

pub(super) fn run(conn: &Connection) -> StepResult {
    conn.execute_batch(&format!("{};\n{};\n{TAGS_14};", ddl18("deck_config"), ddl18("config")))?;
    presets(conn)?;
    config(conn)?;
    tags(conn)
}

/// U4.2: each preset of `col.dconf` becomes a row under its member name, its
/// schema-11 JSON kept as is for step 15 to convert.
fn presets(conn: &Connection) -> StepResult {
    let text = col_text(conn, "dconf")?;
    let members = legacy_json::members(&text).ok_or_else(|| StepError("col.dconf is not a JSON object".into()))?;
    let mut insert =
        conn.prepare("INSERT INTO deck_config (id, name, mtime_secs, usn, config) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    for (key, raw) in members {
        let id = id_key(&key).ok_or_else(|| StepError(format!("col.dconf key {key:?} is not an id")))?;
        let preset = deck_config_from_json(raw.get())?;
        insert.execute((id, &preset.name, preset.mtime.0, preset.usn.0, raw.get().as_bytes()))?;
    }
    clear_col(conn, "dconf")
}

/// U4.3: each member of `col.conf` becomes a `config` row, its value
/// written as compact JSON (U9.2).
fn config(conn: &Connection) -> StepResult {
    let text = col_text(conn, "conf")?;
    let entries = conf_from_json(&text)?;
    let mut insert = conn.prepare("INSERT INTO config (KEY, usn, mtime_secs, val) VALUES (?1, 0, 0, ?2)")?;
    for (key, value) in entries {
        let json = std::str::from_utf8(&value)
            .ok()
            .and_then(compact)
            .ok_or_else(|| StepError(format!("col.conf value of {key:?} cannot be rewritten")))?;
        insert.execute((&key, json.as_bytes()))?;
    }
    clear_col(conn, "conf")
}

/// U4.4: each tag of `col.tags` with its usn, in text order; a tag equal
/// under `unicase` to one already stored is dropped (the key's collation
/// refuses it, and the earlier one keeps its spelling and usn).
fn tags(conn: &Connection) -> StepResult {
    let text = col_text(conn, "tags")?;
    let tags = tags_from_json(&text)?;
    let mut insert = conn.prepare("INSERT OR IGNORE INTO tags (tag, usn) VALUES (?1, ?2)")?;
    for (tag, usn) in tags {
        insert.execute((&tag, usn.0))?;
    }
    clear_col(conn, "tags")
}
