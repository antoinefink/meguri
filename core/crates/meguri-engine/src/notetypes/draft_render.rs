//! The Cards tab's live preview (`notetypes/workspace` W9): one card of a
//! structure draft rendered as `render/preview` V2 to V11 say, for a stored
//! note of the type or a new empty one, and the type's note ids for the
//! preview's note menu. Both read one snapshot and write nothing (V10): no
//! row, no undo step, no change event.
//!
//! The draft is rendered as the save would store it (W7's rule for the
//! issues, `notetypes/structure-changes` S4 and S5): field renames and
//! removals rewrite the templates first, and a stored note's values follow
//! the draft's field origins. Values are used as stored, with no save
//! normalization (V5).

use meguri_model::ids::{NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, Table};
use meguri_template::display::{self, Platform, RenderHost};
use meguri_template::{CardSpecials, FieldMap, ParsedTemplate, card, cloze, type_answer};
use meguri_text::field_empty::counts_as_empty;

use super::NotetypeKind;
use super::structure::StructureDraft;
use super::structure::plan;
use crate::rendering::{RenderMode, RenderedCard};
use crate::{Collection, EngineError};

/// `render/preview` V7, exactly, in every interface language.
pub const SAMPLE_CLOZE: &str = "This is a {{c1::sample}} cloze deletion.";

/// The largest target (V2).
const MAX_TARGET: u32 = 65534;

/// One preview render and the note it rendered (V2, V11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftRender {
    pub card: RenderedCard,
    /// The stored note rendered; `None` for a new empty note.
    pub note: Option<NoteId>,
    /// A cloze draft's card list (V6): the cloze numbers of the note's
    /// values before filling, plus the target's number, ascending. Empty
    /// for a standard draft.
    pub cloze_numbers: Vec<u32>,
}

/// V2 for a structure draft: `target` is a template position of the draft
/// (standard) or a cloze number minus one (cloze); `note` a stored note of
/// the draft's note type, or `None` for a new note with every value empty
/// and no tags. A note that no longer exists, or belongs to another note
/// type, renders as that new note (V4's rule for a missing note).
/// Refuses as the draft's planning does (`InvalidArgument`, `Stale`,
/// `invalid_notetype`), a target past 65534 (`InvalidArgument`) and a
/// standard draft with no template at the target (`NotFound`).
pub fn render_draft_on(
    reader: &impl ReadRows,
    draft: &StructureDraft,
    note: Option<NoteId>,
    target: u32,
    fill_empty: bool,
    platform: Platform,
) -> Result<DraftRender, EngineError> {
    if target > MAX_TARGET {
        return Err(super::structure::invalid("the preview target is past 65534"));
    }
    let base = plan::checked_base(reader, draft)?;
    let layout = plan::layout_of_draft(reader, base, draft)?;
    let saved = &layout.saved;
    let is_cloze = NotetypeKind::of(saved) == NotetypeKind::Cloze;
    let template = if is_cloze { saved.templates.first() } else { saved.templates.get(target as usize) }
        .ok_or_else(|| EngineError::NotFound {
            table: Table::Templates,
            key: format!("{}:{target}", saved.id.0),
        })?;

    // V2 and V4: the stored note, when it is one of this type's.
    let stored = match note {
        Some(id) => reader.note(id)?.filter(|n| n.notetype_id == layout.base.id),
        None => None,
    };
    let stored_values: Vec<&str> = stored.as_ref().map(|n| n.field_values().collect()).unwrap_or_default();
    // S5: each draft field takes the stored value at its origin.
    let values: Vec<String> = draft
        .fields
        .iter()
        .map(|field| {
            field
                .origin
                .and_then(|o| stored_values.get(o as usize).copied())
                .unwrap_or("")
                .to_owned()
        })
        .collect();
    let tags: Vec<String> = stored
        .as_ref()
        .map(|n| crate::tags::canonical::split_text(&n.tags).map(str::to_owned).collect())
        .unwrap_or_default();
    let stored_card = match &stored {
        Some(n) => reader.cards_of_note(n.id)?.into_iter().find(|c| u32::from(c.ord) == target),
        None => None,
    };
    let deck = match &stored_card {
        Some(c) => {
            let home = if c.original_deck_id.0 != 0 { c.original_deck_id } else { c.deck_id };
            reader.deck(home)?.map(|d| d.human_name())
        }
        None => None,
    };

    let question = ParsedTemplate::parse(&template.config.q_format);
    let answer = ParsedTemplate::parse(&template.config.a_format);
    let filled = if fill_empty { fill(saved, &question, &values) } else { values.clone() };
    let fields = FieldMap::new(
        saved.fields.iter().map(|f| f.name.as_str()).zip(filled.iter().map(String::as_str)),
        &CardSpecials {
            ord: target,
            notetype_name: &saved.name,
            template_name: &template.name,
            deck_name: deck.as_deref(),
            flags: stored_card.as_ref().map_or(0, |c| i64::from(c.flags)),
            card_id: stored_card.as_ref().map_or(0, |c| c.id.0),
            tags: &tags,
        },
    );
    let comparable = card::render(&question, &answer, &fields, is_cloze, RenderMode::Preview);
    let hide_play_buttons = matches!(reader.config_value("hideAudioPlayButtons")?, Some(ConfigValue::Bool(true)));
    let review_extra = match reader.config_value("reviewExtra")? {
        Some(ConfigValue::String(s)) => s,
        _ => String::new(),
    };
    // V8: review's display steps with the draft's CSS, then V9's type-in.
    let sides = display::compose(
        &comparable,
        &saved.config.css,
        target,
        RenderHost {
            platform,
            hide_play_buttons,
        },
    );
    let errored = matches!(comparable.outcome, card::Outcome::Error { .. });
    let (question_html, answer_html) = if errored {
        (sides.question, sides.answer)
    } else {
        (
            type_answer::simulate_preview(&sides.question, false),
            type_answer::simulate_preview(&sides.answer, true),
        )
    };
    let media_refs = crate::rendering::media_refs([&question_html, &answer_html], &comparable);
    let cloze_numbers = if is_cloze { cloze_numbers(&values, target) } else { Vec::new() };
    Ok(DraftRender {
        card: RenderedCard {
            media_refs,
            card_id: stored_card.as_ref().map_or(meguri_model::ids::CardId(0), |c| c.id),
            ordinal: target,
            mode: RenderMode::Preview,
            question: Some(question_html),
            answer: Some(answer_html),
            question_replay_ids: sides.question_replay_ids,
            answer_replay_ids: sides.answer_replay_ids,
            comparable,
            css: saved.config.css.clone(),
            hide_play_buttons,
            review_extra,
            flag: stored_card.as_ref().map_or(0, |c| c.flags & 7),
            marked: tags.iter().any(|t| t.to_lowercase() == "marked"),
            type_in: None,
        },
        note: stored.map(|n| n.id),
        cloze_numbers,
    })
}

