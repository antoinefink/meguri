//! curDeck (D11): a local write that preserves undo history.
use super::require;
use crate::{Collection, EngineError, OpResult, OpSource, OpSpec};
use meguri_model::ids::DeckId;
use meguri_storage::{ConfigValue, ReadRows, WriteScope};

pub fn current(rows: &impl ReadRows) -> Result<DeckId, EngineError> {
    let id = rows.config_value("curDeck")?.and_then(|v| v.as_i64()).map(DeckId);
    Ok(match id {
        Some(id) if rows.deck(id)?.is_some() => id,
        _ => DeckId(1),
    })
}
pub fn select_in(scope: &mut WriteScope<'_>, id: DeckId) -> Result<(), EngineError> {
    require(scope, id)?;
    scope.set_config("curDeck", &ConfigValue::from(id.0))?;
    Ok(())
}
pub fn select(collection: &Collection, id: DeckId) -> Result<OpResult<()>, EngineError> {
    collection.run(
        OpSpec::named("set_current_deck", OpSource::UserNonUndoable).skip_undo(),
        move |ctx| select_in(ctx.scope(), id),
    )
}
