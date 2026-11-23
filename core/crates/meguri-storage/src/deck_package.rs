//! The staged collection of a deck package (`package/apkg-import` I3 steps
//! 3 to 7): the open gate with the v1 marker let through, the integrity
//! check, then id repair (I3.1), scheduler conversion (I3.2) and field
//! count repair (I3.3) in one transaction on that private file. Nothing
//! here touches an open collection: the caller owns the staging directory.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use meguri_model::proto::deck::DeckKind;
use rusqlite::{Connection, params};

use crate::package_inspection::{CandidateContext, CandidateError};
use crate::schema::{SCHEMA_VERSION, refuse, version};
use crate::{OpenError, OpenOptions, ReadRows, StorageError, open_collection};

/// The most fields a package note type may have for I3.3's repair.
const MAX_FIELDS: usize = 5000;

/// One day, in milliseconds: how far past now an id may lie (I3.1).
const DAY_MS: i64 = 86_400_000;

/// What the caller supplies besides the candidate context.
pub struct DeckCandidateRepair<'a> {
    /// The device's offset at an instant (epoch seconds), in minutes west:
    /// the creation offset a converted v1 collection without one gets.
    pub minutes_west_at: &'a dyn Fn(i64) -> i32,
    /// `notes/note-records` F1 on a note's values for a field count.
    pub repair_fields: &'a dyn Fn(Vec<String>, usize) -> Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeckCandidateFacts {
    pub ver_found: i64,
    pub notes: u64,
    pub cards: u64,
    pub revlog: u64,
    pub new_note_ids: u64,
    pub new_card_ids: u64,
    pub new_revlog_ids: u64,
    pub repaired_field_counts: u64,
    pub scheduler_converted: bool,
}

fn cancelled(context: &CandidateContext<'_>) -> Result<(), CandidateError> {
    if (context.is_cancelled)() {
        Err(CandidateError::Cancelled)
    } else {
        Ok(())
    }
}

fn corrupt(detail: &str) -> CandidateError {
    CandidateError::Corrupt { detail: detail.into() }
}

/// The gate of I3 step 3, the check of step 4 and the repairs of steps 5 to
/// 7. Every connection is closed when it returns.
pub fn prepare_deck_candidate(
    path: &Path,
    context: &CandidateContext<'_>,
    repair: &DeckCandidateRepair<'_>,
) -> Result<DeckCandidateFacts, CandidateError> {
    cancelled(context)?;
    let probed = version::probe_candidate(path).map_err(|error| match error {
        version::ProbeError::Open(error) => CandidateError::Gate(error),
        version::ProbeError::HotJournal => corrupt("fresh package candidate has a hot journal"),
    })?;
    refuse::refuse_as(&probed, true).map_err(CandidateError::Gate)?;
    if probed.version < SCHEMA_VERSION {
        let needed = std::fs::metadata(path).map_err(StorageError::from)?.len();
        let directory = path.parent().ok_or_else(|| corrupt("candidate directory"))?;
        crate::space::require_space(context.space, directory, needed).map_err(|error| match error {
            StorageError::InsufficientSpace { required, available } => CandidateError::InsufficientSpace {
                needed: required,
                available,
            },
            other => other.into(),
        })?;
    }
    cancelled(context)?;
    let options = OpenOptions::new(context.clock, context.random).package_collection();
    let opened = open_collection(path, &options).map_err(gate)?;
    cancelled(context)?;
    let v1 = probed.scheduler == version::Scheduler::V1;
    let outcome = repair_opened(&opened, probed.version, v1, context, repair);
    let closed = opened.close();
    let facts = outcome?;
    if !closed?.succeeded() {
        return Err(corrupt("candidate checkpoint busy"));
    }
    cancelled(context)?;
    Ok(facts)
}

fn gate(error: OpenError) -> CandidateError {
    let full = match &error {
        OpenError::Io { detail } | OpenError::UpgradeFailed { detail, .. } => {
            detail.contains("database or disk is full") || detail.contains("No space left on device")
        }
        _ => false,
    };
    if full { CandidateError::DiskFull } else { CandidateError::Gate(error) }
}

