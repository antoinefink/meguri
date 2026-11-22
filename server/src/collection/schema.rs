// SPDX-License-Identifier: MIT
//! File-compatible collections; account identity and publication live elsewhere.
use crate::{Error, Result};
use rusqlite::{Connection, OpenFlags, config::DbConfig, params};
use std::{fs::File, path::Path, time::Duration};

pub const TABLES: &[&str] = &["col", "notes", "cards", "revlog", "graves", "deck_config", "config", "tags", "fields", "templates", "notetypes", "decks"];
pub(crate) fn invalid() -> Error { Error::new("invalid_collection") }

/// Uploaded triggers, views and foreign-key actions stay disabled, including
/// during integrity checking, backup and the owned stamp update. Keep their bytes.
pub fn open(path: &Path, writable: bool) -> Result<Connection> {
    crate::storage::private_collection_file(path)?;
    let flags = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | if writable { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    configure(Connection::open_with_flags(path, flags)?, writable)
}

/// A freshly staged main-file upload has no auxiliary database files. Immutable
/// read-only access also handles WAL-format main files without creating WAL/SHM
/// or changing their journal mode. Integrity-check temporary tables stay in RAM.
/// This connection must close before the candidate is published or mutated.
pub(crate) fn open_upload(path: &Path) -> Result<Connection> {
    crate::storage::private_collection_file(path)?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name=path.as_os_str().to_os_string();name.push(suffix);
        if Path::new(&name).try_exists()? { return Err(invalid()); }
    }
    let mut uri=url::Url::from_file_path(path).map_err(|_|invalid())?;
    uri.query_pairs_mut().append_pair("immutable","1");
    let c=configure(Connection::open_with_flags(uri.as_str(), OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW | OpenFlags::SQLITE_OPEN_URI)?,false)?;
    c.pragma_update(None,"temp_store",2)?;
    if c.pragma_query_value(None,"temp_store",|r|r.get::<_,i64>(0))?!=2 { return Err(Error::new("storage_unavailable")); }
    Ok(c)
}

/// Page-local request open: no integrity scan, copy, hash, or migration. The
/// publication adapter has already admitted and pinned a verified schema-18 file.
pub(super) fn open_mcp(path: &Path, writable: bool) -> Result<Connection> {
    use crate::error::McpDiagnosticDetail as Detail;
    crate::storage::private_collection_file(path).map_err(|e| e.mcp_storage_detail(Detail::CollectionStorage))?;
    let flags = OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW
        | if writable { OpenFlags::SQLITE_OPEN_READ_WRITE } else { OpenFlags::SQLITE_OPEN_READ_ONLY };
    let c = Connection::open_with_flags(path, flags).map_err(super::mcp::sql_open_error)?;
    // Request latency must not inherit a large native WAL. Maintenance owns
    // checkpointing outside point operations, including the last-handle close.
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true).map_err(super::mcp::sql_open_error)?;
    let c = configure_timeout_with(c, false, Duration::ZERO, super::mcp::sql_open_storage_error)
        .map_err(|e| e.mcp_storage_detail(Detail::CollectionOpen))?;
    if writable {
        c.pragma_update(None, "wal_autocheckpoint", 0).map_err(super::mcp::sql_open_error)?;
        c.pragma_update(None, "journal_mode", "WAL").map_err(super::mcp::sql_open_error)?;
        c.pragma_update(None, "synchronous", "FULL").map_err(super::mcp::sql_open_error)?;
        let mode: String = c.pragma_query_value(None, "journal_mode", |r|r.get(0)).map_err(super::mcp::sql_open_error)?;
        let sync: i64 = c.pragma_query_value(None, "synchronous", |r|r.get(0)).map_err(super::mcp::sql_open_error)?;
        if mode != "wal" || sync != 2 { return Err(Error::new("storage_unavailable").with_mcp_detail(Detail::CollectionOpen)); }
    }
    Ok(c)
}
fn configure(c: Connection, writable: bool) -> Result<Connection> {
    configure_timeout(c, writable, Duration::from_secs(5))
}
fn configure_timeout(c: Connection, writable: bool, timeout: Duration) -> Result<Connection> {
    configure_timeout_with(c, writable, timeout, Error::from)
}
fn configure_timeout_with(c: Connection, writable: bool, timeout: Duration,
    sql_error: fn(rusqlite::Error) -> Error) -> Result<Connection> {
    // Set this before any SQL can inspect a locked schema. A request connection
    // must never use rusqlite's default five-second wait during configuration.
    c.busy_timeout(timeout).map_err(sql_error)?;
    // Extension loading is absent from the selected rusqlite features; do not
    // enable its API merely to disable it. SQLite connections start disabled.
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, false).map_err(sql_error)?;
    if c.pragma_query_value(None,"foreign_keys",|r|r.get::<_,i64>(0)).map_err(sql_error)?!=0 { return Err(Error::new("storage_unavailable")); }
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false).map_err(sql_error)?;
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false).map_err(sql_error)?;
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW, false).map_err(sql_error)?;
    c.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true).map_err(sql_error)?;
    c.pragma_update(None,"ignore_check_constraints",true).map_err(sql_error)?;
    super::unicase::register(&c)?;
    if writable {
        c.pragma_update(None, "journal_mode", "WAL").map_err(sql_error)?;
        c.pragma_update(None, "synchronous", "FULL").map_err(sql_error)?;
        let mode: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0)).map_err(sql_error)?;
        let sync: i64 = c.pragma_query_value(None, "synchronous", |r| r.get(0)).map_err(sql_error)?;
        if mode != "wal" || sync != 2 { return Err(Error::new("storage_unavailable")); }
    }
    Ok(c)
}

