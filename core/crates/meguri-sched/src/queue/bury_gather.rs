use super::Kind;
use meguri_model::{card::Card, ids::NoteId, proto::deck_config::DeckConfigMessage};
use std::collections::BTreeMap;
#[derive(Default)]
pub struct BuryModes(BTreeMap<NoteId, [bool; 3]>);
impl BuryModes {
    pub fn skips(&self, c: &Card, kind: Kind) -> bool {
        let i = match kind {
            Kind::New => 0,
            Kind::Review => 1,
            Kind::InterdayLearning => 2,
            Kind::Learning => return false,
        };
        self.0.get(&c.note_id).is_some_and(|flags| flags[i])
    }
    /// Called only for offered, non-limit-refused cards, including skipped
    /// siblings. Their own preset affects subsequent siblings, never themselves.
    pub fn offer(&mut self, c: &Card, kind: Kind, p: &DeckConfigMessage) -> bool {
        let skip = self.skips(c, kind);
        let flags = self.0.entry(c.note_id).or_default();
        for (stored, setting) in flags
            .iter_mut()
            .zip([p.bury_new, p.bury_reviews, p.bury_interday_learning])
        {
            *stored |= setting;
        }
        skip
    }
}
