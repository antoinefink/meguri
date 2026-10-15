//! Bulk writes (rust-stack §1): rows inserted with batched multi-row
//! statements, and the usn of many rows set through a TEMP table with
//! `UPDATE … FROM`. Both write rows as given, so they run only in
//! `CallerSupplied` and `Raw` scopes; both journal what they change when the
//! scope keeps a journal. TEMP tables live in the connection's temp schema,
//! never in the collection file's `sqlite_master` (`storage/schema-18`
//! S6.2).

use meguri_model::usn::Usn;
use rusqlite::params_from_iter;

use crate::error::StorageError;
use crate::rows::{Cell, RowImage, RowKey, Table};
use crate::stamp::StampMode;
use crate::write_scope::WriteScope;

/// Rows per statement; fewer when the table is wide, so a statement never
/// binds more than SQLite's 32,766 parameters.
pub const BATCH_ROWS: usize = 1000;
const MAX_PARAMS: usize = 32_766;

fn batch_rows(width: usize) -> usize {
    BATCH_ROWS.min(MAX_PARAMS / width.max(1)).max(1)
}

fn require_as_given(scope: &WriteScope<'_>, what: &str) -> Result<(), StorageError> {
    match scope.mode() {
        StampMode::CallerSupplied | StampMode::Raw => Ok(()),
        mode => Err(StorageError::Mode {
            detail: format!("{what} writes rows as given; a {mode:?} scope stamps them"),
        }),
    }
}

fn quoted_columns(table: Table) -> String {
    table
        .columns()
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

impl WriteScope<'_> {
    /// Inserts new rows as given, in batches: a staging copy or a generated
    /// collection. A row whose key is taken fails the statement (and the
    /// scope's caller rolls back); nothing is compared.
    pub fn insert_rows(&mut self, table: Table, images: &[RowImage]) -> Result<usize, StorageError> {
        require_as_given(self, "insert_rows")?;
        let width = table.columns().len();
        if let Some(bad) = images.iter().find(|image| image.len() != width) {
            return Err(StorageError::Mode {
                detail: format!("a row of {} cells for {table}, which has {width} columns", bad.len()),
            });
        }
        let row = format!("({})", vec!["?"; width].join(", "));
        for batch in images.chunks(batch_rows(width)) {
            let sql = format!(
                "INSERT INTO {} ({}) VALUES {}",
                table.name(),
                quoted_columns(table),
                vec![row.as_str(); batch.len()].join(", ")
            );
            self.conn()
                .prepare_cached(&sql)?
                .execute(params_from_iter(batch.iter().flatten()))?;
            for image in batch {
                self.journal_change(table, RowKey::of(table, image), None, Some(image.clone()))?;
            }
        }
        Ok(images.len())
    }

    /// Sets `usn` on the rows with these keys whose `usn` differs (marking
    /// sent rows as synced, `ids-usn-mtime` S3): modification times stay,
    /// rows already holding `usn` are no change, missing keys are skipped.
    /// Returns how many rows changed.
    pub fn mark_usn(&mut self, table: Table, keys: &[RowKey], usn: Usn) -> Result<usize, StorageError> {
        require_as_given(self, "mark_usn")?;
        let Some(usn_column) = table.usn_column() else {
            return Err(StorageError::Mode {
                detail: format!("{table} rows carry no usn"),
            });
        };
        let conn = self.conn();
        conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS meguri_bulk_keys (a, b);
             DELETE FROM temp.meguri_bulk_keys;",
        )?;
        for batch in keys.chunks(batch_rows(2)) {
            let sql = format!(
                "INSERT INTO temp.meguri_bulk_keys (a, b) VALUES {}",
                vec!["(?, ?)"; batch.len()].join(", ")
            );
            let params = batch.iter().flat_map(|key| {
                let mut cells = key.cells();
                cells.resize(2, Cell::Null);
                cells
            });
            conn.prepare_cached(&sql)?.execute(params_from_iter(params))?;
        }
        let columns = table.columns();
        let join = columns[..table.key_len()]
            .iter()
            .zip(["a", "b"])
            .map(|(c, k)| format!("t.\"{c}\" = k.{k}"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let usn_name = columns[usn_column];
        let selected = columns
            .iter()
            .map(|c| format!("t.\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let before: Vec<RowImage> = {
            let sql = format!(
                "SELECT DISTINCT {selected} FROM {} AS t, temp.meguri_bulk_keys AS k WHERE {join} AND t.\"{usn_name}\" IS NOT ?1",
                table.name()
            );
            let mut statement = conn.prepare(&sql)?;
            let width = columns.len();
            let rows = statement.query_map([i64::from(usn.0)], |row| {
                (0..width).map(|i| row.get_ref(i).map(Cell::from_ref)).collect()
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let sql = format!(
            "UPDATE {name} AS t SET \"{usn_name}\" = ?1 FROM temp.meguri_bulk_keys AS k WHERE {join} AND t.\"{usn_name}\" IS NOT ?1",
            name = table.name()
        );
        conn.prepare(&sql)?.execute([i64::from(usn.0)])?;
        conn.execute_batch("DROP TABLE temp.meguri_bulk_keys")?;
        let changed = before.len();
        for image in before {
            let mut after = image.clone();
            after[usn_column] = Cell::Integer(i64::from(usn.0));
            self.journal_change(table, RowKey::of(table, &image), Some(image), Some(after))?;
        }
        Ok(changed)
    }
}
