//! The browser and search config writes (`specs/browser/config-keys.md` K4
//! to K7, K10): each user action of Browse, its sidebar, the previewer and
//! Settings › Browse as an op on the executor, writing one key's whole
//! value through `storage/config-table` C4 (nothing when the stored value is
//! the same value). meguri-search's `config` reads the keys and computes
//! what each action writes; this module only applies it.
//!
//! Undo (K10.2): the saved-search writes are undoable steps named after
//! their action (R2020); every other write is `UserNonUndoable`, so one that
//! changes its key clears the undo and redo history and one that writes
//! nothing leaves it alone. `flagLabels` is `flags`' "Rename Flag".
//!
//! Each write has an `_in` form for a scope the caller holds (the scenario
//! adapters, and Set Due Date and Reset, which remember their input inside
//! their own op, K6.6) and an op form that runs it as one op.

use meguri_model::changes::OpSource;
use meguri_search::SearchMode;
use meguri_search::config::{
    ActiveColumns, Column, Key, Refusal, SavedSearches, Section, Sort, SortDirection, columns_key, sort_keys,
};
use meguri_storage::{ConfigValue, ReadRows, WriteScope};

use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpResult, OpSpec};
use crate::undo::entry::StepKind;
use crate::undo::names::{DELETE_SAVED_SEARCH, RENAME_SAVED_SEARCH, SAVE_SEARCH, UPDATE_SAVED_SEARCH};

/// A refusal of K4.2, K4.3, K5.2 or K7.2 as the engine reports it: an
/// invalid argument whose message starts with the refusal's kind.
pub fn refused(refusal: Refusal) -> EngineError {
    EngineError::InvalidArgument {
        message: format!("{}: nothing was written", refusal.kind()),
    }
}

/// K10.1: the whole-value write of `key` within `scope`. Refused for
/// `normalize_note_text` and for a value of another shape. Returns whether
/// the key was written.
pub fn set_value_in(scope: &mut WriteScope<'_>, key: Key, value: &ConfigValue) -> Result<bool, EngineError> {
    let shape_ok = key.shape().accepts(value);
    if !key.writable() || !shape_ok {
        return Err(EngineError::InvalidArgument {
            message: format!("{key} takes no such whole-value write: {}", value.compact()),
        });
    }
    write(scope, key, value)
}

/// K4.2 Show column within `scope`.
pub fn show_column_in(scope: &mut WriteScope<'_>, mode: SearchMode, column: Column) -> Result<bool, EngineError> {
    let next = columns(scope, mode)?.show(column);
    write_some(scope, columns_key(mode), next)
}

/// K4.2 Hide column within `scope`; refused `last_column`.
pub fn hide_column_in(scope: &mut WriteScope<'_>, mode: SearchMode, column: Column) -> Result<bool, EngineError> {
    let next = columns(scope, mode)?.hide(column).map_err(refused)?;
    write_some(scope, columns_key(mode), next)
}

/// K4.3 within `scope`: the displayed columns in `order`; refused
/// `invalid_order`.
pub fn reorder_columns_in(scope: &mut WriteScope<'_>, mode: SearchMode, order: &[Column]) -> Result<bool, EngineError> {
    let next = columns(scope, mode)?.reorder(order).map_err(refused)?;
    write(scope, columns_key(mode), &next)
}

/// K5.2 Sort by column (a header click) within `scope`; refused
/// `unsortable_column`.
pub fn sort_by_column_in(scope: &mut WriteScope<'_>, mode: SearchMode, column: Column) -> Result<bool, EngineError> {
    let next = current_sort(scope, mode)?.clicked(mode, column).map_err(refused)?;
    write_sort(scope, mode, next)
}

/// K5.2 Set sort within `scope`; refused `unsortable_column`.
pub fn set_sort_in(
    scope: &mut WriteScope<'_>,
    mode: SearchMode,
    column: Column,
    direction: SortDirection,
) -> Result<bool, EngineError> {
    let next = Sort::chosen(mode, column, direction).map_err(refused)?;
    write_sort(scope, mode, next)
}

