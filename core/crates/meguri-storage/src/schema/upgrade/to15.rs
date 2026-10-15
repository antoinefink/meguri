//! Step 15 (`storage/schema-upgrade` U5 to U8): the tables `fields`,
//! `templates`, `notetypes` and `decks` with their indexes, `ANALYZE`; note
//! types and decks move out of `col`; presets become protobuf with the
//! starting ease ×100.

use std::cmp::Ordering;
use std::collections::HashSet;

use meguri_model::deck::Deck;
use meguri_model::ids::DeckId;
use meguri_model::rng::RandomSource;
use meguri_model::schema11::{deck_config_from_json, deck_from_json, notetype_from_json};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_text::fold::compare_folded;
use meguri_text::normalize::nfc;
use rusqlite::Connection;
use rusqlite::types::ValueRef;
use serde_json::value::RawValue;

use super::legacy_json::{self, id_key};
use super::{StepError, StepResult, UpgradeContext, clear_col, col_text};
use crate::schema::ddl18;

/// U5.1's statements, in its order.
const CREATED: [&str; 12] = [
    "fields",
    "idx_fields_name_ntid",
    "templates",
    "idx_templates_name_ntid",
    "idx_templates_usn",
    "notetypes",
    "idx_notetypes_name",
    "idx_notetypes_usn",
    "decks",
    "idx_decks_name",
    "idx_notes_mid",
    "idx_cards_odid",
];

pub(super) fn run(conn: &Connection, context: &UpgradeContext<'_>) -> StepResult {
    for name in CREATED {
        conn.execute_batch(ddl18(name))?;
    }
    conn.execute_batch("ANALYZE")?;
    notetypes(conn, context.random)?;
    decks(conn, EpochSecs::from_clock(context.clock))?;
    presets(conn)
}

fn same(a: &str, b: &str) -> bool {
    compare_folded(a, b) == Ordering::Equal
}

