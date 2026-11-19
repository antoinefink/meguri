//! Typed storage seams for `sync/one-way` O3/O6.2/O7.3 and
//! `sync/anki-transfers` M2/M3. No transfer admission or network policy.
//!
//! FullDownload needs no additional API: inside verified replacement's
//! caller transform, read `scope.col()`, set `col.last_sync = col.modified`,
//! then `scope.put_col(&col)`. The existing Raw scope preserves other stamps.

use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::RandomSource,
    time::EpochMillis,
    usn::Usn,
};

use crate::{
    OpenOptions, ReadSnapshot, Reader, StorageError, WriteScope,
    verified_replace::{self, Preparation, ReplaceError, ReplacementStage},
};

/// O3 pending edits, excluding reviews (reported separately). Counts are rows,
/// not distinct objects or changes since a local admission sequence.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PendingRows {
    pub content: PendingContent,
    pub definitions: PendingDefinitions,
    pub config: u64,
    pub graves: u64,
}

/// Study content includes reviewed cards whose row is still pending.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PendingContent {
    pub notes: u64,
    pub cards: u64,
}

/// Pending row counts for decks, deck options, note types, and tags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PendingDefinitions {
    pub decks: u64,
    pub deck_config: u64,
    pub notetypes: u64,
    pub tags: u64,
}

/// Collection-wide sync stamps, read alongside the O3 row counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionStamps {
    pub modified: EpochMillis,
    pub schema_modified: EpochMillis,
    pub last_sync: EpochMillis,
    pub usn: Usn,
}

/// All rows, regardless of their pending state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectionTotals {
    pub cards: u64,
    pub notes: u64,
    pub reviews: u64,
}

/// Local SQLite facts only. The engine/policy caller supplies remote facts,
/// sidecar sanity counts, generation and admission-sequence guards. SQLite
/// timestamps and USNs cannot establish those guards or transfer eligibility.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTransferFacts {
    pub stamps: CollectionStamps,
    pub totals: CollectionTotals,
    pub pending_reviews: u64,
    pub pending_rows: PendingRows,
    pub last_review: Option<EpochMillis>,
}

impl LocalTransferFacts {
    pub fn has_cards(&self) -> bool {
        self.totals.cards != 0
    }
}

/// `sync/cloudkit-recovery#V27.1`: the rows that make a collection hold user
/// content. Presets, config and tags do not count. The media rule (no tracked
/// file, no live media name) belongs to the caller, which owns that inventory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UserContentRows {
    pub notes: u64,
    pub cards: u64,
    pub reviews: u64,
    /// Decks other than id 1.
    pub other_decks: u64,
    /// Note types whose row is not a stock one as created (`mtime_secs` not 0).
    pub saved_notetypes: u64,
}

impl UserContentRows {
    /// True only when every count is zero. A failed read is never "none":
    /// the caller treats an error as user content.
    pub fn none(&self) -> bool {
        self.notes == 0 && self.cards == 0 && self.reviews == 0 && self.other_decks == 0 && self.saved_notetypes == 0
    }
}

fn user_content(conn: &rusqlite::Connection) -> Result<UserContentRows, StorageError> {
    // One statement, one read view: no table is counted later.
    Ok(conn.query_row(
        "SELECT (SELECT count(*) FROM notes),
         (SELECT count(*) FROM cards),
         (SELECT count(*) FROM revlog),
         (SELECT count(*) FROM decks WHERE id != 1),
         (SELECT count(*) FROM notetypes WHERE mtime_secs != 0)",
        [],
        |row| {
            let count = |index| -> rusqlite::Result<u64> {
                let count: i64 = row.get(index)?;
                u64::try_from(count).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, count))
            };
            Ok(UserContentRows {
                notes: count(0)?,
                cards: count(1)?,
                reviews: count(2)?,
                other_decks: count(3)?,
                saved_notetypes: count(4)?,
            })
        },
    )?)
}

impl Reader {
    /// V27.1's row test on the open collection, in one read view.
    pub fn user_content_rows(&self) -> Result<UserContentRows, StorageError> {
        user_content(&self.conn)
    }

    /// O3: a single SELECT fixes one consistent view for every fact, without
    /// any write or explicit transaction. An already-open snapshot is reused
    /// and remains owned by its caller; this never begins or ends it.
    pub fn transfer_facts(&self) -> Result<LocalTransferFacts, StorageError> {
        facts(&self.conn)
    }

