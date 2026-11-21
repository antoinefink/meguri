//! Browse's column sets (`specs/browser/workspace.md` V2.2 to V2.4, V4.3):
//! reading `meguriBrowserColumnSets` and each user change as an op that
//! writes the key's whole value through `storage/config-table` C4 (nothing
//! when the stored value is the same value). meguri-search's
//! `config::column_sets` computes the value; this module only applies it.
//!
//! These ops never read or write Anki's column and sort keys (`activeCols`,
//! `activeNoteCols`, `sortType`, `sortBackwards`, `noteSortType`,
//! `browserNoteSortBackwards`): other clients keep their own (V2.4).
//!
//! Undo: the current-deck selection's skip-undo class (V2.4, `engine/undo`
//! M2): a change records no step, keeps the undo steps and empties redo, so
//! a column change never takes away "Undo Suspend Cards".

use meguri_model::changes::OpSource;
use meguri_model::ids::NotetypeId;
use meguri_search::SearchMode;
use meguri_search::config::column_sets::{self, Change, ColumnSet, ColumnSets, SetId};
use meguri_storage::{ConfigValue, ReadRows, Table, WriteScope};

use super::refused;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpResult, OpSpec};

/// The key as `rows` holds it, read per C3; reading writes nothing.
pub fn read(rows: &(impl ReadRows + ?Sized)) -> Result<ColumnSets, EngineError> {
    Ok(ColumnSets::from_stored(rows.config_value(column_sets::KEY)?.as_ref()))
}

/// The set a list uses (V2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The set changes write to: the note type's when the query names one
    /// that exists, even before it has an own set.
    pub set: SetId,
    /// Whether that note type has an own set; false for the standard set.
    pub own: bool,
    pub columns: ColumnSet,
}

/// V2.2: the set a list shows when the query's Note Type token names
/// `notetype` (one value, set to is), or the standard set for `None`. A set
/// whose note type no longer exists is ignored (V2.4): the list uses the
/// standard set and the stored set is left as it is.
pub fn shown(rows: &(impl ReadRows + ?Sized), notetype: Option<NotetypeId>) -> Result<Shown, EngineError> {
    let sets = read(rows)?;
    let existing = match notetype {
        Some(id) if rows.notetype(id)?.is_some() => Some(id),
        _ => None,
    };
    Ok(match existing {
        Some(id) => Shown {
            set: SetId::Notetype(id),
            own: sets.has_own(id),
            columns: sets.set(SetId::Notetype(id)),
        },
        None => Shown {
            set: SetId::Standard,
            own: false,
            columns: sets.standard(),
        },
    })
}

/// `set` must be the standard set or an existing note type's.
fn require_set(scope: &WriteScope<'_>, set: SetId) -> Result<(), EngineError> {
    match set {
        SetId::Notetype(id) if scope.notetype(id)?.is_none() => Err(EngineError::NotFound {
            table: Table::Notetypes,
            key: id.0.to_string(),
        }),
        _ => Ok(()),
    }
}

/// `change` to `set` in `mode` within `scope`: refused `last_column`,
/// `invalid_order` or `unsortable_column` (K4.2, K4.3, K5.2's kinds), and
/// `NotFound` for the set of a note type that does not exist. Returns
/// whether the key was written.
pub fn change_in(
    scope: &mut WriteScope<'_>,
    set: SetId,
    mode: SearchMode,
    change: &Change,
) -> Result<bool, EngineError> {
    require_set(scope, set)?;
    let next = read(scope)?.change(set, mode, change).map_err(refused)?;
    write(scope, next)
}

/// Use Standard Columns (V2.2) within `scope`: the note type's own set
/// becomes a copy of the standard set; nothing when it has none.
pub fn use_standard_in(scope: &mut WriteScope<'_>, notetype: NotetypeId) -> Result<bool, EngineError> {
    require_set(scope, SetId::Notetype(notetype))?;
    let next = read(scope)?.use_standard(notetype);
    write(scope, next)
}

/// The key's whole value through C4, when there is one. Returns whether it
/// was written.
fn write(scope: &mut WriteScope<'_>, value: Option<ConfigValue>) -> Result<bool, EngineError> {
    match value {
        Some(value) => Ok(scope.set_config(column_sets::KEY, &value)?.changed()),
        None => Ok(false),
    }
}

/// The op of a column set write: no undo step, undo steps kept (V2.4).
fn skip_undo(name: &'static str) -> OpSpec {
    OpSpec::named(name, OpSource::UserNonUndoable).skip_undo()
}

/// A user change to a set's columns or sort, as one op. A sort by a column
/// that does not sort in `mode` is refused before any op runs.
pub fn change(
    collection: &Collection,
    set: SetId,
    mode: SearchMode,
    change: Change,
) -> Result<OpResult<bool>, EngineError> {
    change.check(mode).map_err(refused)?;
    collection.run(skip_undo("change_browser_column_set"), move |ctx| {
        change_in(ctx.scope(), set, mode, &change)
    })
}

/// Use Standard Columns, as one op.
pub fn use_standard(collection: &Collection, notetype: NotetypeId) -> Result<OpResult<bool>, EngineError> {
    collection.run(skip_undo("use_standard_browser_columns"), move |ctx| {
        use_standard_in(ctx.scope(), notetype)
    })
}
