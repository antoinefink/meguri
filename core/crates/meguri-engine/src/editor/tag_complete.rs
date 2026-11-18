//! Read-only tag completion: tags/complete C1–C4.
use std::ops::ControlFlow;

use meguri_storage::{ReadRows, ReaderRole};

use crate::{CancelToken, Collection, EngineError, tags::canonical::components};

/// Select whole registry rows on one Query snapshot, before partitioning
/// prefix/inner matches. The limit includes C7's 2^32 without narrowing.
pub fn tag_complete(
    collection: &Collection,
    input: &str,
    limit: u64,
    cancel: &CancelToken,
) -> Result<Vec<String>, EngineError> {
    cancel.check()?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let reader = collection.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    complete_from(input, limit, cancel, |each| {
        snapshot.visit_tag_names(|| cancel.check(), each)
    })
}

/// The source visits one owned name at a time and honors Break. Keeping
/// selection here also permits deterministic cancellation-boundary tests.
fn complete_from(
    input: &str,
    limit: u64,
    cancel: &CancelToken,
    visit: impl FnOnce(&mut dyn FnMut(String) -> Result<ControlFlow<()>, EngineError>) -> Result<(), EngineError>,
) -> Result<Vec<String>, EngineError> {
    cancel.check()?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let filters: Vec<String> = components(input).map(simple_fold).collect();
    let mut prefixes = Vec::new();
    let mut inners = Vec::new();
    let mut remaining = limit;
    visit(&mut |name| {
        cancel.check()?;
        if let Some(prefix) = is_prefix_match(&name, &filters) {
            if prefix {
                prefixes.push(name);
            } else {
                inners.push(name);
            }
            remaining -= 1;
        }
        Ok(if remaining == 0 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) })
    })?;
    cancel.check()?;
    prefixes.extend(inners);
    cancel.check()?;
    Ok(prefixes)
}

fn simple_fold(text: &str) -> String {
    let mapper = icu_casemap::CaseMapper::new();
    text.chars().map(|ch| mapper.simple_fold(ch)).collect()
}

/// None is no match; Some(false) is an inner match. Each filter consumes
/// the first component that contains it, even if a later one starts with it.
fn is_prefix_match(name: &str, filters: &[String]) -> Option<bool> {
    let mut parts = components(name).enumerate();
    let mut prefix = true;
    for (filter_index, filter) in filters.iter().enumerate() {
        let (component_index, offset) = parts.find_map(|(index, part)| {
            simple_fold(part).find(filter.as_str()).map(|offset| (index, offset))
        })?;
        prefix &= component_index == filter_index && offset == 0;
    }
    Some(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_fit_distinguishes_inner_and_prefix_and_preserves_empty_filters() {
        for (name, input, expected) in [
            ("Japanese::Audio", "a", Some(false)),
            ("Japanese::Audio", "jap::a", Some(true)),
            ("Root::Japanese::Audio", "root::a", Some(false)),
            ("German::Verb::modal", "ger::mod", Some(false)),
            ("German::Verb::modal", "ger::v", Some(true)),
            ("German::Verb::modal", "verb::ger", None),
            ("a:::b", "a:::b", Some(true)),
            ("a:::b", "a::b", Some(false)),
            ("a::::b", "a::::b", Some(true)),
            ("a::", "a::", Some(true)),
            ("a", "a::", None),
            ("::", "::", Some(true)),
            ("", "", Some(true)),
        ] {
            let filters = components(input).map(simple_fold).collect::<Vec<_>>();
            assert_eq!(is_prefix_match(name, &filters), expected, "{name:?}, {input:?}");
        }
    }

    #[test]
    fn cancellation_between_rows_and_before_return_discards_every_match() {
        // Cancel after a nonmatch, an inner match, or the final selected row.
        for (name, limit) in [("Other", 2), ("Grammar::Japan", 2), ("Japan", 1)] {
            let cancel = CancelToken::new();
            let mut visits = 0;
            let result = complete_from("jap", limit, &cancel, |each| {
                visits += 1;
                let flow = each(name.into())?;
                cancel.cancel();
                if flow.is_continue() {
                    // A pending next row is refused, never partially returned.
                    visits += 1;
                    let _ = each("Japanese".into())?;
                }
                Ok(())
            });
            assert_eq!(result, Err(EngineError::Cancelled));
            assert_eq!(visits, if limit == 1 { 1 } else { 2 });
        }
        let cancel = CancelToken::new();
        let result = complete_from("", 2, &cancel, |each| {
            let _ = each("Japan".into())?;
            cancel.cancel(); // cancellation at source exhaustion
            Ok(())
        });
        assert_eq!(result, Err(EngineError::Cancelled));

        cancel.cancel();
        assert_eq!(complete_from("", 0, &cancel, |_| panic!("must not read")), Err(EngineError::Cancelled));
        assert_eq!(complete_from("", 0, &CancelToken::default(), |_| panic!("must not read")), Ok(vec![]));
    }
}
