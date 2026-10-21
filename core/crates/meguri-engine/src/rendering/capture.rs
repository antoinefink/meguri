//! U62's owned read boundary. Neither digesting nor rendering takes another reader.
use super::{EngineError, Platform, RenderMode, missing, special_fields};
use meguri_model::{
    card::Card,
    note::Note,
    notetype::{Notetype, NotetypeTemplate},
};
use meguri_storage::{ConfigValue, ReadRows, Table};
use sha2::{Digest, Sha256};

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
        let template = if kind.config.kind == 1 {
            kind.templates.first()
        } else {
            kind.templates.iter().find(|t| t.ord == u32::from(card.ord))
        }
        .cloned()
        .ok_or_else(|| missing(Table::Templates, format!("{}:{}", kind.id.0, card.ord)))?;
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
        let mut h = Sha256::new();
        h.update(b"meguri.render.capture.v1\0");
        fn text(h: &mut Sha256, s: &str) {
            h.update((s.len() as u64).to_le_bytes());
            h.update(s.as_bytes());
        }
        h.update([
            match self.mode {
                RenderMode::Review => 0,
                RenderMode::Preview => 1,
                RenderMode::Browser => 2,
            },
            match self.platform {
                Platform::Mac => 0,
                Platform::Ios => 1,
            },
        ]);
        text(&mut h, &self.kind.name);
        h.update(self.kind.config.kind.to_le_bytes());
        text(&mut h, &self.kind.config.css);
        let values: Vec<_> = self.note.field_values().collect();
        h.update((values.len() as u64).to_le_bytes());
        for value in values {
            text(&mut h, value);
        }
        let tags: Vec<_> = crate::tags::canonical::split_text(&self.note.tags).collect();
        h.update((tags.len() as u64).to_le_bytes());
        for tag in tags {
            text(&mut h, tag);
        }
        h.update((self.kind.fields.len() as u64).to_le_bytes());
        for field in &self.kind.fields {
            text(&mut h, &field.name);
            h.update(field.ord.to_le_bytes());
            text(&mut h, &field.config.font_name);
            h.update(field.config.font_size.to_le_bytes());
        }
        h.update(self.template.ord.to_le_bytes());
        text(&mut h, &self.template.name);
        let (q, a) = super::template_text(&self.template, self.mode);
        text(&mut h, q);
        text(&mut h, a);
        h.update([u8::from(self.home_name.is_some())]);
        if let Some(name) = &self.home_name {
            text(&mut h, name);
        }
        h.update([u8::from(self.hide_play_buttons)]);
        text(&mut h, &self.review_extra);
        meguri_sched::session::token::render_key(
            &self.card,
            self.note.mtime,
            self.kind.id,
            self.kind.mtime,
            &h.finalize().into(),
        )
    }
}
