//! Disk-backed normal-sync view, incoming rows and exact sent images (H4-H10).
//! SQL names come only from Table and Namespace; data always uses parameters.
use crate::rows;
use crate::{Cell, ReadRows, Record, RowImage, RowKey, StampMode, StorageError, Table, WriteScope};
use meguri_model::{clock::Clock, ids::NotetypeId, notetype::Notetype};
use rusqlite::{Connection, params_from_iter};
use std::path::{Path, PathBuf};

pub const PAGE: usize = 250;
#[derive(Clone, Copy, Debug)]
pub enum Namespace {
    View,
    Incoming,
    Sent,
}
impl Namespace {
    fn prefix(self) -> &'static str {
        match self {
            Self::View => "",
            Self::Incoming => "incoming_",
            Self::Sent => "sent_",
        }
    }
}
fn columns(table: Table) -> String {
    table
        .columns()
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",")
}
fn keys(table: Table) -> String {
    table.columns()[..table.key_len()]
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(",")
}
fn condition(table: Table) -> String {
    table.columns()[..table.key_len()]
        .iter()
        .enumerate()
        .map(|(i, c)| format!("\"{c}\" = ?{}", i + 1))
        .collect::<Vec<_>>()
        .join(" AND ")
}
fn bad(detail: &str) -> StorageError {
    StorageError::Mode { detail: detail.into() }
}

