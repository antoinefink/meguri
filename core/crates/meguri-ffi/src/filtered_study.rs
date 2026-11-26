//! Generation-bound filtered management and Custom Study. The job result
//! retains the exact commit notice even when a native listener attaches late.
use crate::{CoreError, callback, runtime, mutations::MutationSummary, jobs::{JobHandle, wrap}};
use meguri_engine::{Generation, decks::{filtered as engine, custom_study as custom}};
use meguri_model::ids::DeckId;
use meguri_storage::ReaderRole;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FilteredStudyTerm { pub search: String, pub limit: u32, pub order: u32 }
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FilteredStudyDefinition {
    pub name: String, pub terms: Vec<FilteredStudyTerm>, pub reschedule: bool,
    pub again_seconds: u32, pub hard_seconds: u32, pub good_seconds: u32,
}
impl From<engine::Definition> for FilteredStudyDefinition {
    fn from(d: engine::Definition) -> Self { Self { name: d.name,
        terms: d.terms.into_iter().map(|t| FilteredStudyTerm { search: t.search, limit: t.limit, order: t.order }).collect(),
        reschedule: d.reschedule, again_seconds: d.again_seconds, hard_seconds: d.hard_seconds, good_seconds: d.good_seconds } }
}
impl From<FilteredStudyDefinition> for engine::Definition {
    fn from(d: FilteredStudyDefinition) -> Self { Self { name: d.name,
        terms: d.terms.into_iter().map(|t| engine::Term { search: t.search, limit: t.limit, order: t.order }).collect(),
        reschedule: d.reschedule, again_seconds: d.again_seconds, hard_seconds: d.hard_seconds, good_seconds: d.good_seconds } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CustomStudyMode { IncreaseNew, IncreaseReview, Forgotten, Ahead, PreviewNew, ByState }
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CustomStudyState { New, Due, Review, All }
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CustomStudyRequest {
    pub mode: CustomStudyMode, pub amount: i32, pub state: CustomStudyState,
    pub include_tags: Vec<String>, pub exclude_tags: Vec<String>,
}
impl From<CustomStudyRequest> for custom::Request {
    fn from(r: CustomStudyRequest) -> Self { Self {
        mode: match r.mode { CustomStudyMode::IncreaseNew => custom::Mode::IncreaseNew,
            CustomStudyMode::IncreaseReview => custom::Mode::IncreaseReview, CustomStudyMode::Forgotten => custom::Mode::Forgotten,
            CustomStudyMode::Ahead => custom::Mode::Ahead, CustomStudyMode::PreviewNew => custom::Mode::PreviewNew,
            CustomStudyMode::ByState => custom::Mode::ByState },
        state: match r.state { CustomStudyState::New => custom::State::New, CustomStudyState::Due => custom::State::Due,
            CustomStudyState::Review => custom::State::Review, CustomStudyState::All => custom::State::All },
        amount: r.amount, include_tags: r.include_tags, exclude_tags: r.exclude_tags } }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CustomStudyDefaults {
    pub available_new: u32, pub available_review: u32, pub extend_new: i32, pub extend_review: i32,
    pub include_tags: Vec<String>, pub exclude_tags: Vec<String>,
}
#[uniffi::export]
pub fn filtered_study_definition(deck: i64, generation: u64) -> Result<FilteredStudyDefinition, CoreError> {
    callback::enter("filtered_study_definition");
    let c = runtime::host()?.collection_for_generation(Generation(generation))?;
    let reader = c.reader(ReaderRole::Query)?;
    let snapshot = reader.snapshot()?;
    Ok(engine::definition(&*snapshot, DeckId(deck))?.into())
}
#[uniffi::export]
pub fn custom_study_defaults(deck: i64, generation: u64) -> Result<CustomStudyDefaults, CoreError> {
    callback::enter("custom_study_defaults");
    let c = runtime::host()?.collection_for_generation(Generation(generation))?;
    let d = custom::defaults(&c, DeckId(deck))?;
    Ok(CustomStudyDefaults { available_new: d.available_new, available_review: d.available_review,
        extend_new: d.extend_new, extend_review: d.extend_review, include_tags: d.include_tags, exclude_tags: d.exclude_tags })
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum FilteredStudyCommand {
    Save { deck: Option<i64>, definition: FilteredStudyDefinition, allow_empty: bool },
    Rebuild { deck: i64 }, Empty { deck: i64 }, Custom { deck: i64, request: CustomStudyRequest },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FilteredStudyMutation { pub summary: MutationSummary, pub deck: i64 }
#[derive(Debug, uniffi::Object)]
pub struct FilteredStudyJob { result: Arc<Mutex<Option<FilteredStudyMutation>>> }
#[uniffi::export]
impl FilteredStudyJob {
    pub fn result(&self) -> Result<Option<FilteredStudyMutation>, CoreError> {
        callback::enter("FilteredStudyJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct FilteredStudyJobStart { pub handle: JobHandle, pub result: Arc<FilteredStudyJob> }
#[uniffi::export]
pub fn start_filtered_study(command: FilteredStudyCommand, generation: u64) -> Result<FilteredStudyJobStart, CoreError> {
    callback::enter("start_filtered_study");
    let c = runtime::host()?.collection_for_generation(Generation(generation))?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        if j.cancel.is_cancelled() { return Err(meguri_engine::EngineError::Cancelled); }
        let mutation = match command {
            FilteredStudyCommand::Save { deck, definition, allow_empty } => {
                let r = engine::save_controlled(&c, deck.map(DeckId), definition.into(), allow_empty, j.cancel.clone())?;
                FilteredStudyMutation { summary: MutationSummary::of(&r), deck: r.value.0 }
            },
            FilteredStudyCommand::Rebuild { deck } => {
                let r = engine::rebuild_controlled(&c, DeckId(deck), j.cancel.clone())?;
                FilteredStudyMutation { summary: MutationSummary::of(&r), deck }
            },
            FilteredStudyCommand::Empty { deck } => {
                let r = engine::empty_controlled(&c, DeckId(deck), j.cancel.clone())?;
                FilteredStudyMutation { summary: MutationSummary::of(&r), deck }
            },
            FilteredStudyCommand::Custom { deck, request } => {
                let r = custom::custom_controlled(&c, DeckId(deck), request.into(), j.cancel.clone())?;
                FilteredStudyMutation { summary: MutationSummary::of(&r), deck: r.value.0 }
            },
        };
        *out.lock().unwrap() = Some(mutation);
        Ok(())
    })?;
    Ok(FilteredStudyJobStart { handle: wrap(handle), result: Arc::new(FilteredStudyJob { result }) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_definition_and_all_request_variants_map_without_losing_fields() {
        let d = engine::Definition { name: "Selection".into(), terms: vec![engine::Term { search: "is:due".into(), limit: 100, order: 10 },
            engine::Term { search: "is:new".into(), limit: 20, order: 5 }], reschedule: false,
            again_seconds: 30, hard_seconds: 120, good_seconds: 3 };
        assert_eq!(engine::Definition::from(FilteredStudyDefinition::from(d.clone())),d);
        for (mode, expected) in [(CustomStudyMode::IncreaseNew,custom::Mode::IncreaseNew),
            (CustomStudyMode::IncreaseReview,custom::Mode::IncreaseReview), (CustomStudyMode::Forgotten,custom::Mode::Forgotten),
            (CustomStudyMode::Ahead,custom::Mode::Ahead), (CustomStudyMode::PreviewNew,custom::Mode::PreviewNew),
            (CustomStudyMode::ByState,custom::Mode::ByState)] {
            for (state, expected_state) in [(CustomStudyState::New,custom::State::New), (CustomStudyState::Due,custom::State::Due),
                (CustomStudyState::Review,custom::State::Review), (CustomStudyState::All,custom::State::All)] {
                let r = custom::Request::from(CustomStudyRequest { mode, state, amount: -2,
                    include_tags: vec!["tag*\"".into()], exclude_tags: vec!["skip:tag".into()] });
                assert_eq!((r.mode,r.state,r.amount),(expected,expected_state,-2));
                assert_eq!(r.include_tags,["tag*\""]); assert_eq!(r.exclude_tags,["skip:tag"]);
            }
        }
    }
}
