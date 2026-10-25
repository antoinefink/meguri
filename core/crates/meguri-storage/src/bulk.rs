//! Bulk writes (rust-stack §1): rows inserted with batched multi-row
//! statements, and the usn of many rows set through a TEMP table with
//! `UPDATE … FROM`, a page of keys at a time. Both write rows as given, so
//! they run only in `CallerSupplied` and `Raw` scopes; both journal what
//! they change when the scope keeps a journal, and read no row image when it
//! keeps none. TEMP tables live in the connection's temp schema, never in
//! the collection file's `sqlite_master` (`storage/schema-18` S6.2).

use meguri_model::usn::Usn;
use rusqlite::params_from_iter;

use crate::error::StorageError;
use crate::rows::{self, Cell, RowImage, RowKey, Table};
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
    /// scope's caller rolls back); nothing is compared. A journal gets the
    /// rows as stored.
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
            self.note_change();
            if self.journal().is_some() {
                for image in batch {
                    let after = rows::stored(self.conn(), table, rows::key_cells(table, image))?;
                    self.journal_change(table, RowKey::of(table, &after), None, Some(after))?;
                }
            }
        }
        Ok(images.len())
    }

    /// Sets `usn` on the rows with these keys whose `usn` differs (marking
    /// sent rows as synced, `ids-usn-mtime` S3): modification times stay,
    /// rows already holding `usn` are no change, missing keys are skipped.
    /// Returns how many rows changed. Works a page of keys at a time: with a
    /// journal, a page's before-images are journalled (and spill) before the
    /// next page is read; with none, no row image is read.
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
        // A key listed twice in a page joins its row twice: the journal keeps
        // one entry for it, and the update changes it once.
        let select = format!(
            "SELECT {selected} FROM {} AS t, temp.meguri_bulk_keys AS k WHERE {join} AND t.\"{usn_name}\" IS NOT ?1",
            table.name()
        );
        let update = format!(
            "UPDATE {name} AS t SET \"{usn_name}\" = ?1 FROM temp.meguri_bulk_keys AS k WHERE {join} AND t.\"{usn_name}\" IS NOT ?1",
            name = table.name()
        );
        let usn_value = i64::from(usn.0);
        let journalled = self.journal().is_some();
        let mut changed = 0;
        for page in keys.chunks(batch_rows(2)) {
            let sql = format!(
                "INSERT INTO temp.meguri_bulk_keys (a, b) VALUES {}",
                vec!["(?, ?)"; page.len()].join(", ")
            );
            let params = page.iter().flat_map(|key| {
                let mut cells = key.cells();
                cells.resize(2, Cell::Null);
                cells
            });
            conn.prepare_cached(&sql)?.execute(params_from_iter(params))?;
            if journalled {
                let mut statement = conn.prepare_cached(&select)?;
                let mut found = statement.query([usn_value])?;
                while let Some(row) = found.next()? {
                    let before = rows::row_image(row, table)?;
                    let mut after = before.clone();
                    after[usn_column] = Cell::Integer(usn_value);
                    self.journal_change(table, RowKey::of(table, &before), Some(before), Some(after))?;
                }
            }
            changed += conn.prepare_cached(&update)?.execute([usn_value])?;
            conn.execute_batch("DELETE FROM temp.meguri_bulk_keys")?;
        }
        conn.execute_batch("DROP TABLE temp.meguri_bulk_keys")?;
        if changed > 0 {
            self.note_change();
        }
        Ok(changed)
    }
}

impl WriteScope<'_> {
    /// One bounded set-based merge. Journal images are read back from SQLite,
    /// including affinity conversion; equal rows produce no journal entry.
    pub fn merge_sync_rows(&mut self, table: Table, images: &[RowImage]) -> Result<usize, StorageError> {
        require_as_given(self, "merge_sync_rows")?;
        if images.is_empty() {
            return Ok(0);
        }
        if images.len() > crate::normal_stage::PAGE || images.iter().any(|r| r.len() != table.columns().len()) {
            return Err(StorageError::Mode {
                detail: "sync merge page width or size".into(),
            });
        }
        let conn = self.conn();
        let cols = quoted_columns(table);
        conn.execute_batch(&format!("DROP TABLE IF EXISTS temp.meguri_sync_page; CREATE TEMP TABLE meguri_sync_page AS SELECT {cols} FROM {} WHERE 0;", table.name()))?;
        let sql = format!(
            "INSERT INTO temp.meguri_sync_page ({cols}) VALUES {}",
            vec![format!("({})", vec!["?"; table.columns().len()].join(",")); images.len()].join(",")
        );
        conn.prepare_cached(&sql)?
            .execute(params_from_iter(images.iter().flatten()))?;
        // The temporary table has the target's affinities. Compare after the
        // conversion, then preserve only the page's actual changed rows.
        let mut changed = Vec::new();
        let mut stmt = conn.prepare_cached(&format!("SELECT {cols} FROM temp.meguri_sync_page"))?;
        let mut found = stmt.query([])?;
        while let Some(row) = found.next()? {
            let image = rows::row_image(row, table)?;
            let prior = rows::select_compare(conn, table, &image)?;
            if !matches!(&prior, Some((_, true))) {
                changed.push((RowKey::of(table, &image), prior.map(|p| p.0)));
            }
        }
        drop(found);
        drop(stmt);
        let join = table.columns()[..table.key_len()]
            .iter()
            .map(|c| format!("t.\"{c}\" = p.\"{c}\""))
            .collect::<Vec<_>>()
            .join(" AND ");
        let equal = table
            .columns()
            .iter()
            .map(|c| format!("t.\"{c}\" IS p.\"{c}\" COLLATE BINARY"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let set = table.columns()[table.key_len()..]
            .iter()
            .map(|c| format!("\"{c}\" = p.\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        conn.execute_batch(&format!("UPDATE {name} AS t SET {set} FROM temp.meguri_sync_page AS p WHERE {join} AND NOT ({equal}); INSERT INTO {name} ({cols}) SELECT {pcols} FROM temp.meguri_sync_page AS p WHERE NOT EXISTS (SELECT 1 FROM {name} AS t WHERE {join});", name=table.name(),pcols=table.columns().iter().map(|c|format!("p.\"{c}\"")).collect::<Vec<_>>().join(",")))?;
        for (key, before) in &mut changed {
            let after = rows::select_image(conn, table, key)?;
            self.journal_change(table, key.clone(), before.take(), after)?;
        }
        conn.execute_batch("DROP TABLE temp.meguri_sync_page")?;
        Ok(changed.len())
    }
}
