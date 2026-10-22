//! Storage-owned standalone snapshot and authored compatibility notice (P7/P11).
//! Paths are new files inside the caller's U32 operation-owned staging directory.
use std::path::Path;

use meguri_model::{
    clock::Clock,
    deck::Deck,
    deck_config::DeckConfig,
    ids::{DeckConfigId, DeckId, NotetypeId},
    notetype::{Notetype, NotetypeField, NotetypeTemplate},
    proto::{
        ProtoBlob,
        deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck},
        deck_config::DeckConfigMessage,
        notetype::{FieldConfig, NotetypeConfig, TemplateConfig},
    },
    rng::RandomSource,
    schema11::col as schema11,
    time::EpochSecs,
    usn::Usn,
};
use rusqlite::{Connection, OpenFlags, params};

use crate::{
    StorageError,
    connections::{READ_ONLY, file_uri, sibling},
};

#[derive(Debug)]
pub enum StandaloneSnapshotError {
    SourceNotFinalised,
    Storage(StorageError),
}
impl std::fmt::Display for StandaloneSnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SourceNotFinalised => f.write_str("source database has an associated WAL or journal file"),
            Self::Storage(error) => std::fmt::Display::fmt(error, f),
        }
    }
}
impl std::error::Error for StandaloneSnapshotError {}
impl From<StorageError> for StandaloneSnapshotError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

/// Reject WAL/journal paths, including empty files or dangling links. Keep the
/// verified, closed source stable throughout this check and the snapshot.
pub fn standalone_size(source: &Path) -> Result<u64, StandaloneSnapshotError> {
    for suffix in ["-wal", "-journal"] {
        match sibling(source, suffix).symlink_metadata() {
            Ok(_) => return Err(StandaloneSnapshotError::SourceNotFinalised),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(StorageError::from(error).into()),
        }
    }
    Ok(std::fs::metadata(source).map_err(StorageError::from)?.len())
}