/// K6.1 within `scope`: true shows notes.
pub fn set_notes_mode_in(scope: &mut WriteScope<'_>, notes: bool) -> Result<bool, EngineError> {
    write(scope, Key::BrowserTableShowNotesMode, &ConfigValue::Bool(notes))
}

/// K6.2 within `scope`.
pub fn set_section_collapsed_in(
    scope: &mut WriteScope<'_>,
    section: Section,
    collapsed: bool,
) -> Result<bool, EngineError> {
    write(scope, section.key(), &ConfigValue::Bool(collapsed))
}

/// K6.3 within `scope`.
pub fn set_preview_both_sides_in(scope: &mut WriteScope<'_>, on: bool) -> Result<bool, EngineError> {
    write(scope, Key::PreviewBothSides, &ConfigValue::Bool(on))
}

/// K6.4 within `scope`: the text exactly as typed.
pub fn set_default_search_text_in(scope: &mut WriteScope<'_>, text: &str) -> Result<bool, EngineError> {
    write(scope, Key::DefaultSearchText, &ConfigValue::from(text))
}

/// K6.5 within `scope`.
pub fn set_ignore_accents_in_search_in(scope: &mut WriteScope<'_>, on: bool) -> Result<bool, EngineError> {
    write(scope, Key::IgnoreAccentsInSearch, &ConfigValue::Bool(on))
}

/// K6.6: Set Due Date from Browse remembers its accepted input, inside its
/// own op.
pub fn remember_set_due_in(scope: &mut WriteScope<'_>, input: &str) -> Result<bool, EngineError> {
    write(scope, Key::SetDueBrowser, &ConfigValue::from(input))
}

/// K6.6: Reset from Browse remembers its two options, inside its own op.
/// Returns whether either key was written.
pub fn remember_reset_options_in(
    scope: &mut WriteScope<'_>,
    restore_position: bool,
    reset_counts: bool,
) -> Result<bool, EngineError> {
    let restore = write(scope, Key::RestorePositionBrowser, &ConfigValue::Bool(restore_position))?;
    let reset = write(scope, Key::ResetCountsBrowser, &ConfigValue::Bool(reset_counts))?;
    Ok(restore || reset)
}

/// K7.2 Save Search within `scope`; refused `empty_name`.
pub fn save_search_in(scope: &mut WriteScope<'_>, name: &str, search: &str) -> Result<bool, EngineError> {
    let next = saved(scope)?.save(name, search).map_err(refused)?;
    write_some(scope, Key::SavedFilters, next)
}

/// K7.2 Update Saved Search within `scope`.
pub fn update_saved_search_in(scope: &mut WriteScope<'_>, name: &str, search: &str) -> Result<bool, EngineError> {
    let next = saved(scope)?.update(name, search);
    write_some(scope, Key::SavedFilters, next)
}

/// K7.2 Rename Saved Search within `scope`.
pub fn rename_saved_search_in(scope: &mut WriteScope<'_>, old: &str, new_text: &str) -> Result<bool, EngineError> {
    let next = saved(scope)?.rename(old, new_text);
    write_some(scope, Key::SavedFilters, next)
}

/// K7.2 Delete Saved Searches within `scope`.
pub fn delete_saved_searches_in(scope: &mut WriteScope<'_>, names: &[&str]) -> Result<bool, EngineError> {
    let next = saved(scope)?.delete(names);
    write_some(scope, Key::SavedFilters, next)
}

/// One key's whole value through C4. Returns whether it was written.
fn write(scope: &mut WriteScope<'_>, key: Key, value: &ConfigValue) -> Result<bool, EngineError> {
    Ok(scope.set_config(key.name(), value)?.changed())
}

/// [`write`] when the action has something to write.
fn write_some(scope: &mut WriteScope<'_>, key: Key, value: Option<ConfigValue>) -> Result<bool, EngineError> {
    match value {
        Some(value) => write(scope, key, &value),
        None => Ok(false),
    }
}