    /// O6.2: prepare a new `collection.anki2` in an owned same-directory stage
    /// from the dedicated lease-backed Sync reader, outside a snapshot.
    /// Only this private copy is transformed, compacted, checked and synced.
    /// `at` is the single injected job instant; no row mtime is moved.
    ///
    /// Caller obligations: hold the engine lease through capture/preparation;
    /// drain writes, enforce generation/admission guards, preflight free space,
    /// own cancellation and the long-operation slot, and keep staging private
    /// and stable. Drop all readers and release the lease before replacement.
    /// No live lock is taken.
    /// Failure leaves unpublished output owned by the stage, never reusable.
    ///
    /// This is preparation exactly once. Stream and activate these exact bytes;
    /// do not call this or replacement's transforming `prepare` again for the
    /// accepted candidate. R6 activation/identity checks remain the caller's.
    /// ReplacementStage uses U32's swept temporary namespace: BEFORE upload can
    /// reach the server, the caller must durably retain independent exact bytes
    /// and their identity in operation-owned recovery storage OUTSIDE that
    /// namespace. Keep them through ambiguous acceptance or activation failure.
    /// This helper neither publishes a recovery artifact nor records intent/OK.
    pub fn prepare_full_upload(
        &self,
        stage: &ReplacementStage,
        at: EpochMillis,
        random: &dyn RandomSource,
    ) -> Result<(), ReplaceError> {
        let live = self.snapshot_full_upload_to(stage)?;
        let clock = FixedClock::new(at.0, UtcOffset::UTC);
        // Unmanaged open: never create/update the collection-directory sidecar.
        verified_replace::prepare_inner(
            stage,
            &live,
            &OpenOptions::new(&clock, random),
            &clock,
            Preparation::FullUpload,
            |scope| transform_upload(scope, at),
        )?;
        crate::atomic_file::sync_directory(stage.directory())?;
        crate::atomic_file::sync_directory(crate::atomic_file::parent(stage.directory()))?;
        Ok(())
    }
}

impl ReadSnapshot<'_> {
    /// V27.1's row test inside the caller's pinned view.
    pub fn user_content_rows(&self) -> Result<UserContentRows, StorageError> {
        user_content(self.conn())
    }

    /// Same O3 read, retaining the caller's pinned view and transaction.
    pub fn transfer_facts(&self) -> Result<LocalTransferFacts, StorageError> {
        facts(self.conn())
    }
}

fn facts(conn: &rusqlite::Connection) -> Result<LocalTransferFacts, StorageError> {
    // Scalar subqueries belong to this one SQLite statement/read view, even
    // when called without begin_snapshot(). No table may be counted later.
    Ok(conn.query_row(
        "SELECT mod, scm, ls, usn,
         (SELECT count(*) FROM cards),
         (SELECT count(*) FROM notes),
         (SELECT count(*) FROM revlog),
         (SELECT count(*) FROM revlog WHERE usn = -1),
         (SELECT count(*) FROM notes WHERE usn = -1),
         (SELECT count(*) FROM cards WHERE usn = -1),
         (SELECT count(*) FROM decks WHERE usn = -1),
         (SELECT count(*) FROM deck_config WHERE usn = -1),
         (SELECT count(*) FROM notetypes WHERE usn = -1),
         (SELECT count(*) FROM tags WHERE usn = -1),
         (SELECT count(*) FROM config WHERE usn = -1),
         (SELECT count(*) FROM graves WHERE usn = -1),
         (SELECT max(id) FROM revlog)
         FROM col WHERE id = 1",
        [],
        |row| {
            // Read each aggregate into i64, as supported by this rusqlite pin.
            // Checked conversion reports invalid negative counts to the caller.
            let count = |index| -> rusqlite::Result<u64> {
                let count: i64 = row.get(index)?;
                u64::try_from(count).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, count))
            };
            Ok(LocalTransferFacts {
                stamps: CollectionStamps {
                    modified: EpochMillis(row.get(0)?),
                    schema_modified: EpochMillis(row.get(1)?),
                    last_sync: EpochMillis(row.get(2)?),
                    usn: Usn(row.get(3)?),
                },
                totals: CollectionTotals {
                    cards: count(4)?,
                    notes: count(5)?,
                    reviews: count(6)?,
                },
                pending_reviews: count(7)?,
                pending_rows: PendingRows {
                    content: PendingContent {
                        notes: count(8)?,
                        cards: count(9)?,
                    },
                    definitions: PendingDefinitions {
                        decks: count(10)?,
                        deck_config: count(11)?,
                        notetypes: count(12)?,
                        tags: count(13)?,
                    },
                    config: count(14)?,
                    graves: count(15)?,
                },
                last_review: row.get::<_, Option<i64>>(16)?.map(EpochMillis),
            })
        },
    )?)
}

