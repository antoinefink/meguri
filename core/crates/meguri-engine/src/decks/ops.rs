//! D5-D7, D10, D13. One standalone call is one transaction and undo step.
use super::{Refusal, is_filtered, names, new_deck, refused, require};
use crate::undo::names as undo;
use crate::{Collection, EngineError, OpResult, OpSpec, StepKind};
use meguri_model::{changes::OpSource, deck::Deck, ids::DeckId};
use meguri_storage::{ReadRows, WriteScope};

pub(crate) fn spec(op: &'static str, name: &'static str) -> OpSpec {
    OpSpec::named(op, OpSource::UserUndoable).undo_step(name, StepKind::Other)
}

fn parents_in(scope: &mut WriteScope<'_>, name: &str) -> Result<(), EngineError> {
    let rows = scope.deck_rows()?;
    let mut p = names::parent(name);
    while let Some(name) = p {
        if names::find(&rows, name).is_some() {
            break;
        }
        scope.add_deck(&new_deck(name.to_owned()))?;
        p = names::parent(name);
    }
    Ok(())
}

pub fn add_in(scope: &mut WriteScope<'_>, human: &str) -> Result<Deck, EngineError> {
    let rows = scope.deck_rows()?;
    let name = names::native_name(human);
    if let Some(existing) = names::find(&rows, &name) {
        return Ok(existing.clone());
    }
    let name = names::prepare(&rows, name)?;
    parents_in(scope, &name)?;
    let id = scope.add_deck(&new_deck(name))?;
    require(scope, id)
}

/// D6 with the caller's other field edits preserved (D13).
fn rename_record(scope: &mut WriteScope<'_>, mut deck: Deck, name: String) -> Result<Deck, EngineError> {
    let old = require(scope, deck.id)?;
    // The unique unicase index already protects a stored name. Ordinary
    // field edits need only this row, even when applied to a whole subtree.
    if name == old.name {
        deck.name = name;
        scope.put(&deck)?;
        return require(scope, deck.id);
    }
    let rows = scope.deck_rows()?;
    let name = names::unique(&rows, deck.id, name);
    if name == old.name {
        deck.name = name;
        scope.put(&deck)?;
        return require(scope, deck.id);
    }
    if let Some(parent) = names::parent(&name)
        && rows.iter().any(|d| {
            names::descendant(&d.name, &old.name)
                && (names::same(parent, &d.name) || names::descendant(parent, &d.name))
        })
    {
        return Err(refused(Refusal::UnderDescendant));
    }
    let name = names::prepare(&rows, name)?;
    parents_in(scope, &name)?;
    // Include ancestors just created underneath the old name (D6.6).
    replace_descendants(scope, &old.name, &name)?;
    deck.name = name.clone();
    scope.put(&deck)?;
    parents_in(scope, &name)?;
    require(scope, deck.id)
}

fn replace_descendants(scope: &mut WriteScope<'_>, old: &str, new: &str) -> Result<(), EngineError> {
    for mut child in scope
        .deck_rows()?
        .into_iter()
        .filter(|d| names::descendant(&d.name, old))
    {
        // Skip components rather than byte offsets: unicase may fold ß to ss.
        let tail = child
            .name
            .split('\u{1f}')
            .skip(old.split('\u{1f}').count())
            .collect::<Vec<_>>()
            .join("\u{1f}");
        child.name = format!("{new}\u{1f}{tail}");
        scope.put(&child)?;
    }
    Ok(())
}

pub fn rename_in(scope: &mut WriteScope<'_>, id: DeckId, human: &str) -> Result<Deck, EngineError> {
    rename_record(scope, require(scope, id)?, names::native_name(human))
}

/// Every other deck writer uses this path: normalize, make unique, move
/// descendants if needed, compare and stamp through storage.
pub fn save_in(scope: &mut WriteScope<'_>, deck: Deck) -> Result<Deck, EngineError> {
    let name = names::normalize_native_name(&deck.name);
    rename_record(scope, deck, name)
}

pub fn move_in(scope: &mut WriteScope<'_>, ids: &[DeckId], parent: Option<DeckId>) -> Result<usize, EngineError> {
    if let Some(id) = parent
        && is_filtered(&require(scope, id)?)
    {
        return Err(refused(Refusal::FilteredParent));
    }
    let mut count = 0;
    for &id in ids {
        let Some(mut deck) = scope.deck(id)? else { continue };
        let name = match parent {
            Some(target) => {
                let target = require(scope, target)?;
                if target.id == id || names::descendant(&target.name, &deck.name) {
                    continue;
                }
                format!(
                    "{}\u{1f}{}",
                    target.name,
                    deck.name.rsplit('\u{1f}').next().unwrap_or_default()
                )
            }
            None => deck.name.rsplit('\u{1f}').next().unwrap_or_default().to_owned(),
        };
        if name == deck.name {
            continue;
        }
        let name = names::unique(&scope.deck_rows()?, id, name);
        replace_descendants(scope, &deck.name, &name)?;
        deck.name = name;
        scope.put(&deck)?;
        count += 1;
    }
    Ok(count)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollapseScope {
    Study,
    Browser,
}
pub fn collapse_in(
    scope: &mut WriteScope<'_>,
    ids: &[DeckId],
    which: CollapseScope,
    collapsed: bool,
) -> Result<(), EngineError> {
    for &id in ids {
        if let Some(mut deck) = scope.deck(id)? {
            match which {
                CollapseScope::Study => deck.common.study_collapsed = collapsed,
                CollapseScope::Browser => deck.common.browser_collapsed = collapsed,
            }
            save_in(scope, deck)?;
        }
    }
    Ok(())
}

pub fn add(collection: &Collection, name: String) -> Result<OpResult<Deck>, EngineError> {
    collection.run(spec("add_deck", undo::ADD_DECK), move |ctx| add_in(ctx.scope(), &name))
}
pub fn rename(collection: &Collection, id: DeckId, name: String) -> Result<OpResult<Deck>, EngineError> {
    collection.run(spec("rename_deck", undo::RENAME_DECK), move |ctx| {
        rename_in(ctx.scope(), id, &name)
    })
}
pub fn move_decks(
    collection: &Collection,
    ids: Vec<DeckId>,
    parent: Option<DeckId>,
) -> Result<OpResult<usize>, EngineError> {
    collection.run(spec("reparent_decks", undo::RENAME_DECK), move |ctx| {
        move_in(ctx.scope(), &ids, parent)
    })
}
pub fn collapse(
    collection: &Collection,
    ids: Vec<DeckId>,
    which: CollapseScope,
    collapsed: bool,
) -> Result<OpResult<()>, EngineError> {
    collection.run(spec("set_deck_collapsed", undo::EXPAND_COLLAPSE), move |ctx| {
        collapse_in(ctx.scope(), &ids, which, collapsed)
    })
}
