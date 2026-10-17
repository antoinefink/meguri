//! The render area's observation kinds. `template` (render-ops.md Q1)
//! evaluates meguri-template on given template texts for a note not yet
//! added, and `cloze` (cloze-ops.md Z1) its cloze functions on given texts;
//! neither reads or changes the scenario's collection. Template card cases
//! need notes and cards in Meguri's collection (U27, U29); U62 adds them, and
//! they are refused until then.

use meguri_template::cloze;
use meguri_template::{CardSpecials, FieldMap, ParsedTemplate, Side, TemplateError};
use serde_json::{Map, Value, json};

use super::{ExecError, Registry, Session};
use crate::scenario::{
    ClozeFunction, ClozeInputs, ClozeRequest, NewNoteCase, NotetypeKind, ObservationArgs, ObservationRequest,
    TemplateCase, TemplateNotetype, TemplateRequest,
};

/// Registers the pure template, cloze and image-occlusion observations.
pub(super) fn register(registry: &mut Registry) {
    registry.observation("template", observe);
    registry.observation("cloze", observe_cloze);
    registry.observation("image_occlusion", observe_image_occlusion);
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn not_yet(detail: impl Into<String>) -> ExecError {
    ExecError::new("unsupported_observation", detail)
}

fn observe(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
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
            (TemplateCase::Card { card, .. }, _) => Err(not_yet(format!(
                "template card case {}: Meguri's executor has no notes or cards yet (U27, U29)",
                card.0
            ))),
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

/// J3: raw template reading and engine field roles, without a collection.
fn observe_image_occlusion(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use crate::scenario::ImageOcclusionRequest;
    use meguri_engine::image_occlusion::field_roles;
    use meguri_model::notetype::NotetypeField;
    use meguri_model::proto::{ProtoBlob, notetype::FieldConfig};
    use meguri_template::image_occlusion::parse_raw;
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("image_occlusion takes {fns, inputs}"));
    };
    match ImageOcclusionRequest::from_mapping(map).map_err(|e| invalid(e.to_string()))? {
        ImageOcclusionRequest::Parse(texts) => {
            let outputs: Vec<_> = texts
                .iter()
                .map(|text| {
                    let raw = parse_raw(text);
                    let numbers: Vec<_> = raw
                        .numbers
                        .into_iter()
                        .map(|n| {
                            let shapes: Vec<_> = n
                                .shapes
                                .into_iter()
                                .map(|s| json!({"shape":s.shape,"props":s.props}))
                                .collect();
                            json!({"number":n.number,"shapes":shapes})
                        })
                        .collect();
                    json!({"hide_all":raw.hide_all,"numbers":numbers})
                })
                .collect();
            Ok(json!({"parse":outputs}))
        }
        ImageOcclusionRequest::Fields(inputs) => {
            let outputs: Vec<_> = inputs
                .into_iter()
                .map(|tags| {
                    let fields: Vec<_> = tags
                        .into_iter()
                        .enumerate()
                        .map(|(i, tag)| {
                            let mut config = FieldConfig::default();
                            config.tag = tag;
                            NotetypeField {
                                ord: i as u32,
                                name: format!("F{i}"),
                                prior_ord: None,
                                config: ProtoBlob::new(config),
                            }
                        })
                        .collect();
                    field_roles(&fields)
                        .map(|r| {
                            json!({
                                "occlusions": r.occlusions,
                                "image": r.image,
                                "header": r.header,
                                "back_extra": r.back_extra,
                            })
                        })
                        .unwrap_or(Value::Null)
                })
                .collect();
            Ok(json!({"fields":outputs}))
        }
    }
}
