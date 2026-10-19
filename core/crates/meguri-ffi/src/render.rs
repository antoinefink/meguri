//! Stored-card rendering on the interactive lane (render/card-render-pipeline K11).
use crate::{callback, error::CoreError, runtime};
use meguri_engine::rendering as engine;
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderMode {
    Review,
    Preview,
    Browser,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderPlatform {
    Mac,
    Ios,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderAv {
    Sound { name: String },
    Tts { language: String, content: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderSide {
    Question,
    Answer,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderBlank {
    EmptyFront,
    MissingCloze,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderErrorKind {
    UnclosedConditional,
    UnopenedConditional,
    MismatchedConditional,
    UnknownField,
    UnknownConditional,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RenderOutcome {
    Rendered,
    Blank { kind: RenderBlank },
    TemplateError { kind: RenderErrorKind, side: RenderSide },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RenderedCard {
    pub card_id: i64,
    pub ordinal: u32,
    pub mode: RenderMode,
    pub question: Option<String>,
    pub answer: Option<String>,
    pub question_av: Vec<RenderAv>,
    pub answer_av: Vec<RenderAv>,
    pub outcome: RenderOutcome,
    pub css: String,
    pub hide_play_buttons: bool,
    pub review_extra: String,
    pub flag: u8,
    pub marked: bool,
    pub type_in: Option<TypeAnswerDescriptor>,
}
impl From<RenderMode> for engine::RenderMode {
    fn from(mode: RenderMode) -> Self {
        match mode {
            RenderMode::Review => Self::Review,
            RenderMode::Preview => Self::Preview,
            RenderMode::Browser => Self::Browser,
        }
    }
}
impl From<engine::RenderedCard> for RenderedCard {
    fn from(card: engine::RenderedCard) -> Self {
        let av = |tags: Vec<engine::AvTag>| {
            tags.into_iter()
                .map(|tag| match tag {
                    engine::AvTag::Sound { name } => RenderAv::Sound { name },
                    engine::AvTag::Tts { language, content } => RenderAv::Tts { language, content },
                })
                .collect()
        };
        let outcome = match card.comparable.outcome {
            engine::Outcome::Rendered => RenderOutcome::Rendered,
            engine::Outcome::Blank(kind) => RenderOutcome::Blank {
                kind: match kind {
                    engine::BlankKind::EmptyFront => RenderBlank::EmptyFront,
                    engine::BlankKind::MissingCloze => RenderBlank::MissingCloze,
                },
            },
            engine::Outcome::Error { side, error } => {
                use meguri_template::ErrorKind as E;
                let kind = match error.kind {
                    E::UnclosedConditional => RenderErrorKind::UnclosedConditional,
                    E::UnopenedConditional => RenderErrorKind::UnopenedConditional,
                    E::MismatchedConditional => RenderErrorKind::MismatchedConditional,
                    E::UnknownField => RenderErrorKind::UnknownField,
                    E::UnknownConditional => RenderErrorKind::UnknownConditional,
                };
                let side = match side {
                    meguri_template::av_tags::AvSide::Question => RenderSide::Question,
                    meguri_template::av_tags::AvSide::Answer => RenderSide::Answer,
                };
                RenderOutcome::TemplateError { kind, side }
            }
        };
        Self {
            card_id: card.card_id.0,
            ordinal: card.ordinal,
            mode: match card.mode {
                engine::RenderMode::Review => RenderMode::Review,
                engine::RenderMode::Preview => RenderMode::Preview,
                engine::RenderMode::Browser => RenderMode::Browser,
            },
            question: card.question,
            answer: card.answer,
            question_av: av(card.comparable.question_av),
            answer_av: av(card.comparable.answer_av),
            outcome,
            css: card.css,
            hide_play_buttons: card.hide_play_buttons,
            review_extra: card.review_extra,
            flag: card.flag,
            marked: card.marked,
            type_in: card.type_in.map(Into::into),
        }
    }
}
#[uniffi::export]
pub fn render_card(card_id: i64, mode: RenderMode, platform: RenderPlatform) -> Result<RenderedCard, CoreError> {
    callback::enter("render_card");
    let collection = runtime::engine()?.collection()?;
    let platform = match platform {
        RenderPlatform::Mac => engine::Platform::Mac,
        RenderPlatform::Ios => engine::Platform::Ios,
    };
    Ok(engine::render_card(&collection, meguri_model::ids::CardId(card_id), mode.into(), platform)?.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TypeAnswerMode {
    Plain,
    Nc,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TypeAnswerDescriptor {
    pub expected: String,
    pub mode: TypeAnswerMode,
    pub case_insensitive: bool,
    pub font: String,
    pub size: u32,
}
impl From<meguri_template::type_answer::Descriptor> for TypeAnswerDescriptor {
    fn from(d: meguri_template::type_answer::Descriptor) -> Self {
        Self {
            expected: d.expected,
            mode: match d.mode {
                meguri_template::type_answer::Mode::Plain => TypeAnswerMode::Plain,
                meguri_template::type_answer::Mode::Nc => TypeAnswerMode::Nc,
            },
            case_insensitive: d.case_insensitive,
            font: d.font,
            size: d.size,
        }
    }
}
impl From<TypeAnswerDescriptor> for meguri_template::type_answer::Descriptor {
    fn from(d: TypeAnswerDescriptor) -> Self {
        Self {
            expected: d.expected,
            mode: match d.mode {
                TypeAnswerMode::Plain => meguri_template::type_answer::Mode::Plain,
                TypeAnswerMode::Nc => meguri_template::type_answer::Mode::Nc,
            },
            case_insensitive: d.case_insensitive,
            font: d.font,
            size: d.size,
        }
    }
}
/// Pure comparison C; runs without an open collection. The pre-rendered
/// answer already has its font and answer-rule wrappers.
#[uniffi::export]
pub fn compare_typed_answer(descriptor: TypeAnswerDescriptor, typed: String) -> Result<String, CoreError> {
    callback::enter("compare_typed_answer");
    Ok(meguri_template::type_answer::compare_typed_answer(
        &descriptor.into(),
        &typed,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comparison_export_works_without_runtime_or_collection() {
        let descriptor = TypeAnswerDescriptor {
            expected: "ka".into(),
            mode: TypeAnswerMode::Plain,
            case_insensitive: false,
            font: "Arial".into(),
            size: 20,
        };
        assert_eq!(
            compare_typed_answer(descriptor.clone(), "kq".into()).unwrap(),
            "<code id=typeans><span class=typeGood>k</span><span class=typeBad>q</span><br><span id=typearrow>&darr;</span><br><span class=typeGood>k</span><span class=typeMissed>a</span></code>"
        );
        let roundtrip: TypeAnswerDescriptor = meguri_template::type_answer::Descriptor::from(descriptor.clone()).into();
        assert_eq!(roundtrip, descriptor);
        let nc = TypeAnswerDescriptor {
            expected: "Qëvax".into(),
            mode: TypeAnswerMode::Nc,
            case_insensitive: true,
            ..descriptor
        };
        assert_eq!(
            compare_typed_answer(nc, "QEVAX".into()).unwrap(),
            "<code id=typeans><span class=typeGood>Qëvax</span></code>"
        );
    }
    #[test]
    fn replacement_refusal_roundtrips_as_a_typed_error() {
        let error = CoreError::from(meguri_engine::EngineError::InvalidTypeAnswerReplacement);
        assert_eq!(error, CoreError::InvalidTypeAnswerReplacement);
        assert_eq!(
            meguri_engine::EngineError::from(error),
            meguri_engine::EngineError::InvalidTypeAnswerReplacement
        );
    }
}