// Only reached on the owned candidate, in prepare_inner's journal-free,
// CallerSupplied scope. Never expose a live bulk-transform operation.
fn transform_upload(scope: &mut WriteScope<'_>, at: EpochMillis) -> Result<(), StorageError> {
    let usn: i32 = scope
        .conn()
        .query_row("SELECT usn FROM col WHERE id = 1", [], |row| row.get(0))?;
    let next_usn = usn.checked_add(1).ok_or_else(|| StorageError::Mode {
        detail: "full-upload col.usn overflow".into(),
    })?;
    // Update only named columns, retaining even legacy cell storage classes
    // rather than decoding/re-encoding unrelated col fields.
    scope.conn().execute(
        "UPDATE col SET usn = ?1, mod = ?2, scm = ?2, ls = ?2 WHERE id = 1",
        rusqlite::params![next_usn, at.0],
    )?;
    scope.conn().execute_batch(
        "DELETE FROM graves;
         UPDATE notes SET usn = 0 WHERE usn = -1;
         UPDATE cards SET usn = 0 WHERE usn = -1;
         UPDATE revlog SET usn = 0 WHERE usn = -1;
         UPDATE decks SET usn = 0 WHERE usn != 0;
         UPDATE deck_config SET usn = 0 WHERE usn != 0;
         UPDATE notetypes SET usn = 0 WHERE usn != 0;
         UPDATE tags SET usn = 0 WHERE usn != 0;",
    )?;
    scope.note_change();
    Ok(())
}

/// Read-only gate/refusals and integrity for a closed served file. No upgrade,
/// sidecar, journal-mode change or SQLite write is permitted here. Legacy
/// schemas can be packaged but have unknown O3 facts until explicitly upgraded.
pub fn verify_server_copy(path: &std::path::Path) -> Result<Option<LocalTransferFacts>, ReplaceError> {
    Ok(inspect_server_copy(path)?.map(|(facts, _)| facts))
}

/// The same read-only gate, with V27.1's row test of the served file. `None`
/// (a legacy schema) proves nothing: the caller treats it as user content.
pub fn server_copy_user_content(path: &std::path::Path) -> Result<Option<UserContentRows>, ReplaceError> {
    Ok(inspect_server_copy(path)?.map(|(_, rows)| rows))
}

fn inspect_server_copy(
    path: &std::path::Path,
) -> Result<Option<(LocalTransferFacts, UserContentRows)>, ReplaceError> {
    use crate::schema::{refuse, version};
    for suffix in ["-wal", "-shm", "-journal"] {
        if crate::connections::sibling(path, suffix).symlink_metadata().is_ok() {
            return Err(ReplaceError::StagingNotFinalised);
        }
    }
    let probed = version::probe_candidate(path).map_err(|error| match error {
        version::ProbeError::Open(crate::OpenError::Io { .. } | crate::OpenError::ConnectionInit { .. }) => {
            ReplaceError::Io(StorageError::Io {
                detail: "closed candidate probe could not finish".into(),
            })
        }
        version::ProbeError::Open(error) => ReplaceError::StagingRefused(error),
        version::ProbeError::HotJournal => ReplaceError::StagingCorrupt,
    })?;
    refuse::refuse(&probed).map_err(ReplaceError::StagingRefused)?;
    let conn = crate::rusqlite::Connection::open_with_flags(
        crate::connections::file_uri(path, Some("mode=ro&immutable=1")),
        crate::connections::READ_ONLY,
    )
    .map_err(StorageError::from)?;
    crate::conn_init::probe(&conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
    if !verified_replace::healthy(&conn)? {
        return Err(ReplaceError::StagingCorrupt);
    }
    let result = if probed.version == crate::schema::SCHEMA_VERSION {
        let objects = crate::schema::expected_objects::read(&conn).map_err(StorageError::from)?;
        let checked = crate::schema::expected_objects::check(&objects);
        if !checked.missing_tables.is_empty() {
            return Err(ReplaceError::StagingRefused(crate::OpenError::NotACollection {
                missing_tables: checked.missing_tables,
            }));
        }
        Some((facts(&conn)?, user_content(&conn)?))
    } else {
        None
    };
    conn.close().map_err(|(_, e)| StorageError::from(e))?;
    Ok(result)
}

/// Revalidate finalised O6.2 bytes without opening a writer or preparing again.
pub fn verify_prepared_upload(stage: &ReplacementStage, live: &std::path::Path) -> Result<(), ReplaceError> {
    use std::io::Read;
    verified_replace::check_device(stage, live)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        if crate::connections::sibling(&stage.path(), suffix)
            .symlink_metadata()
            .is_ok()
        {
            return Err(ReplaceError::StagingNotFinalised);
        }
    }
    let mut header = [0u8; 20];
    std::fs::File::open(stage.path())?.read_exact(&mut header)?;
    if header[18..20] != [1, 1] {
        return Err(ReplaceError::StagingNotFinalised);
    }
    let facts = verify_server_copy(&stage.path())?.ok_or(ReplaceError::StagingNotFinalised)?;
    if facts.stamps.modified != facts.stamps.schema_modified || facts.stamps.modified != facts.stamps.last_sync {
        return Err(ReplaceError::StagingNotFinalised);
    }
    Ok(())
}
