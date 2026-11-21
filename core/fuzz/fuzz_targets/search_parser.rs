//! Search text never panics (specs/search/errors.md E5, E6): every input
//! parses or gives an error whose range is a non-empty part of the text;
//! every query that parses has a canonical text, which reads back without a
//! panic, and plans for both modes build or fail without one.

#![no_main]

use libfuzzer_sys::fuzz_target;
use meguri_model::ids::{DeckId, NotetypeId};
use meguri_search::{FieldShape, FoldMode, NotetypeShape, Plan, SearchContext, SearchMode, SearchNotetypes, parse};

/// Note types that reach every text-term path: name resolution, runs,
/// excluded fields, an excluded sort field and a cloze kind.
fn notetypes() -> SearchNotetypes {
    let shape = |id: i64, name: &str, cloze: bool, sort_field: u32, fields: &[(&str, bool)]| NotetypeShape {
        id: NotetypeId(id),
        name: name.to_owned(),
        cloze,
        sort_field,
        fields: fields
            .iter()
            .map(|&(name, excluded)| FieldShape {
                name: name.to_owned(),
                excluded,
            })
            .collect(),
    };
    SearchNotetypes::new(vec![
        shape(1, "Basic", false, 0, &[("Front", false), ("Back", false)]),
        shape(2, "Memo", false, 2, &[("Front", false), ("Back", false), ("Notes", true)]),
        shape(3, "Cloze", true, 0, &[("Text", false), ("Back Extra", true)]),
    ])
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let length = text.chars().count();
    match parse(&text) {
        Ok(query) => {
            let canonical = query.canonical_text();
            let _ = parse(&canonical);
            let context = SearchContext {
                now: 0,
                today: 0,
                next_day_at: 86_400,
                learn_ahead_secs: SearchContext::DEFAULT_LEARN_AHEAD_SECS,
                current_deck: DeckId(1),
                normalize_note_text: true,
                ignore_accents: false,
                fold: FoldMode::Unicode,
            };
            let notetypes = notetypes();
            for mode in [SearchMode::Cards, SearchMode::Notes] {
                let _ = Plan::build(&query, mode, &context, &SearchNotetypes::default());
                let _ = Plan::build(&query, mode, &context, &notetypes);
            }
        }
        Err(error) => {
            let range = error.range();
            assert!(range.start < range.end && range.end <= length, "{text:?}: {error:?}");
            assert!(error.kind().is_parse_error(), "{text:?}: {error:?}");
            let _ = error.message();
        }
    }
});