/// Keyset page, including composite and text keys, with no whole-table vector.
pub(crate) fn page(
    conn: &Connection,
    prefix: &str,
    table: Table,
    after: Option<&RowKey>,
    pending: bool,
) -> Result<Vec<RowImage>, StorageError> {
    let mut predicates = Vec::new();
    let mut params = Vec::new();
    if let Some(key) = after {
        params = key.cells();
        let binds = (1..=params.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(",");
        predicates.push(format!("({}) > ({binds})", keys(table)));
    }
    if pending {
        let usn = table.usn_column().ok_or_else(|| bad("table has no pending rows"))?;
        predicates.push(format!("\"{}\" = -1", table.columns()[usn]));
    }
    let filter = if predicates.is_empty() {
        "1".into()
    } else {
        predicates.join(" AND ")
    };
    let sql = format!(
        "SELECT {} FROM {prefix}{} WHERE {filter} ORDER BY {} LIMIT {PAGE}",
        columns(table),
        table.name(),
        keys(table)
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    Ok(stmt
        .query_map(params_from_iter(params), |r| rows::row_image(r, table))?
        .collect::<rusqlite::Result<_>>()?)
}
fn get(conn: &Connection, prefix: &str, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .prepare_cached(&format!(
            "SELECT {} FROM {prefix}{} WHERE {}",
            columns(table),
            table.name(),
            condition(table)
        ))?
        .query_row(params_from_iter(key.cells()), |r| rows::row_image(r, table))
        .optional()?)
}
fn save(conn: &Connection, prefix: &str, table: Table, image: &[Cell]) -> Result<(), StorageError> {
    if image.len() != table.columns().len() {
        return Err(bad("stage row width"));
    }
    let binds = vec!["?"; image.len()].join(",");
    conn.prepare_cached(&format!(
        "INSERT OR REPLACE INTO {prefix}{} ({}) VALUES ({binds})",
        table.name(),
        columns(table)
    ))?
    .execute(params_from_iter(image))?;
    Ok(())
}

pub struct NormalStage {
    conn: Connection,
    path: PathBuf,
}
impl crate::paging::sealed::Connected for NormalStage {
    fn connection(&self) -> &Connection {
        &self.conn
    }
}
impl ReadRows for NormalStage {}
impl NormalStage {
    /// Caller owns collection admission and has checked the temporary-space budget.
    pub fn create(collection: &Path) -> Result<Self, StorageError> {
        let path = collection.with_file_name("meguri-sync-stage.db");
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let file = crate::connections::sibling(&path, suffix);
            match std::fs::remove_file(file) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        let conn = Connection::open(&path)?;
        let stage = Self { conn, path };
        crate::conn_init::probe(&stage.conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
        stage.conn.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=OFF; PRAGMA temp_store=FILE; PRAGMA cache_size=-8192;",
        )?;
        stage.conn.execute_batch(include_str!("schema/ddl18.sql"))?;
        for prefix in ["incoming_", "sent_"] {
            for table in Table::ALL {
                let names = table
                    .columns()
                    .iter()
                    .map(|c| {
                        let collation = if table == Table::Tags && *c == "tag" {
                            " COLLATE unicase"
                        } else {
                            ""
                        };
                        format!("\"{c}\"{collation}")
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                stage.conn.execute_batch(&format!(
                    "CREATE TABLE {prefix}{} ({names}, PRIMARY KEY({})) WITHOUT ROWID;",
                    table.name(),
                    keys(table)
                ))?;
            }
        }
        Ok(stage)
    }
    pub fn copy_page(&self, table: Table, images: &[RowImage]) -> Result<(), StorageError> {
        if images.len() > PAGE {
            return Err(bad("snapshot page size"));
        }
        self.conn.execute_batch("BEGIN")?;
        let result = images.iter().try_for_each(|r| save(&self.conn, "", table, r));
        if result.is_ok() {
            self.conn.execute_batch("COMMIT")?;
        } else {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
        result
    }
    pub fn pending_graves(
        &self,
        kind: i64,
        after: Option<&RowKey>,
        limit: usize,
    ) -> Result<Vec<RowImage>, StorageError> {
        let after = match after {
            Some(RowKey::Pair(id, _)) => Some(*id),
            _ => None,
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT oid,type,usn FROM graves WHERE usn=-1 AND type=?1 AND (?2 IS NULL OR oid>?2) ORDER BY oid LIMIT ?3",
        )?;
        Ok(stmt
            .query_map((kind, after, limit.min(PAGE) as i64), |r| {
                rows::row_image(r, Table::Graves)
            })?
            .collect::<rusqlite::Result<_>>()?)
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn row_key(table: Table, image: &[Cell]) -> RowKey {
        RowKey::of(table, image)
    }
    pub fn page(
        &self,
        ns: Namespace,
        table: Table,
        after: Option<&RowKey>,
        pending: bool,
    ) -> Result<Vec<RowImage>, StorageError> {
        page(&self.conn, ns.prefix(), table, after, pending)
    }
    pub fn get(&self, ns: Namespace, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
        get(&self.conn, ns.prefix(), table, key)
    }
    pub fn store(&self, ns: Namespace, table: Table, image: &[Cell]) -> Result<(), StorageError> {
        save(&self.conn, ns.prefix(), table, image)
    }
    pub fn store_record<R: Record>(&self, ns: Namespace, row: &R) -> Result<(), StorageError> {
        self.store(ns, R::TABLE, &row.to_cells())
    }
    pub fn store_notetype(&self, ns: Namespace, nt: &Notetype) -> Result<(), StorageError> {
        self.store(ns, Table::Notetypes, &rows::notetypes::head_cells(nt))?;
        for table in [Table::Fields, Table::Templates] {
            self.conn
                .prepare_cached(&format!("DELETE FROM {}{} WHERE ntid=?1", ns.prefix(), table.name()))?
                .execute([nt.id.0])?;
        }
        for field in &nt.fields {
            self.store(ns, Table::Fields, &rows::notetypes::field_cells(nt.id, field))?;
        }
        for template in &nt.templates {
            self.store(ns, Table::Templates, &rows::notetypes::template_cells(nt.id, template))?;
        }
        Ok(())
    }
    pub fn incoming_notetype(&self, id: NotetypeId) -> Result<Option<Notetype>, StorageError> {
        rows::notetypes::get_prefixed(&self.conn, "incoming_", id)
    }
    pub fn write<T, E: From<StorageError>>(
        &self,
        clock: &dyn Clock,
        work: impl FnOnce(&mut WriteScope<'_>) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut scope = WriteScope::begin(&self.conn, StampMode::CallerSupplied, clock, None)?;
        let result = work(&mut scope)?;
        scope.commit()?;
        Ok(result)
    }
    pub fn count(&self, table: Table) -> Result<u32, StorageError> {
        Ok(self
            .conn
            .query_row(&format!("SELECT count(*) FROM {}", table.name()), [], |r| r.get(0))?)
    }
    pub fn incoming_count(&self) -> Result<u64, StorageError> {
        let mut count = 0u64;
        for table in Table::ALL {
            let table_count =
                self.conn
                    .query_row(&format!("SELECT count(*) FROM incoming_{}", table.name()), [], |r| {
                        r.get::<_, i64>(0)
                    })?;
            let table_count = u64::try_from(table_count).map_err(|_| bad("negative incoming row count"))?;
            count = count
                .checked_add(table_count)
                .ok_or_else(|| bad("incoming row count overflow"))?;
        }
        Ok(count)
    }
    /// H9 compares M's remaining pending keys with the exact sent images.
    pub fn pending_unsent(&self) -> Result<bool, StorageError> {
        for table in Table::ALL.into_iter().filter(|t| t.usn_column().is_some()) {
            if table == Table::Config {
                continue;
            }
            let join = table.columns()[..table.key_len()]
                .iter()
                .map(|c| format!("v.\"{c}\"=s.\"{c}\""))
                .collect::<Vec<_>>()
                .join(" AND ");
            let sql = format!(
                "SELECT EXISTS(SELECT 1 FROM {} v WHERE v.\"{}\"=-1 AND NOT EXISTS(SELECT 1 FROM sent_{} s WHERE {join}))",
                table.name(),
                table.columns()[table.usn_column().unwrap()],
                table.name()
            );
            if self.conn.query_row(&sql, [], |r| r.get::<_, bool>(0))? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
impl Drop for NormalStage {
    fn drop(&mut self) {
        // DELETE-mode scratch transactions are closed before unlink; the job owns
        // no published data here. Launch sweep also covers interrupted processes.
        let _ = self.conn.execute_batch("ROLLBACK");
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let _ = std::fs::remove_file(crate::connections::sibling(&self.path, suffix));
        }
    }
}
impl WriteScope<'_> {
    /// Attached only within the sync op; scope completion/rollback detaches it.
    pub fn attach_normal_stage(&mut self, path: &Path) -> Result<(), StorageError> {
        if self.normal_stage {
            return Err(bad("sync stage already attached"));
        }
        self.conn().execute(
            "ATTACH DATABASE ?1 AS meguri_sync",
            [crate::connections::file_uri(path, Some("mode=ro"))],
        )?;
        self.normal_stage = true;
        Ok(())
    }
    pub fn staged_page(
        &self,
        ns: Namespace,
        table: Table,
        after: Option<&RowKey>,
    ) -> Result<Vec<RowImage>, StorageError> {
        page(
            self.conn(),
            &format!("meguri_sync.{}", ns.prefix()),
            table,
            after,
            false,
        )
    }
    pub fn staged_image(&self, ns: Namespace, table: Table, key: &RowKey) -> Result<Option<RowImage>, StorageError> {
        get(self.conn(), &format!("meguri_sync.{}", ns.prefix()), table, key)
    }
    pub fn staged_notetype(&self, id: NotetypeId) -> Result<Option<Notetype>, StorageError> {
        rows::notetypes::get_prefixed(self.conn(), "meguri_sync.incoming_", id)
    }
    pub fn sync_name_taken(&self, table: Table, id: i64, name: &str) -> Result<bool, StorageError> {
        if !matches!(table, Table::Decks | Table::Notetypes) {
            return Err(bad("name uniqueness table"));
        }
        Ok(self.conn().query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM {} WHERE id != ?1 AND name = ?2 COLLATE unicase)",
                table.name()
            ),
            (id, name),
            |r| r.get(0),
        )?)
    }
    pub fn sync_has_pending(&self) -> Result<bool, StorageError> {
        for table in Table::ALL
            .into_iter()
            .filter(|t| t.usn_column().is_some() && *t != Table::Config)
        {
            if self.conn().query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {} WHERE usn=-1)", table.name()),
                [],
                |r| r.get::<_, bool>(0),
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
