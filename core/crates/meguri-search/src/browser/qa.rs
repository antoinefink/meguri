//! The sole Question-cell path; U84 also uses this for rows.
use meguri_template::{
    ErrorKind,
    av_tags::{self, AvSide},
    card::{BlankKind, CardRender, Outcome},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    Text(String),
    MissingCloze { kind: BlankKind, text: String },
    TemplateError { kind: ErrorKind, side: AvSide },
}

pub fn question(rendered: &CardRender) -> Question {
    if let Outcome::Error { side, error } = &rendered.outcome {
        return Question::TemplateError {
            kind: error.kind,
            side: *side,
        };
    }
    let text = meguri_text::html::text_line_keep_media(&av_tags::browser_display(
        rendered.question.as_deref().unwrap_or_default(),
    ));
    match rendered.outcome {
        Outcome::Blank(kind) => Question::MissingCloze { kind, text },
        _ => Question::Text(text),
    }
}
