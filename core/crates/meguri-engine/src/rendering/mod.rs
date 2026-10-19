//! One read-only, snapshot-consistent stored-card render (render/card-render-pipeline).
mod cache;
mod special_fields;
mod type_answer;
use crate::{Collection, EngineError};
pub(crate) use cache::ParseCache;
use meguri_model::ids::CardId;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, Table};
pub use meguri_template::{
    av_tags::AvTag,
    card::{BlankKind, CardRender, Outcome, RenderMode},
    display::{Platform, RenderHost},
};
use meguri_template::{card, display};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedCard {
    pub card_id: CardId,
    pub ordinal: u32,
    pub mode: RenderMode,
    pub question: Option<String>,
    pub answer: Option<String>,
    /// K4/K5 before display, including the AV lists and typed outcome.
    pub comparable: CardRender,
    pub css: String,
    pub hide_play_buttons: bool,
    pub review_extra: String,
    pub flag: u8,
    pub marked: bool,
    pub type_in: Option<meguri_template::type_answer::Descriptor>,
}
fn missing(table: Table, key: impl ToString) -> EngineError {
    EngineError::NotFound {
        table,
        key: key.to_string(),
    }
}

pub fn render_card(
    collection: &Collection,
    id: CardId,
    mode: RenderMode,
    platform: Platform,
) -> Result<RenderedCard, EngineError> {
    render_card_inner(collection, id, mode, platform, None, false)
}

/// Raw K4/K5 output without display substitutions. In particular, malformed
/// imported fonts cannot change the comparable render observation boundary.
pub fn render_card_comparable(
    collection: &Collection,
    id: CardId,
    mode: RenderMode,
) -> Result<RenderedCard, EngineError> {
    render_card_inner(collection, id, mode, Platform::Mac, None, true)
}

/// K2 observation form of the same product pipeline, with warning tokens,
/// play markers and comparison C for the supplied text. No input attributes.
pub fn review_html(collection: &Collection, id: CardId, typed: &str) -> Result<display::DisplaySides, EngineError> {
    let result = render_card_inner(collection, id, RenderMode::Review, Platform::Mac, Some(typed), false)?;
    Ok(display::DisplaySides {
        question: result.question.unwrap_or_default(),
        answer: result.answer.unwrap_or_default(),
    })
}

fn render_card_inner(
    collection: &Collection,
    id: CardId,
    mode: RenderMode,
    platform: Platform,
    observation: Option<&str>,
    comparable_only: bool,
) -> Result<RenderedCard, EngineError> {
    let reader = collection.reader(ReaderRole::Interactive)?;
    let snapshot = reader.snapshot()?;
    let card = snapshot.card(id)?.ok_or_else(|| missing(Table::Cards, id.0))?;
    let note = snapshot
        .note(card.note_id)?
        .ok_or_else(|| missing(Table::Notes, card.note_id.0))?;
    let kind = snapshot
        .notetype(note.notetype_id)?
        .ok_or_else(|| missing(Table::Notetypes, note.notetype_id.0))?;
    let cloze = kind.config.kind == 1;
    let template = if cloze {
        kind.templates.first()
    } else {
        kind.templates.iter().find(|t| t.ord == u32::from(card.ord))
    }
    .ok_or_else(|| missing(Table::Templates, format!("{}:{}", kind.id.0, card.ord)))?;
    let fields = special_fields::build(&*snapshot, &card, &note, &kind, template)?;
    let q = if mode == RenderMode::Browser && !template.config.q_format_browser.is_empty() {
        &template.config.q_format_browser
    } else {
        &template.config.q_format
    };
    let a = if mode == RenderMode::Browser && !template.config.a_format_browser.is_empty() {
        &template.config.a_format_browser
    } else {
        &template.config.a_format
    };
    let (question, answer) = collection.core.render_cache.get(kind.id, template.ord, mode, q, a);
    let comparable = card::render(&question, &answer, &fields, cloze, mode);
    let hide_play_buttons = matches!(
        snapshot.config_value("hideAudioPlayButtons")?,
        Some(ConfigValue::Bool(true))
    );
    let review_extra = match snapshot.config_value("reviewExtra")? {
        Some(ConfigValue::String(s)) => s,
        _ => String::new(),
    };
    let (question, answer, type_in) = if mode == RenderMode::Browser || comparable_only {
        (comparable.question.clone(), comparable.answer.clone(), None)
    } else {
        let sides = if observation.is_some() {
            display::DisplaySides {
                question: format!(
                    "<style>{}</style>{}",
                    kind.config.css,
                    comparable.question.as_deref().unwrap_or_default()
                ),
                answer: format!(
                    "<style>{}</style>{}",
                    kind.config.css,
                    comparable.answer.as_deref().unwrap_or_default()
                ),
            }
        } else {
            display::compose(
                &comparable,
                &kind.config.css,
                u32::from(card.ord),
                RenderHost {
                    platform,
                    hide_play_buttons,
                },
            )
        };
        let (sides, descriptor) =
            type_answer::apply(sides, &question, &note, &kind, u32::from(card.ord) + 1, observation)?;
        (Some(sides.question), Some(sides.answer), descriptor)
    };
    Ok(RenderedCard {
        card_id: id,
        ordinal: u32::from(card.ord),
        mode,
        question,
        answer,
        comparable,
        css: kind.config.css.clone(),
        hide_play_buttons,
        review_extra,
        flag: card.flags & 7,
        marked: crate::tags::canonical::split_text(&note.tags).any(|t| t.to_lowercase() == "marked"),
        type_in,
    })
}

/// Collection-backed template preview (conformance/render-ops Q1.3/1.4).
/// Overrides do not save the note type, extract audio or generate media.
pub fn render_template(
    collection: &Collection,
    id: CardId,
    question: &str,
    answer: &str,
) -> Result<CardRender, EngineError> {
    let reader = collection.reader(ReaderRole::Interactive)?;
    let snapshot = reader.snapshot()?;
    let card = snapshot.card(id)?.ok_or_else(|| missing(Table::Cards, id.0))?;
    let note = snapshot
        .note(card.note_id)?
        .ok_or_else(|| missing(Table::Notes, card.note_id.0))?;
    let kind = snapshot
        .notetype(note.notetype_id)?
        .ok_or_else(|| missing(Table::Notetypes, note.notetype_id.0))?;
    let cloze = kind.config.kind == 1;
    let template = if cloze {
        kind.templates.first()
    } else {
        kind.templates.iter().find(|t| t.ord == u32::from(card.ord))
    }
    .ok_or_else(|| missing(Table::Templates, format!("{}:{}", kind.id.0, card.ord)))?;
    let fields = special_fields::build(&*snapshot, &card, &note, &kind, template)?;
    Ok(card::render_template(
        &meguri_template::ParsedTemplate::parse(question),
        &meguri_template::ParsedTemplate::parse(answer),
        &fields,
        cloze,
    ))
}