/// The caller exclusively created this empty private file. No stock content is
/// reconstructed. The minimal empty schema is valid independently of clients'
/// UI defaults; current-client acceptance is established by the interop lane.
pub fn create_blank(path: &Path, seconds: i64, millis: i64) -> Result<()> {
    let mut c = open(path, true)?;
    c.pragma_update(None, "max_page_count", 128)?;
    let tx = c.transaction()?;
    tx.execute_batch(include_str!("schema.sql"))?;
    tx.execute("INSERT INTO col VALUES(1,?1,0,?2,18,0,0,0,'','','','','')", params![seconds,millis])?;
    tx.commit()?;
    c.execute_batch("ANALYZE;")?;
    checkpoint(&c)?;
    drop(c);
    File::open(path)?.sync_all()?;
    Ok(())
}

/// Native finish/download may not hide a committed C7 change when the clock
/// repeats a tick or moves backward. Full upload starts a different publication.
pub(crate) fn sync_stamp(c: &Connection, sampled: i64) -> Result<i64> {
    let committed: i64 = c.query_row("SELECT mod FROM col WHERE id=1", [], |r|r.get(0))?;
    Ok(sampled.max(committed))
}

pub fn checkpoint(c: &Connection) -> Result<()> {
    let (busy, _, _): (i64,i64,i64) = c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    if busy != 0 { return Err(Error::new("storage_unavailable")); }
    Ok(())
}

