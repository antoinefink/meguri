//! The worst-case shapes (R1805), read from `profiles/worst-cases/*.json`:
//! the user's shape at a card count, with one stress applied.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::GenError;
use super::profile::Histogram;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorstCase {
    pub name: String,
    pub description: String,
    /// Cards to generate.
    pub cards: u64,
    /// Normal decks, the default deck included.
    #[serde(default)]
    pub decks: Option<u64>,
    /// Tags forming one hierarchy of this many nodes, every node on a note.
    #[serde(default)]
    pub tag_nodes: Option<u64>,
    /// Bytes of the first note type's first front template.
    #[serde(default)]
    pub template_bytes: Option<u64>,
    #[serde(default)]
    pub media_files: Option<MediaFiles>,
    #[serde(default)]
    pub large_fields: Option<LargeFields>,
    #[serde(default)]
    pub inline_images: Option<InlineImages>,
    #[serde(default)]
    pub due_deck: Option<DueDeck>,
}

/// Exactly `count` media files, written, with sizes from `bytes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaFiles {
    pub count: u64,
    pub bytes: Histogram,
}

/// The first `notes` notes hold a field of `bytes` bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LargeFields {
    pub notes: u64,
    pub bytes: u64,
}

/// The first `notes` notes hold `per_note` inline base64 PNGs of
/// `image_bytes` bytes in their first field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InlineImages {
    pub notes: u64,
    pub per_note: u64,
    pub image_bytes: u64,
}

/// Every card is a review card due today or earlier, in one deck besides
/// the default deck, whose preset allows `review_limit` reviews a day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DueDeck {
    pub review_limit: u32,
}

impl WorstCase {
    pub fn load(path: &Path) -> Result<WorstCase, GenError> {
        let text = std::fs::read_to_string(path).map_err(|e| GenError::io(path, e))?;
        serde_json::from_str(&text).map_err(|e| GenError::io(path, e))
    }

    /// The committed worst case `name` (`profiles/worst-cases/<name>.json`).
    pub fn named(name: &str) -> Result<WorstCase, GenError> {
        WorstCase::load(&dir().join(format!("{name}.json")))
    }

    /// The names of the committed worst cases, sorted.
    pub fn names() -> Result<Vec<String>, GenError> {
        let dir = dir();
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| GenError::io(&dir, e))? {
            let path = entry.map_err(|e| GenError::io(&dir, e))?.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Some(stem) = path.file_stem()
            {
                names.push(stem.to_string_lossy().into_owned());
            }
        }
        names.sort();
        Ok(names)
    }
}

/// `core/crates/meguri-testkit/profiles/worst-cases`.
pub fn dir() -> PathBuf {
    super::profiles_dir().join("worst-cases")
}
