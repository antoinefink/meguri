//! The one-shot id search (`browser/result-sets` H13) for the local
//! servers: blocking, on a server reader, every matching id ascending, no
//! handle kept. Its context is the engine's (H9).

use meguri_search::{BuildError, Plan, SearchMode, SearchNotetypes};
use meguri_storage::ReadRows;

use crate::lifecycle::Engine;

use super::{BrowseError, worker};

/// H13: every id of `mode` that `text` matches, ascending; the empty text
/// matches every id. A search error is the one `Failed` gives Browse.
pub fn search_ids(engine: &Engine, text: &str, mode: SearchMode) -> Result<Vec<i64>, BrowseError> {
    let query = meguri_search::parse(text).map_err(BrowseError::Search)?;
    let collection = engine.collection()?;
    // Listed for halts (replace, suspend, close), so its reader comes back
    // at once.
    let token = crate::cancel::CancelToken::new();
    let _running = engine.inner.searches.run(&token, engine.inner.searches.epoch());
    let index = engine.inner.searches.server_reader();
    let lease = collection.server_reader(index)?;
    let snapshot = lease.snapshot()?;
    let context = worker::context(&collection, &*snapshot)?;
    let notetypes = SearchNotetypes::read(&*snapshot)?;
    let plan = Plan::build(&query, mode, &context, &notetypes).map_err(|BuildError::Search(error)| BrowseError::Search(error))?;
    let mut ids = Vec::new();
    snapshot
        .visit_id_rows(plan.sql(), &worker::cells(plan.params()), &|| token.is_cancelled(), &mut |row| {
            ids.push(row[0]);
            true
        })
        .map_err(|_| {
            if token.is_cancelled() {
                BrowseError::Engine(crate::error::EngineError::Cancelled)
            } else {
                BrowseError::Search(worker::refusal(text))
            }
        })?;
    ids.sort_unstable();
    Ok(ids)
}
