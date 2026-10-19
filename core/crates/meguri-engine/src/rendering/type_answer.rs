//! Resolve from the render snapshot's rows, never by taking another reader.
use crate::EngineError;
use meguri_model::{note::Note, notetype::Notetype};
use meguri_template::{
    ParsedTemplate,
    display::DisplaySides,
    media_display,
    type_answer::{self, Descriptor, Field, WarningForm},
};

pub(super) fn apply(
    mut sides: DisplaySides,
    template: &ParsedTemplate,
    note: &Note,
    kind: &Notetype,
    number: u32,
    observation: Option<&str>,
) -> Result<(DisplaySides, Option<Descriptor>), EngineError> {
    let fields: Vec<_> = kind
        .fields
        .iter()
        .zip(note.field_values())
        .map(|(f, value)| Field {
            name: &f.name,
            value,
            font: &f.config.font_name,
            size: f.config.font_size,
        })
        .collect();
    if observation.is_some() {
        sides.question = media_display::encode_for_display(&sides.question).into_owned();
        sides.answer = media_display::encode_for_display(&sides.answer).into_owned();
    }
    let (question, descriptor) = type_answer::question(
        &sides.question,
        template,
        &fields,
        number,
        if observation.is_some() {
            WarningForm::Token
        } else {
            WarningForm::Display
        },
        observation.is_none(),
    )
    .map_err(|_| EngineError::InvalidTypeAnswerReplacement)?;
    let answer = match (&descriptor, observation) {
        (Some(d), None) => type_answer::answer_with_comparison(&sides.answer, d, type_answer::ANSWER_SLOT),
        _ => type_answer::answer(&sides.answer, descriptor.as_ref(), observation.unwrap_or_default()),
    };
    Ok((DisplaySides { question, answer }, descriptor))
}
