//! Connection settings (`storage/open-and-connections` C4, C6, C9).

use std::time::Duration;

use rusqlite::Connection;

/// Anki's page cache: 40 MiB (a negative `cache_size` is KiB).
const CACHE_SIZE_KIB: i64 = -40_960;
/// Prepared statements kept per connection (`prepare_cached`).
const STATEMENT_CACHE: usize = 128;
const WRITER_BUSY_TIMEOUT: Duration = Duration::from_millis(1000);
const READER_BUSY_TIMEOUT: Duration = Duration::from_millis(250);

/// The writer's settings, in C4's order. Fails when the file does not end
/// in WAL mode.
pub(crate) fn writer(conn: &Connection) -> Result<(), WriterSettingsError> {
    conn.busy_timeout(WRITER_BUSY_TIMEOUT)?;
    // NORMAL is SQLite's default; never EXCLUSIVE, so readers on other
    // connections can read (R1452).
    conn.execute_batch(&format!(
        "PRAGMA locking_mode = NORMAL;
         PRAGMA page_size = 4096;
         PRAGMA cache_size = {CACHE_SIZE_KIB};
         PRAGMA temp_store = MEMORY;"
    ))?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(WriterSettingsError::NotWal(mode));
    }
    durability(conn)?;
    conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE);
    Ok(())
}

/// C9: `synchronous=NORMAL`: a commit returns without a sync, and U32's
/// flush makes it durable within 1 s (C9.2). The syncs SQLite still issues
/// (checkpoints, the WAL header on restart) are full ones on Apple, where a
/// plain `fsync` does not flush the drive's cache.
fn durability(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("PRAGMA synchronous = NORMAL;")?;
    #[cfg(target_vendor = "apple")]
    conn.execute_batch("PRAGMA fullfsync = ON; PRAGMA checkpoint_fullfsync = ON;")?;
    Ok(())
}

/// A reader's settings (C6). `query_only` refuses every write, temporary
/// tables included; the sync and maintenance readers go without it because
/// `VACUUM INTO` fails under it.
pub(crate) fn reader(conn: &Connection, query_only: bool) -> rusqlite::Result<()> {
    conn.busy_timeout(READER_BUSY_TIMEOUT)?;
    conn.execute_batch(&format!(
        "PRAGMA cache_size = {CACHE_SIZE_KIB};
         PRAGMA temp_store = MEMORY;
         PRAGMA query_only = {};",
        if query_only { "ON" } else { "OFF" }
    ))?;
    conn.set_prepared_statement_cache_capacity(STATEMENT_CACHE);
    Ok(())
}

#[derive(Debug)]
pub(crate) enum WriterSettingsError {
    Sqlite(rusqlite::Error),
    /// The journal mode SQLite reports instead of `wal`.
    NotWal(String),
}

impl From<rusqlite::Error> for WriterSettingsError {
    fn from(error: rusqlite::Error) -> WriterSettingsError {
        WriterSettingsError::Sqlite(error)
    }
}
