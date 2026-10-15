//! Step 16 (`storage/schema-upgrade` U13): the starting ease divided by 100;
//! presets at or below 1.3 reset to 2.5, and the low card eases of the decks
//! using them raised to 2500.

use std::collections::HashSet;

use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DeckKind, DeckKindMessage};
use meguri_model::proto::deck_config::DeckConfigMessage;
use rusqlite::Connection;

use super::{StepError, StepResult};

/// The starting ease at or below which a preset is reset (U13).
const LOW_EASE: f32 = 1.3;
const RESET_EASE: f32 = 2.5;

pub(super) fn run(conn: &Connection) -> StepResult {
    let reset = presets(conn)?;
    if !reset.is_empty() {
        cards(conn, &decks_using(conn, &reset)?)?;
    }
    Ok(())
}

/// U13.1: each preset rewritten with its ease ÷100 (no clamping); returns
/// the presets reset to 2.5.
fn presets(conn: &Connection) -> Result<HashSet<i64>, StepError> {
    let rows = {
        let mut select = conn.prepare("SELECT id, config FROM deck_config ORDER BY id")?;
        let rows = select.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut update = conn.prepare("UPDATE deck_config SET config = ?2 WHERE id = ?1")?;
    let mut reset = HashSet::new();
    for (id, bytes) in rows {
        let mut config = ProtoBlob::<DeckConfigMessage>::decode(&bytes)?;
        let ease = config.initial_ease / 100.0;
        config.initial_ease = if ease <= LOW_EASE {
            reset.insert(id);
            RESET_EASE
        } else {
            ease
        };
        update.execute((id, config.to_bytes().as_ref()))?;
    }
    Ok(reset)
}

/// The normal decks whose preset is one of `presets`.
fn decks_using(conn: &Connection, presets: &HashSet<i64>) -> Result<Vec<i64>, StepError> {
    let mut select = conn.prepare("SELECT id, kind FROM decks ORDER BY id")?;
    let rows = select.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
    let mut decks = Vec::new();
    for row in rows {
        let (id, bytes) = row?;
        let kind = ProtoBlob::<DeckKindMessage>::decode(&bytes)?;
        if let DeckKind::Normal(normal) = &kind.kind
            && presets.contains(&normal.config_id)
        {
            decks.push(id);
        }
    }
    Ok(decks)
}

/// U13.2: cards in (or filtered out of) those decks with a factor that is not
/// 0 and at most 2000 get 2500 and usn -1; `mod` stays.
fn cards(conn: &Connection, decks: &[i64]) -> StepResult {
    let mut update = conn.prepare(
        "UPDATE cards SET factor = 2500, usn = -1 WHERE factor != 0 AND factor <= 2000 AND (did = ?1 OR odid = ?1)",
    )?;
    for deck in decks {
        update.execute([deck])?;
    }
    Ok(())
}
