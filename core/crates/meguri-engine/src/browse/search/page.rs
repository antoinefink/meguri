//! Row pages through a handle (`browser/result-sets` H7): at most 1000
//! indexes a call, read from the collection as it is at the call in one
//! short transaction on the query reader. A page defers the Question and
//! Answer cells; `qa_cells` gives them, cancellable between rows.

use std::ops::Range;
use std::sync::Arc;

use meguri_search::browser::cells::{Cell, RowTime};
use meguri_search::browser::qa::{Answer, Question};
use meguri_search::SearchMode;
use meguri_search::config::{Column, ColumnKey};
use meguri_storage::ReaderRole;

use crate::browse::{RowFailure, RowRequest, RowResult, rows_in};
use crate::cancel::CancelToken;
use crate::error::EngineError;

use super::{BrowseError, HandleId, MAX_ROWS, SessionShared, check_revision, lock, worker};

/// Which cells `qa_cells` computes (H7, `browser/columns` C10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QaWhich {
    Question,
    Answer,
    Both,
}

/// One row's Question and Answer cells; each present when asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct QaCells {
    pub question: Option<Question>,
    pub answer: Option<Answer>,
}

/// H7: the column keys of a page: the 19 columns and field columns
/// `fld:<name>` (`browser/workspace` V3).
fn columns(keys: &[&str]) -> Result<Vec<ColumnKey>, BrowseError> {
    keys.iter()
        .map(|key| ColumnKey::parse(key).ok_or_else(|| BrowseError::invalid(format!("{key:?} is not a column key"))))
        .collect()
}

/// The ids at `range` in the handle's revision `revision`, with its mode.
fn ids_at(
    shared: &SessionShared,
    handle: HandleId,
    revision: u64,
    range: &Range<u64>,
) -> Result<(SearchMode, Vec<i64>, crate::Collection), BrowseError> {
    if range.start > range.end || range.end - range.start > MAX_ROWS {
        return Err(BrowseError::invalid(format!(
            "a page takes at most {MAX_ROWS} indexes, not {range:?}"
        )));
    }
    let collection = shared.engine()?.collection()?;
    let handle = shared.handle(handle)?;
    let handle = lock(&handle);
    if handle.generation != collection.generation() {
        return Err(BrowseError::UnknownHandle);
    }
    check_revision(&handle, revision)?;
    if range.end > handle.count() {
        return Err(BrowseError::OutOfRange);
    }
    let ids = handle.ids[range.start as usize..range.end as usize].to_vec();
    Ok((handle.mode, ids, collection))
}

/// Reads `columns` of `ids` in one query snapshot at the call's now and
/// today, with `between` before each row.
fn read(
    collection: &crate::Collection,
    mode: SearchMode,
    ids: &[i64],
    columns: &[ColumnKey],
    between: &mut dyn FnMut() -> Result<(), EngineError>,
) -> Result<Vec<RowResult>, BrowseError> {
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    let context = worker::context(collection, &*snapshot)?;
    let request = RowRequest {
        mode,
        ids,
        columns,
        time: RowTime {
            now: context.now,
            today: context.today,
        },
    };
    Ok(rows_in(collection, &*snapshot, &request, between)?)
}

pub(super) fn rows(
    shared: &Arc<SessionShared>,
    handle: HandleId,
    revision: u64,
    range: Range<u64>,
    keys: &[&str],
) -> Result<Vec<RowResult>, BrowseError> {
    let requested = columns(keys)?;
    let (mode, ids, collection) = ids_at(shared, handle, revision, &range)?;
    let deferred = |column: &ColumnKey| matches!(column, ColumnKey::Column(Column::Question | Column::Answer));
    let computed: Vec<ColumnKey> = requested.iter().filter(|column| !deferred(column)).cloned().collect();
    let rows = read(&collection, mode, &ids, &computed, &mut || Ok(()))?;
    Ok(rows
        .into_iter()
        .map(|row| {
            row.map(|mut row| {
                let mut cells = std::mem::take(&mut row.cells).into_iter();
                row.cells = requested
                    .iter()
                    .map(|column| {
                        if deferred(column) {
                            Cell::Deferred
                        } else {
                            cells.next().unwrap_or(Cell::Empty)
                        }
                    })
                    .collect();
                row
            })
        })
        .collect())
}

pub(super) fn qa_cells(
    shared: &Arc<SessionShared>,
    handle: HandleId,
    revision: u64,
    range: Range<u64>,
    which: QaWhich,
    cancel: Option<&CancelToken>,
) -> Result<Vec<Result<QaCells, RowFailure>>, BrowseError> {
    let (mode, ids, collection) = ids_at(shared, handle, revision, &range)?;
    let columns: Vec<ColumnKey> = match which {
        QaWhich::Question => vec![ColumnKey::Column(Column::Question)],
        QaWhich::Answer => vec![ColumnKey::Column(Column::Answer)],
        QaWhich::Both => vec![ColumnKey::Column(Column::Question), ColumnKey::Column(Column::Answer)],
    };
    let rows = read(&collection, mode, &ids, &columns, &mut || match cancel {
        Some(token) => token.check(),
        None => Ok(()),
    })?;
    Ok(rows
        .into_iter()
        .map(|row| {
            row.map(|row| {
                let mut cells = QaCells {
                    question: None,
                    answer: None,
                };
                for cell in row.cells {
                    match cell {
                        Cell::Question(question) => cells.question = Some(question),
                        Cell::Answer(answer) => cells.answer = Some(answer),
                        _ => {}
                    }
                }
                cells
            })
        })
        .collect())
}
