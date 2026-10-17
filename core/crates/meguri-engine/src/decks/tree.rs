//! The deck tree without queue counts (D12).
use super::{is_filtered, names};
use crate::EngineError;
use meguri_model::ids::DeckId;
use meguri_storage::ReadRows;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub id: DeckId,
    pub name: String,
    pub filtered: bool,
    pub study_collapsed: bool,
    pub browser_collapsed: bool,
    pub children: Vec<Node>,
}

pub fn tree(rows: &impl ReadRows) -> Result<Vec<Node>, EngineError> {
    let decks = rows.deck_rows()?;
    let other_root = decks
        .iter()
        .any(|d| d.id != DeckId(1) && names::parent(&d.name).is_none());
    let hide_default = other_root
        && !rows.deck_has_cards(DeckId(1))?
        && decks.iter().find(|d| d.id == DeckId(1)).is_some_and(|default| {
            names::parent(&default.name).is_none() && !decks.iter().any(|d| names::descendant(&d.name, &default.name))
        });
    fn children(decks: &[meguri_model::deck::Deck], parent: Option<&str>, hide_default: bool) -> Vec<Node> {
        decks
            .iter()
            .filter(|d| !(hide_default && d.id == DeckId(1)))
            .filter(|d| match (names::parent(&d.name), parent) {
                (None, None) => true,
                (Some(a), Some(b)) => names::same(a, b),
                _ => false,
            })
            .map(|d| Node {
                id: d.id,
                name: d.name.rsplit('\u{1f}').next().unwrap_or_default().to_owned(),
                filtered: is_filtered(d),
                study_collapsed: d.common.study_collapsed,
                browser_collapsed: d.common.browser_collapsed,
                children: children(decks, Some(&d.name), hide_default),
            })
            .collect()
    }
    Ok(children(&decks, None, hide_default))
}
