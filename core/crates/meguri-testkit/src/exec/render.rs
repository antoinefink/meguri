//! Render observations use the stored-card engine (Q4/Q5) and template preview (Q1).
use crate::scenario::{LogicalRef, RefKind};
use meguri_engine::rendering::{self, AvTag, CardRender, Outcome, Platform, RenderMode};
use meguri_model::ids::CardId;
use meguri_storage::{ReadRows, ReaderRole};
use meguri_template::cloze;
use meguri_template::{CardSpecials, FieldMap, ParsedTemplate, Side, TemplateError};
use serde_json::{Map, Value, json};

use super::{ExecError, Registry, Session};
use crate::scenario::{
    ClozeFunction, ClozeInputs, ClozeRequest, NewNoteCase, NotetypeKind, ObservationArgs, ObservationRequest,
    TemplateCase, TemplateNotetype, TemplateRequest,
};

/// Registers template, cloze and stored-card pipeline observations.
pub(super) fn register(registry: &mut Registry) {
    registry.observation("template", observe);
    registry.observation("cloze", observe_cloze);
    registry.observation_part("template.card");
    registry.observation("render", observe_render);
    registry.observation("render_status", observe_status);
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}

fn observe(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("observation template takes {notetype?, cases}"));
    };
    let request = TemplateRequest::from_mapping(map).map_err(|e| invalid(e.to_string()))?;
    let results = request
        .cases
        .iter()
        .map(|case| match (case, &request.notetype) {
            (TemplateCase::NewNote(case), Some(notetype)) => new_note(notetype, case),
            (TemplateCase::NewNote(_), None) => Err(invalid("a new-note case needs notetype")),
            (TemplateCase::Card { card, front, back }, _) => {
                let collection = session.engine_collection()?;
                let id = *session.cards.get(card.0.index()).ok_or_else(|| ExecError::new("unknown_ref", card.0.to_string()))?;
                let rendered = rendering::render_template(&collection, CardId(id), front, back).map_err(core)?;
                match &rendered.outcome {
                    Outcome::Error { side, error } => Ok(failed(error, side_name(*side))),
                    _ => Ok(json!({"question": rendered.question, "answer": rendered.answer, "empty": matches!(rendered.outcome, Outcome::Blank(_))})),
                }
            },
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(results))
}

/// Q1.4 for a note and card not yet added (Q1.2).
fn new_note(notetype: &TemplateNotetype, case: &NewNoteCase) -> Result<Value, ExecError> {
    let default_name;
    let template_name = match &case.name {
        Some(name) => name.as_str(),
        None => {
            default_name = format!("Card {}", u64::from(case.ord) + 1);
            &default_name
        }
    };
    let specials = CardSpecials {
        ord: case.ord,
        notetype_name: &notetype.name,
        template_name,
        deck_name: None,
        flags: 0,
        card_id: 0,
        tags: &case.tags,
    };
    let note_fields = notetype
        .fields
        .iter()
        .zip(&case.fields)
        .map(|(name, value)| (name.as_str(), value.as_str()));
    let fields = FieldMap::new(note_fields, &specials);
    let front = ParsedTemplate::parse(&case.front);
    let back = ParsedTemplate::parse(&case.back);
    let question = match front.render(&fields, Side::Question) {
        Ok(text) => text,
        Err(error) => return Ok(failed(&error, "question")),
    };
    // T10: a blank question leaves the answer template unexamined. A cloze
    // note's card is blank when no value of its field map holds its cloze
    // number (render/cloze#C15).
    let blank = match notetype.kind {
        NotetypeKind::Standard => front.is_blank_question(&fields),
        NotetypeKind::Cloze => fields.is_blank_cloze_card(),
    };
    if blank {
        return Ok(json!({"question": question, "answer": null, "empty": true}));
    }
    match back.render(&fields, Side::Answer { front_side: &question }) {
        Ok(answer) => Ok(json!({"question": question, "answer": answer, "empty": false})),
        Err(error) => Ok(failed(&error, "answer")),
    }
}

/// Q1.5: an error result, kind and side only.
fn failed(error: &TemplateError, side: &str) -> Value {
    json!({"error": {"kind": error.kind.name(), "side": side}})
}