fn repair_opened(
    opened: &crate::OpenedCollection,
    ver_found: i64,
    v1: bool,
    context: &CandidateContext<'_>,
    repair: &DeckCandidateRepair<'_>,
) -> Result<DeckCandidateFacts, CandidateError> {
    let conn = &opened.writer().conn;
    {
        let mut statement = conn.prepare("PRAGMA main.integrity_check")?;
        let mut rows = statement.query([])?;
        let first = rows.next()?.map(|r| r.get::<_, String>(0)).transpose()?;
        if first.as_deref() != Some("ok") || rows.next()?.is_some() {
            return Err(corrupt("candidate integrity_check"));
        }
    }
    cancelled(context)?;
    // The file is a private copy that is read, never activated: triggers
    // and views a package brought could rewrite the rows the repairs below
    // and the import read, so they go first.
    let extras: Vec<(String, String)> = conn
        .prepare("SELECT type, name FROM sqlite_master WHERE type IN ('trigger', 'view')")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (kind, name) in extras {
        let kind = if kind == "view" { "VIEW" } else { "TRIGGER" };
        conn.execute_batch(&format!("DROP {kind} IF EXISTS \"{}\"", name.replace('"', "\"\"")))?;
    }
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let run = || -> Result<DeckCandidateFacts, CandidateError> {
        let now_ms = context.clock.now_ms();
        let new_note_ids = repair_ids(conn, "notes", Some(("cards", "nid")), now_ms, context)?;
        let new_card_ids = repair_ids(conn, "cards", Some(("revlog", "cid")), now_ms, context)?;
        let new_revlog_ids = repair_ids(conn, "revlog", None, now_ms, context)?;
        let scheduler_converted = v1 && convert_scheduler(opened, now_ms.div_euclid(1000), context, repair)?;
        let repaired_field_counts = repair_field_counts(conn, context, repair)?;
        let count = |table: &str| -> Result<u64, CandidateError> {
            let count: i64 = conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
            u64::try_from(count).map_err(|_| corrupt("candidate row count"))
        };
        Ok(DeckCandidateFacts {
            ver_found,
            notes: count("notes")?,
            cards: count("cards")?,
            revlog: count("revlog")?,
            new_note_ids,
            new_card_ids,
            new_revlog_ids,
            repaired_field_counts,
            scheduler_converted,
        })
    };
    match run() {
        Ok(facts) => {
            conn.execute_batch("COMMIT")?;
            Ok(facts)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

/// I3.1 for one table: rows above the cutoff, in ascending id, take the ids
/// from `now_ms` upward that no row at or below the cutoff holds. `child`
/// names the table and column that refer to this table's ids.
fn repair_ids(
    conn: &Connection,
    table: &str,
    child: Option<(&str, &str)>,
    now_ms: i64,
    context: &CandidateContext<'_>,
) -> Result<u64, CandidateError> {
    let cutoff = now_ms.saturating_add(DAY_MS);
    let ahead: Vec<i64> = conn
        .prepare(&format!("SELECT id FROM {table} WHERE id > ?1 ORDER BY id"))?
        .query_map([cutoff], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if ahead.is_empty() {
        return Ok(0);
    }
    let mut taken: BTreeSet<i64> = conn
        .prepare(&format!("SELECT id FROM {table} WHERE id >= ?1 AND id <= ?2"))?
        .query_map([now_ms, cutoff], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    if let Some((child, column)) = child {
        // An id some child row names without a parent stays unused, so a
        // repaired row never adopts another row's orphans.
        let orphaned: Vec<i64> = conn
            .prepare(&format!(
                "SELECT DISTINCT {column} FROM {child} WHERE {column} >= ?1 AND {column} <= ?2 \
                 AND {column} NOT IN (SELECT id FROM {table})"
            ))?
            .query_map([now_ms, cutoff], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        taken.extend(orphaned);
    }
    let mut next = now_ms;
    let mut update = conn.prepare(&format!("UPDATE {table} SET id = ?1 WHERE id = ?2"))?;
    let mut follow = match child {
        Some((child, column)) => Some(conn.prepare(&format!("UPDATE {child} SET {column} = ?1 WHERE {column} = ?2"))?),
        None => None,
    };
    for old in &ahead {
        cancelled(context)?;
        while taken.contains(&next) {
            next += 1;
        }
        if next > cutoff {
            // More than a day of milliseconds of rows: not a real package.
            return Err(corrupt("too many ids ahead of the clock"));
        }
        update.execute(params![next, old])?;
        if let Some(follow) = &mut follow {
            follow.execute(params![next, old])?;
        }
        next += 1;
    }
    Ok(ahead.len() as u64)
}

struct OldCard {
    id: i64,
    deck: i64,
    home: i64,
    kind: i64,
    queue: i64,
    due: i64,
    left: i64,
}

/// I3.2 for a collection whose marker means v1. Returns whether anything
/// was converted.
fn convert_scheduler(
    opened: &crate::OpenedCollection,
    now_s: i64,
    context: &CandidateContext<'_>,
    repair: &DeckCandidateRepair<'_>,
) -> Result<bool, CandidateError> {
    let writer = opened.writer();
    let conn = &writer.conn;
    let studied: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM cards WHERE queue <> 0)", [], |r| r.get(0))?;
    if !studied {
        return Ok(false);
    }
    // 1. Review log.
    conn.execute("UPDATE revlog SET ease = ease + 1 WHERE type IN (0, 2) AND ease IN (2, 3)", [])?;
    // 2. Cards: decided from the rows as they are now.
    let decks = writer.deck_rows()?;
    let mut filtered: HashMap<i64, (bool, bool)> = HashMap::new();
    let mut presets: HashMap<i64, i64> = HashMap::new();
    for deck in &decks {
        match &deck.kind.kind {
            DeckKind::Filtered(kind) => {
                filtered.insert(deck.id.0, (kind.reschedule, !kind.delays.is_empty()));
            }
            DeckKind::Normal(kind) => {
                presets.insert(deck.id.0, kind.config_id);
            }
        }
    }
    let steps: HashMap<i64, (usize, usize)> = writer
        .deck_config_rows()?
        .into_iter()
        .map(|preset| (preset.id.0, (preset.config.learn_steps.len(), preset.config.relearn_steps.len())))
        .collect();
    let cards: Vec<OldCard> = conn
        .prepare("SELECT id, did, odid, type, queue, due, left FROM cards ORDER BY id")?
        .query_map([], |r| {
            Ok(OldCard {
                id: r.get(0)?,
                deck: r.get(1)?,
                home: r.get(2)?,
                kind: r.get(3)?,
                queue: r.get(4)?,
                due: r.get(5)?,
                left: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut update = conn.prepare("UPDATE cards SET type = ?1, queue = ?2, left = ?3 WHERE id = ?4")?;
    for card in cards {
        cancelled(context)?;
        let held = filtered.get(&card.deck).copied();
        if held.is_none() && !matches!(card.kind, 2 | 3) {
            continue;
        }
        let (mut kind, mut queue, mut left) = (card.kind, card.queue, card.left);
        // 2.1
        if card.kind == 2 && matches!(card.queue, 1 | 3) {
            kind = 3;
        }
        // 2.2
        if let Some((reschedule, delays)) = held
            && card.home != 0
        {
            if delays {
                let preset = presets.get(&card.home).copied().unwrap_or(1);
                let count = steps
                    .get(&preset)
                    .map_or(0, |(learn, relearn)| if card.kind == 2 { *relearn } else { *learn });
                left = left.min(i64::try_from(count).unwrap_or(i64::MAX));
            }
            if reschedule {
                if queue == 0 && kind != 0 {
                    queue = match kind {
                        2 => 2,
                        _ if card.due > 1_000_000_000 => 1,
                        _ => 3,
                    };
                }
            } else {
                if queue == 0 {
                    queue = 2;
                }
                if kind == 1 {
                    queue = 4;
                    kind = 0;
                }
            }
        }
        if (kind, queue, left) != (card.kind, card.queue, card.left) {
            update.execute(params![kind, queue, left, card.id])?;
        }
    }
    // 3. Config.
    conn.execute(
        "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('schedVer', -1, ?1, CAST('2' AS BLOB)) \
         ON CONFLICT(KEY) DO UPDATE SET usn = -1, mtime_secs = ?1, val = CAST('2' AS BLOB)",
        [now_s],
    )?;
    let offset = writer
        .config_value("creationOffset")?
        .and_then(|value| value.as_i64())
        .filter(|minutes| (-1439..=1439).contains(minutes));
    if offset.is_none() {
        let created = writer.col()?.created.0;
        let minutes = (repair.minutes_west_at)(created);
        conn.execute(
            "INSERT INTO config (KEY, usn, mtime_secs, val) VALUES ('creationOffset', -1, ?1, CAST(?2 AS BLOB)) \
             ON CONFLICT(KEY) DO UPDATE SET usn = -1, mtime_secs = ?1, val = CAST(?2 AS BLOB)",
            params![now_s, minutes.to_string()],
        )?;
    }
    Ok(true)
}

/// I3.3: a note whose value count differs from its note type's field count
/// is repaired by F1; a note of an unknown note type is left alone.
fn repair_field_counts(
    conn: &Connection,
    context: &CandidateContext<'_>,
    repair: &DeckCandidateRepair<'_>,
) -> Result<u64, CandidateError> {
    // Separators are counted in bytes on both sides: `length` of text
    // counts characters, which would miscount any note with a multi-byte
    // character. Pages keep the rows held at once bounded.
    let mut select = conn.prepare(
        "SELECT n.id, n.flds, f.fields FROM notes n \
         JOIN (SELECT ntid, count(*) AS fields FROM fields GROUP BY ntid) f ON f.ntid = n.mid \
         WHERE n.id > ?1 AND length(CAST(n.flds AS BLOB)) \
             - length(CAST(replace(CAST(n.flds AS TEXT), char(31), '') AS BLOB)) + 1 <> f.fields \
         ORDER BY n.id LIMIT 200",
    )?;
    let mut update = conn.prepare("UPDATE notes SET flds = ?1 WHERE id = ?2")?;
    let mut repaired = 0;
    let mut after = i64::MIN;
    loop {
        cancelled(context)?;
        let page: Vec<(i64, String, i64)> = select
            .query_map([after], |r| {
                let text = match r.get_ref(1)? {
                    rusqlite::types::ValueRef::Text(bytes) | rusqlite::types::ValueRef::Blob(bytes) => {
                        String::from_utf8_lossy(bytes).into_owned()
                    }
                    _ => String::new(),
                };
                Ok((r.get(0)?, text, r.get(2)?))
            })?
            .collect::<Result<_, _>>()?;
        let Some(last) = page.last() else { break };
        after = last.0;
        for (id, fields, wanted) in &page {
            let wanted = usize::try_from(*wanted).map_err(|_| corrupt("field count"))?;
            // No real note type comes near this: a crafted count would make
            // every repaired note that many values long.
            if wanted > MAX_FIELDS {
                return Err(corrupt("note type field count"));
            }
            let values: Vec<String> = fields.split('\u{1f}').map(str::to_owned).collect();
            if wanted == 0 || values.len() == wanted {
                continue;
            }
            let fixed = (repair.repair_fields)(values, wanted);
            update.execute(params![fixed.join("\u{1f}"), id])?;
            repaired += 1;
        }
    }
    Ok(repaired)
}

/// Opens a prepared candidate for reading its rows (the plan of an import).
/// The v1 marker stays accepted: an unconverted v1 package whose cards are
/// all new keeps it (I3.2).
pub fn open_deck_candidate(
    path: &Path,
    clock: &dyn meguri_model::clock::Clock,
    random: &dyn meguri_model::rng::RandomSource,
) -> Result<crate::OpenedCollection, OpenError> {
    open_collection(path, &OpenOptions::new(clock, random).package_collection())
}

/// What the rows of a prepared candidate refer to (`package/apkg-import`
/// I4): read in one pass, without the notes' text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeckCandidateRefs {
    /// Every `did` and non-zero `odid` a card names, ascending.
    pub decks: Vec<i64>,
    /// Every `mid` a note names, ascending.
    pub notetypes: Vec<i64>,
    pub studied_card: bool,
    pub revlog_row: bool,
}

pub fn deck_candidate_refs(opened: &crate::OpenedCollection) -> Result<DeckCandidateRefs, StorageError> {
    let conn = &opened.writer().conn;
    let list = |sql: &str| -> Result<Vec<i64>, StorageError> {
        Ok(conn
            .prepare(sql)?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?)
    };
    Ok(DeckCandidateRefs {
        decks: list("SELECT did FROM cards UNION SELECT odid FROM cards WHERE odid <> 0 ORDER BY 1")?,
        notetypes: list("SELECT DISTINCT mid FROM notes ORDER BY 1")?,
        studied_card: conn.query_row("SELECT EXISTS(SELECT 1 FROM cards WHERE queue <> 0)", [], |r| r.get(0))?,
        revlog_row: conn.query_row("SELECT EXISTS(SELECT 1 FROM revlog)", [], |r| r.get(0))?,
    })
}