/// [`render_draft_on`] on one interactive snapshot (V2: never on the main
/// thread; the caller runs it on the interactive lane).
pub fn render_draft(
    collection: &Collection,
    draft: &StructureDraft,
    note: Option<NoteId>,
    target: u32,
    fill_empty: bool,
    platform: Platform,
) -> Result<DraftRender, EngineError> {
    let lease = collection.reader(ReaderRole::Interactive)?;
    let snapshot = lease.snapshot()?;
    render_draft_on(&*snapshot, draft, note, target, fill_empty, platform)
}

/// W9's note menu: the ids of the type's notes, ascending; `NotFound` for
/// an unknown note type.
pub fn note_ids(collection: &Collection, notetype: NotetypeId) -> Result<Vec<NoteId>, EngineError> {
    let lease = collection.reader(ReaderRole::Interactive)?;
    let snapshot = lease.snapshot()?;
    if snapshot.notetype(notetype)?.is_none() {
        return Err(EngineError::NotFound {
            table: Table::Notetypes,
            key: notetype.0.to_string(),
        });
    }
    Ok(snapshot.notetype_note_ids(notetype)?)
}

/// V6: every empty value (E1) becomes the sample cloze text when its field
/// is a cloze reference of the question (T11.1, exact names), else
/// `(<name>)`; a question with a structural error fills nothing.
fn fill(saved: &Notetype, question: &ParsedTemplate, values: &[String]) -> Vec<String> {
    if question.structure_error().is_some() {
        return values.to_vec();
    }
    let clozes = question.cloze_references();
    saved
        .fields
        .iter()
        .zip(values)
        .map(|(field, value)| {
            if !counts_as_empty(value) {
                value.clone()
            } else if clozes.contains(&field.name.as_str()) {
                SAMPLE_CLOZE.to_owned()
            } else {
                format!("({})", field.name)
            }
        })
        .collect()
}

/// V6's card list of a cloze draft, from the values before filling.
fn cloze_numbers(values: &[String], target: u32) -> Vec<u32> {
    let mut numbers: std::collections::BTreeSet<u32> =
        values.iter().flat_map(|v| cloze::numbers(v)).map(u32::from).collect();
    numbers.insert(target + 1);
    numbers.into_iter().collect()
}