/// Z1: every requested function on every input, in input order (Z2).
fn observe_cloze(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("observation cloze takes {fns, inputs}"));
    };
    let request = ClozeRequest::from_mapping(map).map_err(|e| invalid(e.to_string()))?;
    let mut value = Map::new();
    for &function in &request.fns {
        let outputs = match &request.inputs {
            ClozeInputs::Texts(texts) if function == ClozeFunction::Numbers => texts
                .iter()
                .map(|text| json!(cloze::numbers(text).into_iter().collect::<Vec<u16>>()))
                .collect(),
            ClozeInputs::Texts(_) => return Err(invalid(format!("{} takes [text, card] pairs", function.name()))),
            ClozeInputs::Cards(pairs) => pairs
                .iter()
                .map(|(text, card)| cloze_output(function, text, u32::from(*card)))
                .collect::<Result<Vec<_>, _>>()?,
        };
        value.insert(function.name().to_owned(), Value::Array(outputs));
    }
    Ok(Value::Object(value))
}

/// Z2 for the functions that take `[text, card]` pairs.
fn cloze_output(function: ClozeFunction, text: &str, card: u32) -> Result<Value, ExecError> {
    Ok(json!(match function {
        ClozeFunction::Front => cloze::render(text, card, cloze::Side::Front),
        ClozeFunction::Back => cloze::render(text, card, cloze::Side::Back),
        ClozeFunction::OnlyFront => cloze::render_only(text, card, cloze::Side::Front),
        ClozeFunction::OnlyBack => cloze::render_only(text, card, cloze::Side::Back),
        ClozeFunction::Typing => cloze::type_answer(text, card),
        ClozeFunction::Numbers => return Err(invalid("numbers takes strings")),
    }))
}

fn side_name(side: meguri_template::av_tags::AvSide) -> &'static str {
    match side {
        meguri_template::av_tags::AvSide::Question => "question",
        meguri_template::av_tags::AvSide::Answer => "answer",
    }
}
fn references(s: &Session<'_>, request: &ObservationRequest) -> Vec<LogicalRef> {
    match &request.args {
        ObservationArgs::Refs(refs) => refs.clone(),
        _ => (0..s.cards.len())
            .map(|i| LogicalRef {
                kind: RefKind::Card,
                n: i as u32 + 1,
            })
            .collect(),
    }
}
fn comparable(result: &CardRender, browser: bool) -> Value {
    if let Outcome::Error { side, error } = &result.outcome {
        return failed(error, side_name(*side));
    }
    let mut value = json!({"question": result.question, "answer": result.answer, "blank": match result.outcome { Outcome::Blank(k) => Some(k.name()), _ => None }});
    if !browser {
        let av = |tags: &[AvTag]| {
            tags.iter()
                .map(|t| match t {
                    AvTag::Sound { name } => json!({"sound": name}),
                    AvTag::Tts { language, .. } => json!({"tts": language}),
                })
                .collect::<Vec<_>>()
        };
        value["question_av"] = json!(av(&result.question_av));
        value["answer_av"] = json!(av(&result.answer_av));
    }
    value
}
fn observe_render(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    stored(s, request, false)
}
fn observe_status(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    stored(s, request, true)
}
fn stored(s: &mut Session<'_>, request: &ObservationRequest, status: bool) -> Result<Value, ExecError> {
    let collection = s.engine_collection()?;
    let mut out = Map::new();
    for reference in references(s, request) {
        let id = CardId(
            *s.cards
                .get(reference.index())
                .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))?,
        );
        let exists = collection
            .reader(ReaderRole::Query)
            .map_err(core)?
            .card(id)
            .map_err(core)?
            .is_some();
        let value = if !exists {
            Value::Null
        } else {
            let review = rendering::render_card(&collection, id, RenderMode::Review, Platform::Mac).map_err(core)?;
            if status {
                json!({"ord": review.ordinal, "status": match review.comparable.outcome { Outcome::Rendered => "ok", Outcome::Blank(_) => "empty", Outcome::Error{..} => "template_error" }})
            } else {
                let browser =
                    rendering::render_card(&collection, id, RenderMode::Browser, Platform::Mac).map_err(core)?;
                json!({"review": comparable(&review.comparable, false), "browser": comparable(&browser.comparable, true)})
            }
        };
        out.insert(reference.to_string(), value);
    }
    Ok(out.into())
}
