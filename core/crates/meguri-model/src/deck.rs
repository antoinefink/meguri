//! Deck records (`specs/model/row-records.md` R7) and deck names
//! (`specs/model/schema11-json.md` S11, R1465).

use unicode_normalization::UnicodeNormalization;

use crate::ids::DeckId;
use crate::note::FIELD_SEPARATOR;
use crate::proto::ProtoBlob;
use crate::proto::deck::{DeckCommon, DeckKindMessage};
use crate::time::EpochSecs;
use crate::usn::Usn;

/// A `decks` row with its two blobs.
#[derive(Clone, Debug)]
pub struct Deck {
    pub id: DeckId,
    /// Native form: components joined by U+001F.
    pub name: String,
    pub mtime: EpochSecs,
    pub usn: Usn,
    pub common: ProtoBlob<DeckCommon>,
    pub kind: ProtoBlob<DeckKindMessage>,
}

impl Deck {
    /// The name as shown, components joined by `::`.
    pub fn human_name(&self) -> String {
        human_name(&self.name)
    }
}

/// S11 native to human: every U+001F becomes `::`.
pub fn human_name(native: &str) -> String {
    native.replace(FIELD_SEPARATOR, "::")
}

/// S11 human to native: split at each `::` from the left; in each component
/// NFC, ASCII controls removed, Unicode whitespace and `:` trimmed from both
/// ends, and `blank` when nothing is left; joined with U+001F.
pub fn native_name(human: &str) -> String {
    let components: Vec<String> = human
        .split("::")
        .map(|component| {
            let composed: String = component.nfc().filter(|c| !c.is_ascii_control()).collect();
            let trimmed = composed.trim_matches(|c: char| c.is_whitespace() || c == ':');
            if trimmed.is_empty() {
                "blank".to_owned()
            } else {
                trimmed.to_owned()
            }
        })
        .collect();
    components.join(&FIELD_SEPARATOR.to_string())
}

/// decks/decks D1: normalization on every stored-name write.
pub fn normalize_native_name(native: &str) -> String {
    native
        .split(FIELD_SEPARATOR)
        .map(normalize_component)
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

fn normalize_component(component: &str) -> String {
    let clean: String = component.chars().filter(|c| !c.is_ascii_control()).collect();
    let composed: String = clean.nfc().collect();
    let trimmed = composed.trim_matches(|c: char| c.is_whitespace() || c == ':');
    if trimmed.is_empty() {
        "blank".to_owned()
    } else {
        trimmed.to_owned()
    }
}