pub fn validate(c: &Connection) -> Result<()> {
    let mut q=c.prepare("PRAGMA table_list")?;
    let mut list=q.query([])?;
    while let Some(row)=list.next()? {
        if matches!(row.get::<_,String>(2)?.as_str(),"virtual"|"shadow") {return Err(invalid());}
    }
    let mut objects = c.prepare("SELECT type,name,sql FROM sqlite_schema")?;
    let mut rows = objects.query([])?;
    let mut tables = std::collections::BTreeSet::new();
    while let Some(row) = rows.next()? {
        let kind: String = row.get(0)?;
        let name: String = row.get(1)?;
        let sql: Option<String> = row.get(2)?;
        if kind == "table" {
            // Virtual modules are executable schema, not ordinary preserved data.
            let sql = sql.unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_uppercase();
            if sql.starts_with("CREATE VIRTUAL") { return Err(invalid()); }
            tables.insert(name);
        }
    }
    if TABLES.iter().any(|name| !tables.contains(*name)) { return Err(invalid()); }
    // Generated columns and expression/foreign partial indexes can run stored
    // expressions during integrity checking or stamp mutation. Retain ordinary
    // foreign objects, but refuse these executable forms before table scans.
    for table in &tables {
        let quoted=table.replace('"',"\"\"");
        let mut q=c.prepare(&format!("PRAGMA table_xinfo(\"{quoted}\")"))?;
        let mut columns=q.query([])?;
        while let Some(row)=columns.next()? {
            if row.get::<_,i64>(6)?!=0 { return Err(invalid()); }
        }
        let mut q=c.prepare(&format!("PRAGMA index_list(\"{quoted}\")"))?;
        let mut indexes=q.query([])?;
        while let Some(row)=indexes.next()? {
            let name:String=row.get(1)?;
            if row.get::<_,bool>(4)? {
                let sql:String=c.query_row("SELECT sql FROM sqlite_schema WHERE type='index' AND name=?1",[&name],|r|r.get(0))?;
                let normalized=sql.chars().filter(|c|!c.is_ascii_whitespace()).collect::<String>().to_ascii_uppercase();
                if name!="idx_cards_odid" || normalized!="CREATEINDEXIDX_CARDS_ODIDONCARDS(ODID)WHEREODID!=0" { return Err(invalid()); }
            }
            let escaped=name.replace('"',"\"\"");
            let mut q=c.prepare(&format!("PRAGMA index_xinfo(\"{escaped}\")"))?;
            let mut terms=q.query([])?;
            while let Some(row)=terms.next()? {
                if row.get::<_,i64>(1)? == -2 { return Err(invalid()); }
            }
        }
    }
    let mut q = c.prepare("PRAGMA integrity_check")?;
    let mut rows = q.query([])?;
    if rows.next()?.ok_or_else(invalid)?.get::<_,String>(0)? != "ok" || rows.next()?.is_some() { return Err(invalid()); }
    let count: i64 = c.query_row("SELECT count(*) FROM col", [], |r| r.get(0))?;
    let (id,ver): (i64,i64) = c.query_row("SELECT id,ver FROM col", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
    if count != 1 || id != 1 || ver != 18 { return Err(invalid()); }
    super::model_adapter::validate(c)?;
    Ok(())
}

/// SQLite backup includes committed WAL pages without regenerating records or
/// running VACUUM's schema programs. Destination was exclusively created by us.
pub fn snapshot(source: &Connection, destination: &Path) -> Result<()> {
    let mut target = open(destination, true)?;
    let backup = rusqlite::backup::Backup::new(source, &mut target)?;
    backup.run_to_completion(128, Duration::from_millis(5), None)?;
    drop(backup);
    checkpoint(&target)?;
    target.pragma_update(None, "journal_mode", "DELETE")?;
    drop(target);
    File::open(destination)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collection_stamp_preserves_foreign_key_rows_and_disables_actions_on_every_open() {
        use std::os::unix::fs::OpenOptionsExt;
        let temp=tempfile::tempdir().unwrap();let path=temp.path().join("owned.anki2");
        std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&path).unwrap();
        create_blank(&path,123,456).unwrap();
        let raw=Connection::open(&path).unwrap();
        raw.execute_batch("CREATE UNIQUE INDEX owned_stamp ON col(mod);
            CREATE TABLE owned_child(stamp INTEGER REFERENCES col(mod) ON UPDATE CASCADE);
            INSERT INTO owned_child VALUES(0);").unwrap();
        drop(raw);
        let c=open(&path,true).unwrap();validate(&c).unwrap();
        c.execute("UPDATE col SET mod=789,usn=usn+1 WHERE id=1",[]).unwrap();
        assert_eq!(c.query_row("SELECT stamp FROM owned_child",[],|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(c.pragma_query_value(None,"foreign_keys",|r|r.get::<_,i64>(0)).unwrap(),0);
        checkpoint(&c).unwrap();drop(c);
        let c=open(&path,false).unwrap();validate(&c).unwrap();
        assert_eq!(c.pragma_query_value(None,"foreign_keys",|r|r.get::<_,i64>(0)).unwrap(),0);
        assert_eq!(c.query_row("SELECT stamp FROM owned_child",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
}
