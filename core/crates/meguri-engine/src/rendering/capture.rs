//! U62's owned read boundary. Neither digesting nor rendering takes another reader.
use super::{EngineError, Platform, RenderMode, missing, special_fields};
use meguri_model::{
    card::Card,
    ids::{DeckId, NotetypeId},
    note::Note,
    notetype::{Notetype, NotetypeTemplate},
};
use meguri_storage::{ConfigValue, ReadRows, Table};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, btree_map::Entry};

pub(crate) struct Capture {
    pub(super) card: Card,
    pub(crate) note: Note,
    pub(super) kind: Notetype,
    pub(super) template: NotetypeTemplate,
    home_name: Option<String>,
    pub(super) hide_play_buttons: bool,
    pub(super) review_extra: String,
    pub(super) mode: RenderMode,
    pub(super) platform: Platform,
}

impl Capture {
    pub(crate) fn read(
        rows: &impl ReadRows,
        card: Card,
        mode: RenderMode,
        platform: Platform,
    ) -> Result<Self, EngineError> {
        let note = rows
            .note(card.note_id)?
            .ok_or_else(|| missing(Table::Notes, card.note_id.0))?;
        let kind = rows
            .notetype(note.notetype_id)?
            .ok_or_else(|| missing(Table::Notetypes, note.notetype_id.0))?;
        let template = selected_template(&kind, &card)?.clone();
        let home = if card.original_deck_id.0 == 0 {
            card.deck_id
        } else {
            card.original_deck_id
        };
        let home_name = rows.deck(home)?.map(|d| d.human_name());
        let hide_play_buttons = matches!(
            rows.config_value("hideAudioPlayButtons")?,
            Some(ConfigValue::Bool(true))
        );
        let review_extra = match rows.config_value("reviewExtra")? {
            Some(ConfigValue::String(s)) => s,
            _ => String::new(),
        };
        Ok(Self {
            card,
            note,
            kind,
            template,
            home_name,
            hide_play_buttons,
            review_extra,
            mode,
            platform,
        })
    }

    pub(super) fn fields(&self) -> meguri_template::FieldMap {
        special_fields::from_captured(
            &self.card,
            &self.note,
            &self.kind,
            &self.template,
            self.home_name.as_deref(),
        )
    }

    pub(crate) fn key(&self) -> [u8; 32] {
        capture_key(
            &self.card,
            &self.note,
            &self.kind,
            &self.template,
            self.home_name.as_deref(),
            self.hide_play_buttons,
            &self.review_extra,
            self.mode,
            self.platform,
        )
    }
}

/// Key-only sharing for one immutable reader snapshot. Notes and cards are never cached.
pub(crate) struct KeyBatch<'a, R> {
    rows: &'a R,
    kinds: BTreeMap<NotetypeId, Notetype>,
    homes: BTreeMap<DeckId, Option<String>>,
    settings: Option<(bool, String)>,
    mode: RenderMode,
    platform: Platform,
}

impl<'a, R: ReadRows> KeyBatch<'a, R> {
    pub(crate) fn new(rows: &'a R, mode: RenderMode, platform: Platform) -> Self {
        Self {
            rows,
            kinds: BTreeMap::new(),
            homes: BTreeMap::new(),
            settings: None,
            mode,
            platform,
        }
    }

    pub(crate) fn rows(&self) -> &'a R {
        self.rows
    }

    pub(crate) fn key(
        &mut self,
        card: &Card,
        project_note: impl FnOnce(&mut Note) -> Result<(), EngineError>,
    ) -> Result<([u8; 32], Note), EngineError> {
        let mut note = self
            .rows
            .note(card.note_id)?
            .ok_or_else(|| missing(Table::Notes, card.note_id.0))?;
        // Select from the stored note, before a pending undo replaces its values,
        // exactly as Capture::read followed by the session's note projection does.
        let kind = match self.kinds.entry(note.notetype_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(
                self.rows
                    .notetype(note.notetype_id)?
                    .ok_or_else(|| missing(Table::Notetypes, note.notetype_id.0))?,
            ),
        };
        let template = selected_template(kind, card)?;
        let home = if card.original_deck_id.0 == 0 {
            card.deck_id
        } else {
            card.original_deck_id
        };
        let home_name = match self.homes.entry(home) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(self.rows.deck(home)?.map(|d| d.human_name())),
        };
        let (hide_play_buttons, review_extra) = match &mut self.settings {
            Some(settings) => settings,
            empty @ None => empty.insert((
                matches!(
                    self.rows.config_value("hideAudioPlayButtons")?,
                    Some(ConfigValue::Bool(true))
                ),
                match self.rows.config_value("reviewExtra")? {
                    Some(ConfigValue::String(s)) => s,
                    _ => String::new(),
                },
            )),
        };
        project_note(&mut note)?;
        let key = capture_key(
            card,
            &note,
            kind,
            template,
            home_name.as_deref(),
            *hide_play_buttons,
            review_extra,
            self.mode,
            self.platform,
        );
        Ok((key, note))
    }
}

fn selected_template<'a>(kind: &'a Notetype, card: &Card) -> Result<&'a NotetypeTemplate, EngineError> {
    if kind.config.kind == 1 {
        kind.templates.first()
    } else {
        kind.templates.iter().find(|t| t.ord == u32::from(card.ord))
    }
    .ok_or_else(|| missing(Table::Templates, format!("{}:{}", kind.id.0, card.ord)))
}

#[allow(clippy::too_many_arguments)]
fn capture_key(
    card: &Card,
    note: &Note,
    kind: &Notetype,
    template: &NotetypeTemplate,
    home_name: Option<&str>,
    hide_play_buttons: bool,
    review_extra: &str,
    mode: RenderMode,
    platform: Platform,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"meguri.render.capture.v1\0");
    fn text(h: &mut Sha256, s: &str) {
        h.update((s.len() as u64).to_le_bytes());
        h.update(s.as_bytes());
    }
    h.update([
        match mode {
            RenderMode::Review => 0,
            RenderMode::Preview => 1,
            RenderMode::Browser => 2,
        },
        match platform {
            Platform::Mac => 0,
            Platform::Ios => 1,
        },
    ]);
    text(&mut h, &kind.name);
    h.update(kind.config.kind.to_le_bytes());
    text(&mut h, &kind.config.css);
    let values: Vec<_> = note.field_values().collect();
    h.update((values.len() as u64).to_le_bytes());
    for value in values {
        text(&mut h, value);
    }
    let tags: Vec<_> = crate::tags::canonical::split_text(&note.tags).collect();
    h.update((tags.len() as u64).to_le_bytes());
    for tag in tags {
        text(&mut h, tag);
    }
    h.update((kind.fields.len() as u64).to_le_bytes());
    for field in &kind.fields {
        text(&mut h, &field.name);
        h.update(field.ord.to_le_bytes());
        text(&mut h, &field.config.font_name);
        h.update(field.config.font_size.to_le_bytes());
    }
    h.update(template.ord.to_le_bytes());
    text(&mut h, &template.name);
    let (q, a) = super::template_text(template, mode);
    text(&mut h, q);
    text(&mut h, a);
    h.update([u8::from(home_name.is_some())]);
    if let Some(name) = home_name {
        text(&mut h, name);
    }
    h.update([u8::from(hide_play_buttons)]);
    text(&mut h, review_extra);
    meguri_sched::session::token::render_key(card, note.mtime, kind.id, kind.mtime, &h.finalize().into())
}
