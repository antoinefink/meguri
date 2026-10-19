//! Read-only composition of two parsed sides (render/card-render-pipeline K4/K5).
use crate::{
    FieldMap, ParsedTemplate, Side, TemplateError,
    av_tags::{self, AvSide, AvTag, ExtractedSide},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RenderMode {
    #[default]
    Review,
    Preview,
    Browser,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlankKind {
    EmptyFront,
    MissingCloze,
}
impl BlankKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::EmptyFront => "empty_front",
            Self::MissingCloze => "missing_cloze",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Rendered,
    Blank(BlankKind),
    Error { side: AvSide, error: TemplateError },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardRender {
    pub question: Option<String>,
    pub answer: Option<String>,
    pub question_av: Vec<AvTag>,
    pub answer_av: Vec<AvTag>,
    pub outcome: Outcome,
}
impl CardRender {
    fn error(side: AvSide, error: TemplateError) -> Self {
        Self {
            question: None,
            answer: None,
            question_av: Vec::new(),
            answer_av: Vec::new(),
            outcome: Outcome::Error { side, error },
        }
    }
}
fn extract(text: String, side: AvSide, mode: RenderMode, audio: bool) -> ExtractedSide {
    if mode == RenderMode::Browser || !audio {
        ExtractedSide { text, av: Vec::new() }
    } else {
        av_tags::extract(&text, side)
    }
}

pub fn render(
    question: &ParsedTemplate,
    answer: &ParsedTemplate,
    fields: &FieldMap,
    cloze: bool,
    mode: RenderMode,
) -> CardRender {
    compose(question, answer, fields, cloze, mode, true)
}

/// Q1.4: template preview, with no AV extraction and the raw FrontSide.
pub fn render_template(
    question: &ParsedTemplate,
    answer: &ParsedTemplate,
    fields: &FieldMap,
    cloze: bool,
) -> CardRender {
    compose(question, answer, fields, cloze, RenderMode::Review, false)
}

fn compose(
    question: &ParsedTemplate,
    answer: &ParsedTemplate,
    fields: &FieldMap,
    cloze: bool,
    mode: RenderMode,
    audio: bool,
) -> CardRender {
    let front = match question.render(fields, Side::Question) {
        Ok(text) => text,
        Err(error) => return CardRender::error(AvSide::Question, error),
    };
    let blank = if cloze {
        fields.is_blank_cloze_card().then_some(BlankKind::MissingCloze)
    } else {
        (mode != RenderMode::Browser && question.is_blank_question(fields)).then_some(BlankKind::EmptyFront)
    };
    let front = extract(front, AvSide::Question, mode, audio);
    if let Some(kind) = blank {
        return CardRender {
            question: Some(front.text),
            answer: None,
            question_av: front.av,
            answer_av: Vec::new(),
            outcome: Outcome::Blank(kind),
        };
    }
    let front_side = if mode == RenderMode::Browser { "" } else { &front.text };
    let back = match answer.render(fields, Side::Answer { front_side }) {
        Ok(text) => text,
        Err(error) => return CardRender::error(AvSide::Answer, error),
    };
    let back = extract(back, AvSide::Answer, mode, audio);
    CardRender {
        question: Some(front.text),
        answer: Some(back.text),
        question_av: front.av,
        answer_av: back.av,
        outcome: Outcome::Rendered,
    }
}
