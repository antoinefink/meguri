//! The sole Question- and Answer-cell path (`browser/columns` C10), for
//! Browse rows and card excerpts.
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

/// The Answer cell: a blank card's has no text (C10 step 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    Text(String),
    MissingCloze { kind: BlankKind },
    TemplateError { kind: ErrorKind, side: AvSide },
}

/// C10 step 4: the answer's audio display form without the beginning that
/// repeats the question's (code point by code point), as one H6 line with
/// media names kept.
pub fn answer(rendered: &CardRender) -> Answer {
    match &rendered.outcome {
        Outcome::Error { side, error } => Answer::TemplateError {
            kind: error.kind,
            side: *side,
        },
        Outcome::Blank(kind) => Answer::MissingCloze { kind: *kind },
        _ => {
            let q = av_tags::browser_display(rendered.question.as_deref().unwrap_or_default());
            let a = av_tags::browser_display(rendered.answer.as_deref().unwrap_or_default());
            let rest = a.strip_prefix(q.as_str()).unwrap_or(&a);
            Answer::Text(meguri_text::html::text_line_keep_media(rest))
        }
    }
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
