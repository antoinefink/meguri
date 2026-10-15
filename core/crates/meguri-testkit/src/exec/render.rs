//! The render area's observation kinds. `template` (render-ops.md Q1)
//! evaluates meguri-template on given template texts for a note not yet
//! added; it never reads or changes the scenario's collection. Card cases
//! need notes and cards in Meguri's collection (U27, U29) and are refused
//! until then; so are cloze note types and the cloze filters, whose
//! behavior is U61's.

use meguri_template::{CardSpecials, FieldMap, NodeKind, ParsedTemplate, Side, TemplateError};
use serde_json::{Value, json};

use super::{ExecError, Registry, Session};
use crate::scenario::{
    NewNoteCase, NotetypeKind, ObservationArgs, ObservationRequest, TemplateCase, TemplateNotetype, TemplateRequest,
};

/// Registers the `template` kind.
pub(super) fn register(registry: &mut Registry) {
    registry.observation("template", observe);
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
    if notetype.kind == NotetypeKind::Cloze {
        return Err(not_yet(
            "template: the blank check of cloze note types is U61's (specs/render/cloze.md)",
        ));
    }
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
    let front = parse(&case.front)?;
    let back = parse(&case.back)?;
    let question = match front.render(&fields, Side::Question) {
        Ok(text) => text,
        Err(error) => return Ok(failed(&error, "question")),
    };
    // T10: a blank question leaves the answer template unexamined.
    if front.is_blank_question(&fields) {
        return Ok(json!({"question": question, "answer": null, "empty": true}));
    }
    match back.render(&fields, Side::Answer { front_side: &question }) {
        Ok(answer) => Ok(json!({"question": question, "answer": answer, "empty": false})),
        Err(error) => Ok(failed(&error, "answer")),
    }
}

/// The template, refused when it runs a cloze filter (F8, U61).
fn parse(text: &str) -> Result<ParsedTemplate, ExecError> {
    let parsed = ParsedTemplate::parse(text);
    let runs_cloze = parsed.nodes().iter().any(|node| match &node.kind {
        NodeKind::Replacement { filters, .. } => {
            filters.first().is_none_or(|outer| outer != "type")
                && filters.iter().any(|f| f == "cloze" || f == "cloze-only")
        }
        _ => false,
    });
    if runs_cloze {
        return Err(not_yet(format!(
            "template {text:?} uses a cloze filter, whose output is U61's (specs/render/cloze.md)"
        )));
    }
    Ok(parsed)
}

/// Q1.5: an error result, kind and side only.
fn failed(error: &TemplateError, side: &str) -> Value {
    json!({"error": {"kind": error.kind.name(), "side": side}})
}
