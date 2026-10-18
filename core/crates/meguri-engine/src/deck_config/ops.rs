use super::{defaults, require};
use crate::decks::{self, Refusal, names};
use crate::{Collection, EngineError, OpResult, SchemaReason};
use meguri_model::{
    ids::{DeckConfigId, DeckId},
    proto::deck::DeckKind,
};
use meguri_storage::{ReadRows, WriteScope};

/// P4: the pinned FSRS-6 defaults, each stored as f32.
const FSRS6: [f32; 21] = [
    0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614, 0.2629,
    1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

pub fn add_in(scope: &mut WriteScope<'_>, name: &str) -> Result<DeckConfigId, EngineError> {
    let mut preset = defaults();
    preset.name = name.to_owned();
    preset.config.set_fsrs_params_6(FSRS6.to_vec());
    Ok(scope.add_deck_config(&preset)?)
}
pub fn clone_in(scope: &mut WriteScope<'_>, id: DeckConfigId, name: &str) -> Result<DeckConfigId, EngineError> {
    let mut preset = require(scope, id)?;
    preset.name = name.to_owned();
    Ok(scope.add_deck_config(&preset)?)
}
pub fn rename_in(scope: &mut WriteScope<'_>, id: DeckConfigId, name: &str) -> Result<(), EngineError> {
    let mut preset = require(scope, id)?;
    preset.name = name.to_owned();
    scope.put(&preset)?;
    Ok(())
}

/// P10: edit only live decoded fields; older FSRS lists have no setters.
/// The caller's operation supplies the undo name and transaction.
pub fn update_in(
    scope: &mut WriteScope<'_>,
    id: DeckConfigId,
    edit: impl FnOnce(&mut meguri_model::proto::deck_config::DeckConfigMessage),
) -> Result<(), EngineError> {
    let preset = require(scope, id)?;
    let view = meguri_sched::model::preset::PresetView::new(&preset);
    if let Some(changed) = view.edit(edit).map_err(|error| EngineError::InvalidArgument {
        message: error.to_string(),
    })? {
        scope.put(&changed)?;
    }
    Ok(())
}

pub fn assign_in(
    scope: &mut WriteScope<'_>,
    id: DeckId,
    preset: DeckConfigId,
    subtree: bool,
) -> Result<(), EngineError> {
    require(scope, preset)?;
    let deck = decks::require(scope, id)?;
    if decks::is_filtered(&deck) {
        return Err(decks::refused(Refusal::FilteredDeck));
    }
    for mut row in scope
        .deck_rows()?
        .into_iter()
        .filter(|d| d.id == id || (subtree && names::descendant(&d.name, &deck.name)))
    {
        if let DeckKind::Normal(ref mut normal) = row.kind.kind {
            normal.config_id = preset.0;
            decks::save_in(scope, row)?;
        }
    }
    Ok(())
}

pub fn remove_in(
    scope: &mut WriteScope<'_>,
    id: DeckConfigId,
    fallback: DeckConfigId,
    confirmed: bool,
) -> Result<(), EngineError> {
    if id == DeckConfigId(1) {
        return Err(decks::refused(Refusal::DefaultPreset));
    }
    require(scope, id)?;
    if id == fallback || scope.deck_config(fallback)?.is_none() {
        return Err(EngineError::InvalidArgument {
            message: "fallback must exist and differ from the removed preset".into(),
        });
    }
    crate::mark_schema_change(scope, &[SchemaReason::PresetRemoved], confirmed)?;
    for mut deck in scope.deck_rows()? {
        if let DeckKind::Normal(ref mut normal) = deck.kind.kind
            && (normal.config_id == id.0 || scope.deck_config(DeckConfigId(normal.config_id))?.is_none())
        {
            normal.config_id = fallback.0;
            decks::save_in(scope, deck)?;
        }
    }
    scope.remove_deck_config(id)?;
    Ok(())
}
fn spec(op: &'static str) -> crate::OpSpec {
    decks::ops::spec(op, crate::undo::names::DECK_OPTIONS)
}
pub fn add(c: &Collection, name: String) -> Result<OpResult<DeckConfigId>, EngineError> {
    c.run(spec("add_preset"), move |ctx| add_in(ctx.scope(), &name))
}
pub fn clone(c: &Collection, id: DeckConfigId, name: String) -> Result<OpResult<DeckConfigId>, EngineError> {
    c.run(spec("clone_preset"), move |ctx| clone_in(ctx.scope(), id, &name))
}
pub fn rename(c: &Collection, id: DeckConfigId, name: String) -> Result<OpResult<()>, EngineError> {
    c.run(spec("rename_preset"), move |ctx| rename_in(ctx.scope(), id, &name))
}
pub fn assign(c: &Collection, deck: DeckId, preset: DeckConfigId, subtree: bool) -> Result<OpResult<()>, EngineError> {
    c.run(spec("set_deck_preset"), move |ctx| {
        assign_in(ctx.scope(), deck, preset, subtree)
    })
}
pub fn remove(
    c: &Collection,
    id: DeckConfigId,
    fallback: DeckConfigId,
    confirmed: bool,
) -> Result<OpResult<()>, EngineError> {
    c.run(spec("remove_preset"), move |ctx| {
        remove_in(ctx.scope(), id, fallback, confirmed)
    })
}
