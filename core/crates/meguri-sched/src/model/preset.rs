use meguri_model::{
    card::Card,
    deck_config::DeckConfig,
    ids::DeckConfigId,
    proto::deck::DeckKind,
    proto::{BlobMessage, ProtoBlob, deck_config::DeckConfigMessage},
};
use meguri_storage::{ReadRows, StorageError};

macro_rules! option_enum {
    ($name:ident, $first:ident $(, $variant:ident = $code:literal)* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(i32)]
        pub enum $name { $first = 0, $($variant = $code,)* }
        impl $name {
            pub fn from_stored(code: i32) -> Self {
                match code { $($code => Self::$variant,)* _ => Self::$first }
            }
        }
    };
}
option_enum!(InsertOrder, Sequential, Random = 1);
option_enum!(LeechAction, Suspend, TagOnly = 1);
option_enum!(ReviewMix, Mixed, AfterReviews = 1, BeforeReviews = 2);
option_enum!(
    NewSortOrder,
    CardTypeThenGathered,
    Gathered = 1,
    CardTypeThenRandom = 2,
    RandomNoteThenCardType = 3,
    Random = 4
);
option_enum!(
    ReviewOrder,
    DueThenRandom,
    DueThenDeck = 1,
    DeckThenDue = 2,
    IntervalAscending = 3,
    IntervalDescending = 4,
    EaseAscending = 5,
    EaseDescending = 6,
    RetrievabilityAscending = 7,
    Random = 8,
    Added = 9,
    ReverseAdded = 10,
    RetrievabilityDescending = 11,
    RelativeOverdueness = 12
);
option_enum!(
    GatherPriority,
    Deck,
    PositionAscending = 1,
    PositionDescending = 2,
    RandomNotes = 3,
    RandomCards = 4,
    DeckThenRandomNotes = 5
);
option_enum!(QuestionAction, ShowAnswer, ShowReminder = 1);
option_enum!(AnswerAction, Bury, Again = 1, Good = 2, Hard = 3, ShowReminder = 4);

#[derive(Clone, Debug)]
pub struct PresetView {
    source: DeckConfig,
    fields: DeckConfigMessage,
    later_parameters: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadOnlyPresetFields;
impl std::fmt::Display for ReadOnlyPresetFields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("older FSRS lists and opaque preset fields are read-only")
    }
}
impl std::error::Error for ReadOnlyPresetFields {}

impl PresetView {
    /// V1: None is a damaged/missing home deck, never a filtered preset.
    /// A missing preset uses factory values, not the user's edited preset 1.
    pub fn for_card(rows: &impl ReadRows, card: &Card) -> Result<Option<Self>, StorageError> {
        let Some(home) = rows.deck(super::card_sched::CardSched::new(card).home_deck())? else {
            return Ok(None);
        };
        let DeckKind::Normal(normal) = &home.kind.kind else {
            return Ok(None);
        };
        let source = rows
            .deck_config(DeckConfigId(normal.config_id))?
            .unwrap_or_else(|| DeckConfig {
                id: DeckConfigId(normal.config_id),
                name: String::new(),
                mtime: Default::default(),
                usn: Default::default(),
                config: ProtoBlob::new(DeckConfigMessage::builtin_defaults()),
            });
        Ok(Some(Self::new(&source)))
    }
    pub fn new(source: &DeckConfig) -> Self {
        Self {
            source: source.clone(),
            fields: source.config.for_use(),
            later_parameters: source.config.has_later_fsrs_parameters(),
        }
    }
    /// Bounded numeric values; enum storage codes remain intact (V4).
    pub fn fields(&self) -> &DeckConfigMessage {
        &self.fields
    }
    pub fn source(&self) -> &DeckConfig {
        &self.source
    }
    pub fn has_later_fsrs_parameters(&self) -> bool {
        self.later_parameters
    }
    pub fn insert_order(&self) -> InsertOrder {
        InsertOrder::from_stored(self.fields.new_card_insert_order)
    }
    pub fn leech_action(&self) -> LeechAction {
        LeechAction::from_stored(self.fields.leech_action)
    }
    pub fn new_mix(&self) -> ReviewMix {
        ReviewMix::from_stored(self.fields.new_mix)
    }
    pub fn interday_learning_mix(&self) -> ReviewMix {
        ReviewMix::from_stored(self.fields.interday_learning_mix)
    }
    pub fn new_sort_order(&self) -> NewSortOrder {
        NewSortOrder::from_stored(self.fields.new_card_sort_order)
    }
    pub fn review_order(&self) -> ReviewOrder {
        ReviewOrder::from_stored(self.fields.review_order)
    }
    pub fn gather_priority(&self) -> GatherPriority {
        GatherPriority::from_stored(self.fields.new_card_gather_priority)
    }
    pub fn question_action(&self) -> QuestionAction {
        QuestionAction::from_stored(self.fields.question_action)
    }
    pub fn answer_action(&self) -> AnswerAction {
        AnswerAction::from_stored(self.fields.answer_action)
    }
    pub fn parameters(&self) -> &[f32] {
        [
            self.fields.fsrs_params_6(),
            self.fields.fsrs_params_5(),
            self.fields.fsrs_params_4(),
        ]
        .into_iter()
        .find(|p| !p.is_empty())
        .unwrap_or(&[])
    }
    pub fn decay(&self) -> f32 {
        let params = self.parameters();
        if params.is_empty() {
            0.1542
        } else {
            params.get(20).copied().unwrap_or(0.5)
        }
    }
    pub fn allows_short_term(&self) -> bool {
        let params = self.parameters();
        params.is_empty() || (params.len() >= 19 && params[17] > 0.0 && params[18] > 0.0)
    }
    /// V8: compare with the read view first, so an unchanged edit never
    /// repairs invalid stored values or rewrites their original bytes.
    pub fn edit(&self, edit: impl FnOnce(&mut DeckConfigMessage)) -> Result<Option<DeckConfig>, ReadOnlyPresetFields> {
        let mut fields = self.fields.clone();
        edit(&mut fields);
        if !fields.same_read_only_fields(&self.fields) {
            return Err(ReadOnlyPresetFields);
        }
        if fields.same_content(&self.fields) {
            return Ok(None);
        }
        let mut out = self.source.clone();
        out.config = ProtoBlob::new(fields);
        Ok(Some(out))
    }
}