/// The members of a `col` JSON column whose keys are ids (S12), in
/// ascending id order (U11); equal ids keep text order.
fn id_members<'a>(column: &str, text: &'a str) -> Result<Vec<(i64, &'a RawValue)>, StepError> {
    let members = legacy_json::members(text).ok_or_else(|| StepError(format!("col.{column} is not a JSON object")))?;
    let mut entries = members
        .into_iter()
        .map(|(key, raw)| match id_key(&key) {
            Some(id) => Ok((id, raw)),
            None => Err(StepError(format!("col.{column} key {key:?} is not an id"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|(id, _)| *id);
    Ok(entries)
}

/// U6.4: going through the names in order, while a name equals under
/// `unicase` an earlier one as already renamed, `suffix` is appended to it.
fn make_unique(names: &mut [String], suffix: char) {
    for i in 0..names.len() {
        let (earlier, rest) = names.split_at_mut(i);
        let name = &mut rest[0];
        while earlier.iter().any(|e| same(e, name)) {
            name.push(suffix);
        }
    }
}

/// U6.2: a uniformly random id from 1 to 2^32 − 1 that no other note type
/// of the collection uses.
fn new_notetype_id(used: &mut HashSet<i64>, random: &dyn RandomSource) -> i64 {
    loop {
        let id = (random.next_u64() >> 32) as i64;
        if id != 0 && used.insert(id) {
            return id;
        }
    }
}

/// U6: each note type of `col.models` becomes a `notetypes` row with its
/// `fields` and `templates` rows, under its member name.
fn notetypes(conn: &Connection, random: &dyn RandomSource) -> StepResult {
    let text = col_text(conn, "models")?;
    let entries = id_members("models", &text)?;
    let mut used: HashSet<i64> = entries.iter().map(|(id, _)| *id).filter(|id| *id != 0).collect();
    let mut written: Vec<String> = Vec::with_capacity(entries.len());
    let mut insert_notetype =
        conn.prepare("INSERT INTO notetypes (id, name, mtime_secs, usn, config) VALUES (?1, ?2, ?3, ?4, ?5)")?;
    let mut insert_field = conn.prepare("INSERT INTO fields (ntid, ord, name, config) VALUES (?1, ?2, ?3, ?4)")?;
    let mut insert_template =
        conn.prepare("INSERT INTO templates (ntid, ord, name, mtime_secs, usn, config) VALUES (?1, ?2, ?3, 0, 0, ?4)")?;
    for (key, raw) in entries {
        let notetype = notetype_from_json(raw.get())?;
        let id = if key == 0 {
            new_notetype_id(&mut used, random)
        } else {
            key
        };

        let mut templates: Vec<String> = notetype.templates.iter().map(|t| nfc(&t.name).into_owned()).collect();
        make_unique(&mut templates, '+');
        let mut fields: Vec<String> = notetype.fields.iter().map(|f| nfc(&f.name).into_owned()).collect();
        make_unique(&mut fields, '+');
        let mut name = nfc(&notetype.name).into_owned();
        while written.iter().any(|w| same(w, &name)) {
            name.push('_');
        }

        insert_notetype.execute((
            id,
            &name,
            notetype.mtime.0,
            notetype.usn.0,
            notetype.config.to_bytes().as_ref(),
        ))?;
        for (ord, (field, name)) in notetype.fields.iter().zip(&fields).enumerate() {
            insert_field.execute((id, ord as i64, name, field.config.to_bytes().as_ref()))?;
        }
        for (ord, (template, name)) in notetype.templates.iter().zip(&templates).enumerate() {
            insert_template.execute((id, ord as i64, name, template.config.to_bytes().as_ref()))?;
        }
        written.push(name);
    }
    clear_col(conn, "models")
}

/// U7: each deck of `col.decks` becomes a `decks` row under its own `id`,
/// with its name normalized (the model's conversion) and unique under
/// `unicase`; a deck whose name changes is stamped modified.
fn decks(conn: &Connection, now: EpochSecs) -> StepResult {
    let text = col_text(conn, "decks")?;
    let mut decks: Vec<(Deck, String)> = Vec::new();
    for (_, raw) in id_members("decks", &text)? {
        let deck = deck_from_json(raw.get())?;
        let stored = legacy_json::string_member(raw.get(), "name")
            .ok_or_else(|| StepError(format!("deck {} has no name", deck.id)))?;
        decks.push((deck, stored));
    }
    // U11: by the decks' own ids, not the member names.
    decks.sort_by_key(|(deck, _)| deck.id);
    let mut written: Vec<(DeckId, String)> = Vec::with_capacity(decks.len());
    let mut insert = conn.prepare(
        "INSERT OR REPLACE INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for (mut deck, stored) in decks {
        if deck.id.0 == 0 {
            return Err(StepError("a deck has id 0".to_owned()));
        }
        let mut renamed = deck.human_name() != stored;
        // A later deck with the same id replaces an earlier one, so its name
        // does not count.
        while written
            .iter()
            .any(|(id, name)| *id != deck.id && same(name, &deck.name))
        {
            deck.name.push('_');
            renamed = true;
        }
        if renamed {
            deck.mtime = now;
            deck.usn = Usn::PENDING;
        }
        insert.execute((
            deck.id.0,
            &deck.name,
            deck.mtime.0,
            deck.usn.0,
            deck.common.to_bytes().as_ref(),
            deck.kind.to_bytes().as_ref(),
        ))?;
        written.retain(|(id, _)| *id != deck.id);
        written.push((deck.id, deck.name));
    }
    clear_col(conn, "decks")
}

/// U8: each preset's schema-11 JSON converted, its starting ease multiplied
/// by 100 in 32-bit float arithmetic, written as the row whose id is the
/// JSON's `id`.
fn presets(conn: &Connection) -> StepResult {
    let rows = {
        let mut select = conn.prepare("SELECT id, config FROM deck_config ORDER BY id")?;
        let rows = select.query_map([], |row| {
            let bytes = match row.get_ref(1)? {
                ValueRef::Text(b) | ValueRef::Blob(b) => b.to_vec(),
                _ => Vec::new(),
            };
            Ok((row.get::<_, i64>(0)?, bytes))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut write = conn.prepare(
        "INSERT OR REPLACE INTO deck_config (id, name, mtime_secs, usn, config) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for (row_id, bytes) in rows {
        let json =
            std::str::from_utf8(&bytes).map_err(|_| StepError(format!("preset {row_id}: the JSON is not UTF-8")))?;
        let mut preset = deck_config_from_json(json)?;
        preset.config.initial_ease *= 100.0;
        write.execute((
            preset.id.0,
            &preset.name,
            preset.mtime.0,
            preset.usn.0,
            preset.config.to_bytes().as_ref(),
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique(names: &[&str], suffix: char) -> Vec<String> {
        let mut names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        make_unique(&mut names, suffix);
        names
    }

    #[test]
    fn duplicate_names_get_suffixes_in_list_order() {
        // U6.4's example.
        assert_eq!(unique(&["A", "a", "a+"], '+'), ["A", "a+", "a++"]);
        assert_eq!(unique(&["Card", "CARD"], '+'), ["Card", "CARD+"]);
        assert_eq!(unique(&["Straße", "STRASSE", "x"], '_'), ["Straße", "STRASSE_", "x"]);
        assert_eq!(unique(&["b", "a"], '+'), ["b", "a"]);
    }

    #[test]
    fn new_note_type_ids_are_in_range_and_unused() {
        #[derive(Debug)]
        struct Fixed(std::sync::Mutex<Vec<u64>>);
        impl RandomSource for Fixed {
            fn next_u64(&self) -> u64 {
                self.0.lock().unwrap().remove(0)
            }
        }
        // 0 and an id in use are drawn again.
        let random = Fixed(std::sync::Mutex::new(vec![0, 5 << 32, u64::MAX, 7 << 32]));
        let mut used = HashSet::from([5]);
        assert_eq!(new_notetype_id(&mut used, &random), 4_294_967_295);
        assert_eq!(new_notetype_id(&mut used, &random), 7);
    }
}