/// K5.2: the mode's two sort keys, each through C4.
fn write_sort(scope: &mut WriteScope<'_>, mode: SearchMode, sort: Sort) -> Result<bool, EngineError> {
    let (column_key, backwards_key) = sort_keys(mode);
    let column = write(scope, column_key, &ConfigValue::from(sort.column.key()))?;
    let backwards = write(scope, backwards_key, &ConfigValue::Bool(sort.direction.descending()))?;
    Ok(column || backwards)
}

/// The mode's active columns as the op reads them now.
fn columns(scope: &WriteScope<'_>, mode: SearchMode) -> Result<ActiveColumns, EngineError> {
    let stored = scope.config_value(columns_key(mode).name())?;
    Ok(ActiveColumns::from_stored(mode, stored.as_ref()))
}

/// The mode's sort as the op reads it now (K5.1).
fn current_sort(scope: &WriteScope<'_>, mode: SearchMode) -> Result<Sort, EngineError> {
    let (column_key, backwards_key) = sort_keys(mode);
    let column = scope.config_value(column_key.name())?;
    let backwards = scope.config_value(backwards_key.name())?;
    Ok(Sort::from_stored(mode, column.as_ref(), backwards.as_ref()))
}

/// The saved searches as the op reads them now.
fn saved(scope: &WriteScope<'_>) -> Result<SavedSearches, EngineError> {
    let stored = scope.config_value(Key::SavedFilters.name())?;
    Ok(SavedSearches::from_stored(stored.as_ref()))
}

/// A write the user cannot undo (K10.2).
fn non_undoable(name: &'static str) -> OpSpec {
    OpSpec::named(name, OpSource::UserNonUndoable)
}

/// A saved-search write: one undoable step named `step` (K10.2, R2020).
fn saved_search_step(name: &'static str, step: &'static str) -> OpSpec {
    OpSpec::named(name, OpSource::UserUndoable).undo_step(step, StepKind::Other)
}

/// K10.1's whole-value write as one op with no undo entry (the scenario op
/// `browser_config_set`, `specs/conformance/browser-config-ops.md` B1).
pub fn set_value(collection: &Collection, key: Key, value: &ConfigValue) -> Result<OpResult<bool>, EngineError> {
    let value = value.clone();
    collection.run(non_undoable("set_browser_config"), move |ctx| {
        set_value_in(ctx.scope(), key, &value)
    })
}

/// K4.2 Show column.
pub fn show_column(collection: &Collection, mode: SearchMode, column: Column) -> Result<OpResult<bool>, EngineError> {
    collection.run(non_undoable("show_browser_column"), move |ctx| {
        show_column_in(ctx.scope(), mode, column)
    })
}

/// K4.2 Hide column; refused `last_column`.
pub fn hide_column(collection: &Collection, mode: SearchMode, column: Column) -> Result<OpResult<bool>, EngineError> {
    collection.run(non_undoable("hide_browser_column"), move |ctx| {
        hide_column_in(ctx.scope(), mode, column)
    })
}

/// K4.3 Reorder; refused `invalid_order`.
pub fn reorder_columns(
    collection: &Collection,
    mode: SearchMode,
    order: &[Column],
) -> Result<OpResult<bool>, EngineError> {
    let order = order.to_vec();
    collection.run(non_undoable("reorder_browser_columns"), move |ctx| {
        reorder_columns_in(ctx.scope(), mode, &order)
    })
}

/// K5.2 Sort by column (a header click); refused `unsortable_column`
/// before any op runs.
pub fn sort_by_column(
    collection: &Collection,
    mode: SearchMode,
    column: Column,
) -> Result<OpResult<bool>, EngineError> {
    Sort::chosen(mode, column, SortDirection::Ascending).map_err(refused)?;
    collection.run(non_undoable("sort_browser"), move |ctx| {
        sort_by_column_in(ctx.scope(), mode, column)
    })
}

