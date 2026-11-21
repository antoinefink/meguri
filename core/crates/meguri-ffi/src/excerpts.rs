//! One blocking query-lane export; one typed entry per requested signed id.
use crate::{
    callback,
    error::CoreError,
    render::{RenderBlank, RenderErrorKind, RenderSide},
    runtime,
};
use meguri_engine::browse::{self, Question};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ExcerptQuestion {
    Text { text: String },
    MissingCloze { kind: RenderBlank, text: String },
    TemplateError { kind: RenderErrorKind, side: RenderSide },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CardExcerpt {
    Found {
        card_id: i64,
        question: ExcerptQuestion,
        deck: String,
    },
    Missing {
        card_id: i64,
    },
    Damaged {
        card_id: i64,
    },
}
impl From<browse::CardExcerpt> for CardExcerpt {
    fn from(value: browse::CardExcerpt) -> Self {
        match value {
            browse::CardExcerpt::Missing { card_id } => Self::Missing { card_id: card_id.0 },
            browse::CardExcerpt::Damaged { card_id } => Self::Damaged { card_id: card_id.0 },
            browse::CardExcerpt::Found {
                card_id,
                question,
                deck,
            } => Self::Found {
                card_id: card_id.0,
                deck,
                question: question.into(),
            },
        }
    }
}

/// The Question cell (`browser/columns` C10), of an excerpt or of a Browse
/// row (`browse_search`).
impl From<Question> for ExcerptQuestion {
    fn from(question: Question) -> Self {
        match question {
            Question::Text(text) => ExcerptQuestion::Text { text },
            Question::MissingCloze { kind, text } => ExcerptQuestion::MissingCloze {
                kind: blank(kind),
                text,
            },
            Question::TemplateError { kind, side } => ExcerptQuestion::TemplateError {
                kind: kind.into(),
                side: side.into(),
            },
        }
    }
}

pub(crate) fn blank(kind: meguri_engine::rendering::BlankKind) -> RenderBlank {
    match kind {
        meguri_engine::rendering::BlankKind::MissingCloze => RenderBlank::MissingCloze,
        meguri_engine::rendering::BlankKind::EmptyFront => RenderBlank::EmptyFront,
    }
}
#[uniffi::export]
pub fn card_excerpts(card_ids: Vec<i64>) -> Result<Vec<CardExcerpt>, CoreError> {
    callback::enter("card_excerpts");
    let collection = runtime::engine()?.collection()?;
    let ids = card_ids.into_iter().map(meguri_model::ids::CardId).collect::<Vec<_>>();
    Ok(browse::card_excerpts(&collection, &ids)?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::ids::CardId;
    #[test]
    fn typed_results_preserve_signed_ids_empty_text_and_error_side() {
        assert_eq!(
            CardExcerpt::from(browse::CardExcerpt::Missing {
                card_id: CardId(i64::MIN)
            }),
            CardExcerpt::Missing { card_id: i64::MIN }
        );
        assert_eq!(
            CardExcerpt::from(browse::CardExcerpt::Damaged {
                card_id: CardId(i64::MAX)
            }),
            CardExcerpt::Damaged { card_id: i64::MAX }
        );
        for question in [
            Question::Text(String::new()),
            Question::MissingCloze {
                kind: meguri_engine::rendering::BlankKind::MissingCloze,
                text: "kept".into(),
            },
            Question::TemplateError {
                kind: meguri_template::ErrorKind::UnknownField,
                side: meguri_template::av_tags::AvSide::Answer,
            },
        ] {
            let expected = match &question {
                Question::Text(_) => ExcerptQuestion::Text { text: String::new() },
                Question::MissingCloze { .. } => ExcerptQuestion::MissingCloze {
                    kind: RenderBlank::MissingCloze,
                    text: "kept".into(),
                },
                Question::TemplateError { .. } => ExcerptQuestion::TemplateError {
                    kind: RenderErrorKind::UnknownField,
                    side: RenderSide::Answer,
                },
            };
            assert_eq!(
                CardExcerpt::from(browse::CardExcerpt::Found {
                    card_id: CardId(-1),
                    question,
                    deck: "Full::Deck".into()
                }),
                CardExcerpt::Found {
                    card_id: -1,
                    question: expected,
                    deck: "Full::Deck".into()
                }
            );
        }
    }
}
