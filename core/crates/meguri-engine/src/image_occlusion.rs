//! Image-occlusion note services (render/image-occlusion-format F2, F13).
use meguri_model::note::Note;
use meguri_model::notetype::{Notetype, NotetypeField};

// The engine's editor service surface also gives higher layers the codec.
pub use meguri_template::image_occlusion::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldRoles {
    pub occlusions: usize,
    pub image: usize,
    pub header: usize,
    pub back_extra: usize,
}

/// F2: all four tags, using the first occurrence; otherwise positions 0–3.
pub fn field_roles(fields: &[NotetypeField]) -> Option<FieldRoles> {
    let find = |tag| fields.iter().position(|f| f.config.tag == Some(tag));
    match (find(0), find(1), find(2), find(3)) {
        (Some(occlusions), Some(image), Some(header), Some(back_extra)) => Some(FieldRoles {
            occlusions,
            image,
            header,
            back_extra,
        }),
        _ if fields.len() >= 4 => Some(FieldRoles {
            occlusions: 0,
            image: 1,
            header: 2,
            back_extra: 3,
        }),
        _ => None,
    }
}

pub fn detect(notetype: &Notetype) -> Option<FieldRoles> {
    (notetype.config.original_stock_kind == 6)
        .then(|| field_roles(&notetype.fields))
        .flatten()
}

#[derive(Clone, Debug)]
pub struct OcclusionNote {
    pub fields: FieldRoles,
    pub image_name: Option<String>,
    /// An unreadable field is reported with the rest of the note unchanged.
    pub document: Result<Document, Error>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotImageOcclusion;
impl std::fmt::Display for NotImageOcclusion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not_image_occlusion")
    }
}
impl std::error::Error for NotImageOcclusion {}

/// F13 Get, on a note and its note type from the same read snapshot.
pub fn get(note: &Note, notetype: &Notetype) -> Result<OcclusionNote, NotImageOcclusion> {
    let fields = detect(notetype)
        .filter(|_| note.notetype_id == notetype.id)
        .ok_or(NotImageOcclusion)?;
    let values: Vec<_> = note.field_values().collect();
    Ok(OcclusionNote {
        fields,
        image_name: values.get(fields.image).and_then(|s| image_name(s)).map(str::to_owned),
        document: parse(values.get(fields.occlusions).copied().unwrap_or_default()),
    })
}

/// F13's literal image lookup, deliberately independent of HTML decoding.
pub fn image_name(field: &str) -> Option<&str> {
    use regex::Regex;
    use std::sync::OnceLock;
    static TAG: OnceLock<Regex> = OnceLock::new();
    static SRC: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"<img\s[^>]*>").expect("image tag"));
    let src = SRC.get_or_init(|| Regex::new(r#"src\s*=\s*"([^"]+)""#).expect("image source"));
    for tag in tag.find_iter(field) {
        if let Some(capture) = src.captures_iter(tag.as_str()).last() {
            return capture.get(1).map(|m| m.as_str());
        }
    }
    None
}

/// F13 Update adapter for the normal note save (U27). Invoke inside the
/// normal update's executor op, with the note/type read in that same op.
/// Only that save owns normalization, derived fields and card generation.
/// A refusal never calls the save; the caller's normal no-op save handles
/// a replacement equal to the stored field.
pub fn update<T>(
    note: &Note,
    notetype: &Notetype,
    occlusions: &str,
    save: impl FnOnce(Note) -> Result<T, crate::EngineError>,
) -> Result<T, crate::EngineError> {
    let roles = detect(notetype)
        .filter(|_| note.notetype_id == notetype.id)
        .ok_or_else(|| crate::EngineError::InvalidArgument {
            message: NotImageOcclusion.to_string(),
        })?;
    let mut fields: Vec<_> = note.field_values().collect();
    let field = fields
        .get_mut(roles.occlusions)
        .ok_or_else(|| crate::EngineError::InvalidArgument {
            message: "Occlusion field missing from note".to_owned(),
        })?;
    *field = occlusions;
    let mut updated = note.clone();
    updated.fields = fields.join("\u{1f}");
    save(updated)
}
