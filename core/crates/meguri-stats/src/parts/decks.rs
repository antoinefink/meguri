//! What the parts read of the decks on the request's snapshot: the desired
//! retention in force of each normal deck (`sched/presets` V7, for
//! `stats/history-reads` H3.4 and H4.2) and the decks by name in
//! `sched/counts` C1's sibling order (for H10's rows). Each is read on its
//! own, so a failure of one fails only the parts that need it (H15).

use std::collections::{BTreeMap, HashMap};

use meguri_engine::EngineError;
use meguri_engine::decks::is_filtered;
use meguri_model::ids::DeckConfigId;
use meguri_model::proto::deck::DeckKind;
use meguri_sched::model::deck_sched::DeckSched;
use meguri_sched::model::preset::PresetView;
use meguri_storage::ReadRows;

/// Each normal deck's desired retention in force: its own value when it has
/// one, else its preset's, read with V3 (the built-in defaults for a preset
/// id that names none, V1). A filtered deck has none (V7).
#[derive(Debug, Default)]
pub(crate) struct Retentions(BTreeMap<i64, f32>);

impl Retentions {
    pub(crate) fn read(rows: &impl ReadRows) -> Result<Retentions, EngineError> {
        let mut presets: HashMap<i64, f32> = HashMap::new();
        let mut retention = BTreeMap::new();
        for deck in rows.deck_rows()? {
            let DeckKind::Normal(normal) = &deck.kind.kind else {
                continue;
            };
            let preset = match presets.get(&normal.config_id) {
                Some(value) => *value,
                None => {
                    let value = PresetView::for_config(rows, DeckConfigId(normal.config_id))?
                        .fields()
                        .desired_retention;
                    presets.insert(normal.config_id, value);
                    value
                }
            };
            retention.insert(deck.id.0, DeckSched::new(Some(normal), &deck.common).retention(preset));
        }
        Ok(Retentions(retention))
    }

    /// The retention in force for a card's home deck (`odid` when not 0,
    /// else `did`); none when that deck is missing or filtered.
    pub(crate) fn of(&self, did: i64, odid: i64) -> Option<f32> {
        self.0.get(&home_deck(did, odid)).copied()
    }
}

/// One deck as By Deck places it.
#[derive(Clone, Debug)]
pub(crate) struct DeckEntry {
    pub id: i64,
    /// The stored name under simple case folding, as G2.1's `deck:` rule
    /// compares names.
    pub folded: String,
    pub filtered: bool,
}

/// Every deck, in the stored names' unicase order: so siblings come in
/// C1's order. Unlike the deck list's tree, no deck is hidden (Default)
/// or left out (a deck whose parent is missing): H10.1 places every card
/// of a normal deck.
pub(crate) fn deck_entries(rows: &impl ReadRows) -> Result<Vec<DeckEntry>, EngineError> {
    Ok(rows
        .deck_rows()?
        .iter()
        .map(|deck| DeckEntry {
            id: deck.id.0,
            folded: meguri_search::simple_fold(&deck.name),
            filtered: is_filtered(deck),
        })
        .collect())
}

/// The home deck of a card (`stats/history-reads` H1).
pub(crate) fn home_deck(did: i64, odid: i64) -> i64 {
    if odid != 0 { odid } else { did }
}
