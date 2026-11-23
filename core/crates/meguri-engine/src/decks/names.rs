//! Naming and component-aware hierarchy rules (D1-D3).
use super::{Refusal, is_filtered, refused};
use crate::EngineError;
use meguri_model::{deck::Deck, ids::DeckId};
use meguri_text::fold::compare_folded;

pub use meguri_model::deck::{human_name, normalize_native_name};

/// D1's stored result: S11 input conversion, then stored-name normalization.
pub fn native_name(human: &str) -> String {
    normalize_native_name(&meguri_model::deck::native_name(human))
}
pub fn same(a: &str, b: &str) -> bool {
    compare_folded(a, b).is_eq()
}
pub fn parent(name: &str) -> Option<&str> {
    name.rsplit_once('\u{1f}').map(|(p, _)| p)
}
pub fn descendant(name: &str, ancestor: &str) -> bool {
    let mut parts = name.split('\u{1f}');
    for part in ancestor.split('\u{1f}') {
        if !parts.next().is_some_and(|p| same(p, part)) {
            return false;
        }
    }
    parts.next().is_some()
}
pub fn find<'a>(rows: &'a [Deck], name: &str) -> Option<&'a Deck> {
    rows.iter().find(|d| same(&d.name, name))
}
pub fn unique(rows: &[Deck], id: DeckId, mut name: String) -> String {
    while rows.iter().any(|d| d.id != id && same(&d.name, &name)) {
        name.push('+');
    }
    name
}

/// Refuse before any missing parent is written; adopt the nearest ancestor.
pub fn prepare(rows: &[Deck], mut name: String) -> Result<String, EngineError> {
    let mut next = parent(&name);
    let mut missing = 0;
    while let Some(p) = next {
        if let Some(ancestor) = find(rows, p) {
            if is_filtered(ancestor) {
                return Err(refused(Refusal::FilteredParent));
            }
            name = format!("{}{}", ancestor.name, &name[p.len()..]);
            return Ok(name);
        }
        missing += 1;
        if missing >= 11 {
            return Err(refused(Refusal::TooDeep));
        }
        next = parent(p);
    }
    Ok(name)
}