/// VACUUM reads a single consistent view. Immutable read-only opening avoids
/// creating SHM/WAL or touching a device-only sidecar on the closed source.
pub fn snapshot_standalone(source: &Path, destination: &Path) -> Result<(), StandaloneSnapshotError> {
    standalone_size(source)?;
    snapshot(source, destination).map_err(Into::into)
}
fn snapshot(source: &Path, destination: &Path) -> Result<(), StorageError> {
    let conn = Connection::open_with_flags(file_uri(source, Some("mode=ro&immutable=1")), READ_ONLY)?;
    crate::conn_init::probe(&conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
    conn.execute("VACUUM main INTO ?1", [file_uri(destination, None)])?;
    conn.close().map_err(|(_, error)| StorageError::from(error))?;
    finalise_snapshot(destination)
}

/// Finalise only an unpublished snapshot owned by the caller's staging
/// directory, never a source collection or a published destination.
pub(crate) fn finalise_snapshot(destination: &Path) -> Result<(), StorageError> {
    // VACUUM may produce rollback mode. SQLite owns this conversion and the
    // final checkpoint/close; never patch the header of a database by hand.
    let copy = Connection::open_with_flags(destination, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    crate::conn_init::probe(&copy).map_err(|detail| StorageError::ConnectionInit { detail })?;
    let mode: String = copy.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
    if mode != "wal" {
        return Err(StorageError::Io {
            detail: "snapshot did not enter WAL mode".into(),
        });
    }
    let busy: i64 = copy.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))?;
    if busy != 0 {
        return Err(StorageError::Io {
            detail: "snapshot checkpoint was busy".into(),
        });
    }
    copy.close().map_err(|(_, error)| StorageError::from(error))?;
    for suffix in ["-wal", "-shm", "-journal"] {
        match sibling(destination, suffix).symlink_metadata() {
            Ok(_) => {
                return Err(StorageError::Io {
                    detail: "snapshot retained a SQLite auxiliary file".into(),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub const NOTICE: &str =
    "This file needs a newer version of Anki. Please update to Anki 2.1.50 or later, then import the file again.";

/// Authored schema-11 notice; no fixture bytes or stock third-party assets.
pub fn create_placeholder(path: &Path, clock: &dyn Clock, random: &dyn RandomSource) -> Result<(), StorageError> {
    // Claim only a new staging file; an existing file is never opened writable.
    let claim = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    drop(claim);
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    crate::conn_init::probe(&conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
    conn.execute_batch("PRAGMA page_size=512; PRAGMA journal_mode=DELETE; BEGIN IMMEDIATE;")?;
    for name in [
        "col",
        "notes",
        "cards",
        "revlog",
        "ix_notes_usn",
        "ix_cards_usn",
        "ix_revlog_usn",
        "ix_cards_nid",
        "ix_cards_sched",
        "ix_revlog_cid",
        "ix_notes_csum",
    ] {
        conn.execute_batch(crate::schema::ddl18(name))?;
    }
    conn.execute_batch("CREATE TABLE graves ( usn integer NOT NULL, oid integer NOT NULL, type integer NOT NULL )")?;
    let now = clock.now_ms();
    let seconds = now.div_euclid(1000);
    let mut config = NotetypeConfig::default();
    config.target_deck_id_unused = 1;
    let mut requirement = meguri_model::proto::notetype::CardRequirement::default();
    requirement.kind = 1;
    requirement.field_ords = vec![0];
    config.reqs = vec![requirement];
    config.css = ".card { font-family: system-ui, sans-serif; font-size: 18px; margin: 2em; }".into();
    let mut field = FieldConfig::default();
    field.font_name = "system-ui".into();
    field.font_size = 18;
    let mut template = TemplateConfig::default();
    template.q_format = "{{Text}}".into();
    template.a_format = "{{Text}}".into();
    let notetype = Notetype {
        id: NotetypeId(1),
        name: "Notice".into(),
        mtime: EpochSecs(seconds),
        usn: Usn(0),
        config: ProtoBlob::new(config),
        fields: vec![NotetypeField {
            ord: 0,
            name: "Text".into(),
            config: ProtoBlob::new(field),
            prior_ord: Some(0),
        }],
        templates: vec![NotetypeTemplate {
            ord: 0,
            name: "Card 1".into(),
            mtime: EpochSecs(0),
            usn: Usn(0),
            config: ProtoBlob::new(template),
            prior_ord: Some(0),
        }],
    };
    let mut common = DeckCommon::default();
    common.study_collapsed = true;
    common.browser_collapsed = true;
    let mut normal = NormalDeck::default();
    normal.config_id = 1;
    let deck = Deck {
        id: DeckId(1),
        name: "Default".into(),
        mtime: EpochSecs(0),
        usn: Usn(0),
        common: ProtoBlob::new(common),
        kind: ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal))),
    };
    let preset = DeckConfig {
        id: DeckConfigId(1),
        name: "Default".into(),
        mtime: EpochSecs(0),
        usn: Usn(0),
        config: ProtoBlob::new(DeckConfigMessage::builtin_defaults()),
    };
    let conf = r#"{"activeDecks":[1],"addToCur":true,"collapseTime":1200,"curDeck":1,"curModel":1,"dueCounts":true,"estTimes":true,"newSpread":0,"nextPos":2,"schedVer":2,"sortBackwards":false,"sortType":"noteFld","timeLim":0}"#;
    conn.execute(
        "INSERT INTO col VALUES (1,?1,?2,?2,11,0,0,0,?3,?4,?5,?6,'{}')",
        params![
            seconds.div_euclid(86_400) * 86_400,
            now,
            conf,
            schema11::notetypes_to_json(&[notetype]),
            schema11::decks_to_json(&[deck]),
            schema11::deck_configs_to_json(&[preset])
        ],
    )?;
    let guid = meguri_text::base91::encode(random.next_u64());
    conn.execute(
        "INSERT INTO notes VALUES (?1,?2,1,?3,0,'',?4,?5,?6,0,'')",
        params![
            now,
            guid,
            seconds,
            NOTICE,
            meguri_text::checksum::sort_text(NOTICE).as_ref(),
            meguri_text::checksum::note_checksum(NOTICE)
        ],
    )?;
    conn.execute(
        "INSERT INTO cards VALUES (?1,?1,1,0,?2,0,0,0,1,0,0,0,0,0,0,0,0,'')",
        params![now, seconds],
    )?;
    conn.execute_batch("COMMIT")?;
    conn.close().map_err(|(_, error)| StorageError::from(error))?;
    Ok(())
}
