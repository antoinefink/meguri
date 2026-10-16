//! The outside values a search depends on (`specs/search/query-model.md` Q8).
//! The engine fills a [`SearchContext`]; search code never calls scheduling
//! or engine code (KTD2). A plan records which of these values it read
//! ([`ContextReads`]), so caches key on exactly those (U90).

use meguri_model::ids::DeckId;

/// How text terms fold letter case (Q8; `specs/search/case-folding.md`, U81).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FoldMode {
    /// Unicode simple case folding (R1036): the product's only mode.
    #[default]
    Unicode,
    /// Anki's ASCII-only folding, so that differential runs can compare exact
    /// results with the oracle (U91).
    #[cfg(feature = "test-hooks")]
    AsciiCompat,
}

/// Every outside value a query may read (Q8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchContext {
    /// The clock, in epoch seconds.
    pub now: i64,
    /// Days elapsed (U40 day clock).
    pub today: u32,
    /// Epoch seconds of the next day boundary (U40).
    pub next_day_at: i64,
    /// Config `collapseTime`; [`SearchContext::DEFAULT_LEARN_AHEAD_SECS`]
    /// when missing.
    pub learn_ahead_secs: u32,
    /// Config `curDeck`.
    pub current_deck: DeckId,
    /// Config `normalize_note_text`; true when missing.
    pub normalize_note_text: bool,
    /// Config `ignoreAccentsInSearch`; false when missing.
    pub ignore_accents: bool,
    pub fold: FoldMode,
}

impl SearchContext {
    /// Q8: the learn-ahead limit when config `collapseTime` is missing.
    pub const DEFAULT_LEARN_AHEAD_SECS: u32 = 1200;
}

/// One value of [`SearchContext`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ContextValue {
    Now,
    Today,
    NextDayAt,
    LearnAhead,
    CurrentDeck,
    NormalizeNoteText,
    IgnoreAccents,
    Fold,
}

impl ContextValue {
    pub const ALL: [ContextValue; 8] = [
        ContextValue::Now,
        ContextValue::Today,
        ContextValue::NextDayAt,
        ContextValue::LearnAhead,
        ContextValue::CurrentDeck,
        ContextValue::NormalizeNoteText,
        ContextValue::IgnoreAccents,
        ContextValue::Fold,
    ];

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// A set of [`ContextValue`]s: what a plan read (Q9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ContextReads(u8);

impl ContextReads {
    pub const NONE: ContextReads = ContextReads(0);

    #[must_use]
    pub fn with(self, value: ContextValue) -> ContextReads {
        ContextReads(self.0 | value.bit())
    }

    #[must_use]
    pub fn union(self, other: ContextReads) -> ContextReads {
        ContextReads(self.0 | other.0)
    }

    pub fn contains(self, value: ContextValue) -> bool {
        self.0 & value.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The values in the set, in [`ContextValue::ALL`] order.
    pub fn iter(self) -> impl Iterator<Item = ContextValue> {
        ContextValue::ALL.into_iter().filter(move |v| self.contains(*v))
    }
}

impl FromIterator<ContextValue> for ContextReads {
    fn from_iter<I: IntoIterator<Item = ContextValue>>(values: I) -> ContextReads {
        values.into_iter().fold(ContextReads::NONE, ContextReads::with)
    }
}
