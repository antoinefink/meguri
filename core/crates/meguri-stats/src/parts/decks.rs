//! What the parts read of the decks on the request's snapshot: the desired
//! retention in force of each normal deck (`sched/presets` V7, for
//! `stats/history-reads` H3.4 and H4.2) and the deck tree (`sched/counts`
//! C1, for H10's rows).

use std::collections::HashMap;

use meguri_engine::decks::tree::{self, Node};
use meguri_engine::EngineError;
use meguri_model::ids::DeckConfigId;
use meguri_model::proto::deck::DeckKind;
use meguri_sched::model::deck_sched::DeckSched;
use meguri_sched::model::preset::PresetView;
use meguri_storage::ReadRows;

/// The decks as the request's snapshot holds them.
#[derive(Debug, Default)]
pub(crate) struct DeckFacts {
    /// Each normal deck's desired retention in force: its own value when it
    /// has one, else its preset's, read with V3 (the built-in defaults for a
    /// preset id that names none, V1). A filtered deck has none (V7).
    pub retention: HashMap<i64, f32>,
    /// The deck tree: siblings in C1's order; a deck whose parent is
    /// missing is left out with its descendants.
    pub tree: Vec<Node>,
}

impl DeckFacts {
    pub(crate) fn read(rows: &impl ReadRows) -> Result<DeckFacts, EngineError> {
        let mut presets: HashMap<i64, f32> = HashMap::new();
        let mut retention = HashMap::new();
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
        Ok(DeckFacts {
            retention,
            tree: tree::tree(rows)?,
        })
    }

    /// The retention in force for a card's home deck (`odid` when not 0,
    /// else `did`); none when that deck is missing or filtered.
    pub(crate) fn retention_of(&self, did: i64, odid: i64) -> Option<f32> {
        self.retention.get(&home_deck(did, odid)).copied()
    }
}

/// The home deck of a card (`stats/history-reads` H1).
pub(crate) fn home_deck(did: i64, odid: i64) -> i64 {
    if odid != 0 { odid } else { did }
}
