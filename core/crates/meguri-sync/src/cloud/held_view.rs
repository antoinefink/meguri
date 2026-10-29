//! P7 eligible rows over one pinned snapshot. Journals remain lazy: key
//! enumeration never reads spilled images, and row() selects the oldest run.
use super::{
    Error,
    row_payload::{ReadView, View},
};
use meguri_engine::{Seq, undo::holdback::HeldBackRun};
use meguri_model::ids::CardId;
use meguri_storage::{ReadRows, RowImage, RowKey, Table};
use std::{cell::Cell, cmp::Ordering};

pub(super) struct HeldView<'a, T: ReadRows + ?Sized> {
    rows: &'a T,
    runs: &'a [HeldBackRun],
    image_seq: Cell<u64>,
    held: Cell<bool>,
}
impl<'a, T: ReadRows + ?Sized> HeldView<'a, T> {
    pub fn new(rows: &'a T, runs: &'a [HeldBackRun], seq: Seq) -> Self {
        Self {
            rows,
            runs,
            image_seq: Cell::new(seq.0),
            held: Cell::new(false),
        }
    }
    pub fn image(&self) -> (u64, bool) {
        (self.image_seq.get(), self.held.get())
    }
    fn held_run(&self, table: Table, key: &RowKey) -> Option<(&HeldBackRun, Seq)> {
        self.runs
            .iter()
            .filter_map(|run| run.image_seq(table, key).map(|seq| (run, seq)))
            .min_by_key(|(_, seq)| *seq)
    }
    fn note_hold(&self, seq: Seq) {
        self.held.set(true);
        self.image_seq.set(self.image_seq.get().min(seq.0));
    }
    fn keys(&self, table: Table) -> Vec<RowKey> {
        let mut keys: Vec<_> = self
            .runs
            .iter()
            .flat_map(|run| run.keys())
            .filter(|(t, _)| *t == table)
            .map(|(_, key)| key.clone())
            .collect();
        keys.sort_by(|a, b| compare(table, a, b));
        keys.dedup_by(|a, b| compare(table, a, b).is_eq());
        keys
    }
    fn merged_page(&self, table: Table, after: Option<&RowKey>, parent: Option<i64>) -> Result<Vec<RowImage>, Error> {
        let mut cursor = after.cloned();
        loop {
            let base = if let Some(id) = parent {
                let ord = match cursor {
                    Some(RowKey::Pair(_, ord)) => Some(ord),
                    _ => None,
                };
                ReadView(self.rows).parts(table, id, ord)?
            } else {
                ReadView(self.rows).page(table, cursor.as_ref())?
            };
            let end = base
                .last()
                .map(|row| RowKey::from_stored(table, row).map_err(|_| Error::InvalidData))
                .transpose()?;
            let mut keys = self.keys(table);
            keys.extend(
                base.iter()
                    .map(|row| RowKey::from_stored(table, row).map_err(|_| Error::InvalidData))
                    .collect::<Result<Vec<_>, _>>()?,
            );
            keys.retain(|key| {
                cursor.as_ref().is_none_or(|a| compare(table, key, a).is_gt())
                    && end.as_ref().is_none_or(|e| !compare(table, key, e).is_gt())
                    && parent.is_none_or(|id| matches!(key, RowKey::Pair(p, _) if *p == id))
            });
            keys.sort_by(|a, b| compare(table, a, b));
            keys.dedup_by(|a, b| compare(table, a, b).is_eq());
            let mut out = Vec::new();
            for key in keys {
                if let Some(row) = self.row(table, &key)? {
                    out.push(row);
                }
                if out.len() == 250 {
                    break;
                }
            }
            if !out.is_empty() || end.is_none() {
                return Ok(out);
            }
            cursor = end;
        }
    }
}
pub(super) fn compare(table: Table, a: &RowKey, b: &RowKey) -> Ordering {
    if table == Table::Tags
        && let (RowKey::Text(a), RowKey::Text(b)) = (a, b)
        && let (Ok(a), Ok(b)) = (std::str::from_utf8(a), std::str::from_utf8(b))
    {
        return meguri_text::fold::compare_folded(a, b);
    }
    a.cmp(b)
}
impl<T: ReadRows + ?Sized> View for HeldView<'_, T> {
    fn row(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, Error> {
        if let Some((run, seq)) = self.held_run(table, key) {
            self.note_hold(seq);
            return Ok(run
                .before(table, key)
                .map_err(|_| Error::InvalidData)?
                .ok_or(Error::InvalidData)?
                .before);
        }
        Ok(self.rows.row_image(table, key)?)
    }
    fn page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, Error> {
        self.merged_page(table, after, None)
    }
    fn parts(&self, table: Table, id: i64, after: Option<i64>) -> Result<Vec<RowImage>, Error> {
        self.merged_page(table, after.map(|ord| RowKey::Pair(id, ord)).as_ref(), Some(id))
    }
    fn history(&self, id: i64) -> Result<i64, Error> {
        let mut maximum = self.rows.max_revlog_id(CardId(id))?;
        while maximum != 0 {
            let key = RowKey::Id(maximum);
            if let Some((run, seq)) = self.held_run(Table::Revlog, &key) {
                self.note_hold(seq);
                let before = run
                    .before(Table::Revlog, &key)
                    .map_err(|_| Error::InvalidData)?
                    .ok_or(Error::InvalidData)?
                    .before;
                if before
                    .as_ref()
                    .is_some_and(|row| super::row_payload::int(row, 1) == Ok(id))
                {
                    break;
                }
                maximum = self.rows.max_revlog_id_before(CardId(id), maximum)?;
            } else {
                break;
            }
        }
        // A held deletion can restore the maximum, including a row no longer
        // present in the snapshot. Inspect only the requested history's images.
        for key in self.keys(Table::Revlog) {
            let (run, seq) = self.held_run(Table::Revlog, &key).ok_or(Error::InvalidData)?;
            let before = run
                .before(Table::Revlog, &key)
                .map_err(|_| Error::InvalidData)?
                .ok_or(Error::InvalidData)?
                .before;
            let live = self.rows.row_image(Table::Revlog, &key)?;
            if before
                .as_ref()
                .or(live.as_ref())
                .is_some_and(|row| super::row_payload::int(row, 1) == Ok(id))
            {
                self.note_hold(seq);
                if let Some(row) = before {
                    maximum = maximum.max(super::row_payload::int(&row, 0)?);
                }
            }
        }
        Ok(maximum)
    }
}