/// K5.2 Set sort; refused `unsortable_column` before any op runs.
pub fn set_sort(
    collection: &Collection,
    mode: SearchMode,
    column: Column,
    direction: SortDirection,
) -> Result<OpResult<bool>, EngineError> {
    Sort::chosen(mode, column, direction).map_err(refused)?;
    collection.run(non_undoable("sort_browser"), move |ctx| {
        set_sort_in(ctx.scope(), mode, column, direction)
    })
}

/// K6.1: Browse's Cards/Notes switch.
pub fn set_notes_mode(collection: &Collection, notes: bool) -> Result<OpResult<bool>, EngineError> {
    collection.run(non_undoable("set_browser_mode"), move |ctx| {
        set_notes_mode_in(ctx.scope(), notes)
    })
}

/// K6.2: collapsing or expanding a sidebar section.
pub fn set_section_collapsed(
    collection: &Collection,
    section: Section,
    collapsed: bool,
) -> Result<OpResult<bool>, EngineError> {
    collection.run(non_undoable("collapse_sidebar_section"), move |ctx| {
        set_section_collapsed_in(ctx.scope(), section, collapsed)
    })
}

/// K6.3: the previewer's both-sides toggle.
pub fn set_preview_both_sides(collection: &Collection, on: bool) -> Result<OpResult<bool>, EngineError> {
    collection.run(non_undoable("set_preview_both_sides"), move |ctx| {
        set_preview_both_sides_in(ctx.scope(), on)
    })
}

/// K6.4: Settings › Browse's default search text.
pub fn set_default_search_text(collection: &Collection, text: &str) -> Result<OpResult<bool>, EngineError> {
    let text = text.to_owned();
    collection.run(non_undoable("set_default_search_text"), move |ctx| {
        set_default_search_text_in(ctx.scope(), &text)
    })
}

/// K6.5: Settings › Browse's "Ignore accents in search".
pub fn set_ignore_accents_in_search(collection: &Collection, on: bool) -> Result<OpResult<bool>, EngineError> {
    collection.run(non_undoable("set_ignore_accents_in_search"), move |ctx| {
        set_ignore_accents_in_search_in(ctx.scope(), on)
    })
}

/// K7.2 Save Search, one "Save Search" step; refused `empty_name` before
/// any op runs.
pub fn save_search(collection: &Collection, name: &str, search: &str) -> Result<OpResult<bool>, EngineError> {
    if name.is_empty() {
        return Err(refused(Refusal::EmptyName));
    }
    let (name, search) = (name.to_owned(), search.to_owned());
    collection.run(saved_search_step("save_search", SAVE_SEARCH), move |ctx| {
        save_search_in(ctx.scope(), &name, &search)
    })
}

/// K7.2 Update Saved Search, one "Update Saved Search" step.
pub fn update_saved_search(collection: &Collection, name: &str, search: &str) -> Result<OpResult<bool>, EngineError> {
    let (name, search) = (name.to_owned(), search.to_owned());
    collection.run(
        saved_search_step("update_saved_search", UPDATE_SAVED_SEARCH),
        move |ctx| update_saved_search_in(ctx.scope(), &name, &search),
    )
}

/// K7.2 Rename Saved Search, one "Rename Saved Search" step.
pub fn rename_saved_search(collection: &Collection, old: &str, new_text: &str) -> Result<OpResult<bool>, EngineError> {
    let (old, new_text) = (old.to_owned(), new_text.to_owned());
    collection.run(
        saved_search_step("rename_saved_search", RENAME_SAVED_SEARCH),
        move |ctx| rename_saved_search_in(ctx.scope(), &old, &new_text),
    )
}

/// K7.2 Delete Saved Searches, one "Delete Saved Search" step.
pub fn delete_saved_searches(collection: &Collection, names: &[&str]) -> Result<OpResult<bool>, EngineError> {
    let names: Vec<String> = names.iter().map(|name| (*name).to_owned()).collect();
    collection.run(
        saved_search_step("delete_saved_searches", DELETE_SAVED_SEARCH),
        move |ctx| {
            let names: Vec<&str> = names.iter().map(String::as_str).collect();
            delete_saved_searches_in(ctx.scope(), &names)
        },
    )
}
