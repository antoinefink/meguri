//! Bounded parsed-syntax cache. Exact text is the snapshot witness (K11):
//! timestamps and change delivery alone cannot distinguish same-second edits.
use meguri_model::ids::NotetypeId;
use meguri_template::{ParsedTemplate, card::RenderMode};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

type Key = (NotetypeId, u32, RenderMode);
#[derive(Clone)]
struct Pair {
    question: Arc<ParsedTemplate>,
    answer: Arc<ParsedTemplate>,
}
#[derive(Default)]
struct State {
    entries: HashMap<Key, Pair>,
    bytes: usize,
}
#[derive(Default)]
pub(crate) struct ParseCache(Mutex<State>);
impl ParseCache {
    pub(crate) fn get(
        &self,
        id: NotetypeId,
        ord: u32,
        mode: RenderMode,
        question: &str,
        answer: &str,
    ) -> (Arc<ParsedTemplate>, Arc<ParsedTemplate>) {
        let key = (id, ord, mode);
        {
            let state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(pair) = state
                .entries
                .get(&key)
                .filter(|p| p.question.source() == question && p.answer.source() == answer)
            {
                return (pair.question.clone(), pair.answer.clone());
            }
        }
        let pair = Pair {
            question: Arc::new(ParsedTemplate::parse(question)),
            answer: Arc::new(ParsedTemplate::parse(answer)),
        };
        let bytes = weight(&pair);
        // Include flat nodes and their owned strings in a conservative
        // retention weight. Oversized parses render without entering cache.
        if bytes <= 32 * 1024 * 1024 {
            let mut state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(old) = state.entries.remove(&key) {
                state.bytes -= weight(&old);
            }
            if state.entries.len() >= 32 || state.bytes.saturating_add(bytes) > 32 * 1024 * 1024 {
                state.entries.clear();
                state.bytes = 0;
            }
            state.bytes += bytes;
            state.entries.insert(key, pair.clone());
        }
        (pair.question, pair.answer)
    }
}

fn weight(pair: &Pair) -> usize {
    let sources = pair.question.source().len().saturating_add(pair.answer.source().len());
    let nodes = pair.question.nodes().len().saturating_add(pair.answer.nodes().len());
    sources
        .saturating_mul(8)
        .saturating_add(nodes.saturating_mul(2 * std::mem::size_of::<meguri_template::parser::Node>()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reuse_requires_exact_sources_even_after_an_older_snapshot_is_inserted() {
        let cache = ParseCache::default();
        let get = |q| cache.get(NotetypeId(1), 0, RenderMode::Review, q, "{{FrontSide}}");
        let (first, _) = get("old");
        assert!(Arc::ptr_eq(&first, &get("old").0));
        assert_eq!(get("new").0.source(), "new");
        assert_eq!(get("old").0.source(), "old");
        assert_eq!(get("new").0.source(), "new");
    }
}
