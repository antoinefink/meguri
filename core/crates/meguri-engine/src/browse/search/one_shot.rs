//! The one-shot id search (`browser/result-sets` H13) for the local
//! servers: blocking, on a server reader, every matching id ascending, no
//! handle kept. Its context is the engine's (H9); a plan with slow terms
//! takes Q10's two stages, as a session's request would.

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
    let (epoch, stopping) = engine.inner.searches.epoch();
    if stopping {
        return Err(BrowseError::Engine(crate::error::EngineError::unavailable(
            "the collection is changing generation, suspending or closing",
        )));
    }
    let _running = engine.inner.searches.run(&token, epoch);
    let index = engine.inner.searches.server_reader();
    let lease = collection.server_reader(index)?;
    // A stop that came while the reader was busy ends the call here.
    if token.is_cancelled() {
        return Err(BrowseError::Engine(crate::error::EngineError::Cancelled));
    }
    let snapshot = lease.snapshot()?;
    let context = worker::context(&collection, &*snapshot)?;
    let notetypes = SearchNotetypes::read(&*snapshot)?;
    let plan = Plan::build(&query, mode, &context, &notetypes).map_err(|BuildError::Search(error)| BrowseError::Search(error))?;
    let failed = |_| {
        if token.is_cancelled() {
            BrowseError::Engine(crate::error::EngineError::Cancelled)
        } else {
            BrowseError::Search(worker::refusal(text))
        }
    };
    let read = |sql: &str, params: &[meguri_search::Param]| -> Result<Vec<i64>, BrowseError> {
        let mut ids = Vec::new();
        snapshot
            .visit_id_rows(sql, &worker::cells(params), &|| token.is_cancelled(), &mut |row| {
                ids.push(row[0]);
                true
            })
            .map_err(failed)?;
        Ok(ids)
    };
    if !plan.is_slow() {
        let mut ids = read(plan.sql(), plan.params())?;
        ids.sort_unstable();
        return Ok(ids);
    }
    // Q10 as H3 runs it: the relaxed query's candidates in ascending id
    // order (the mode's final key alone), then the original query on each
    // chunk, at the chunk size a test sets (H16).
    let relaxed =
        Plan::build_relaxed(&query, mode, &context, &notetypes).map_err(|BuildError::Search(error)| BrowseError::Search(error))?;
    let mut candidates = read(relaxed.sql(), relaxed.params())?;
    candidates.sort_unstable();
    let fixed = engine.inner.searches.stream_chunk();
    let chunk = fixed.unwrap_or(worker::DEFAULT_CHUNK);
    let sql = plan.restricted_sql(&format!("?{}", plan.params().len() + 1));
    let mut params = plan.params().to_vec();
    params.push(meguri_search::Param::Integer(0));
    let mut ids = Vec::new();
    let mut start = 0usize;
    let mut size = chunk;
    while start < candidates.len() {
        let end = (start + size).min(candidates.len());
        let window = &candidates[start..end];
        start = end;
        if fixed.is_none() {
            size = (size * 2).min(worker::MAX_CHUNK);
        }
        *params.last_mut().expect("the list parameter") = meguri_search::plan::sql::id_list(window);
        let keep: std::collections::HashSet<i64> = read(&sql, &params)?.into_iter().collect();
        ids.extend(window.iter().copied().filter(|id| keep.contains(id)));
    }
    Ok(ids)
}
