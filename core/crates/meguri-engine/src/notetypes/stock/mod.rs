//! The six stock note types (`specs/notetypes/stock-kinds.md`): their
//! structure is Anki's (K1, K3, K4), their template text and styling are
//! Meguri's own (K5 to K7, R908, R1959), in `templates/`. A new collection
//! holds all six (K2); "Add: <stock kind>" adds one later (K8).

pub mod basic;
pub mod cloze;
pub mod image_occlusion;

use meguri_model::config::ConfigEntry;
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_model::proto::ProtoBlob;
use meguri_model::rng::RandomSource;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::{StampMode, WriteScope};

use super::{CURRENT_NOTETYPE_KEY, Normalize, NotetypeKind, insert, new_config, new_field, new_template};
use crate::error::EngineError;

/// K1, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StockKind {
    Basic,
    BasicAndReversed,
    BasicOptionalReversed,
    BasicTyping,
    Cloze,
    ImageOcclusion,
}

impl StockKind {
    /// K1's order: the order a new collection adds them in (K2).
    pub const ALL: [StockKind; 6] = [
        StockKind::Basic,
        StockKind::BasicAndReversed,
        StockKind::BasicOptionalReversed,
        StockKind::BasicTyping,
        StockKind::Cloze,
        StockKind::ImageOcclusion,
    ];

    /// The stored `original_stock_kind` (K1).
    pub const fn stored(self) -> i32 {
        match self {
            StockKind::Basic => 1,
            StockKind::BasicAndReversed => 2,
            StockKind::BasicOptionalReversed => 3,
            StockKind::BasicTyping => 4,
            StockKind::Cloze => 5,
            StockKind::ImageOcclusion => 6,
        }
    }

    /// Anki's English name (K1), which Meguri does not localize.
    pub const fn name(self) -> &'static str {
        match self {
            StockKind::Basic => "Basic",
            StockKind::BasicAndReversed => "Basic (and reversed card)",
            StockKind::BasicOptionalReversed => "Basic (optional reversed card)",
            StockKind::BasicTyping => "Basic (type in the answer)",
            StockKind::Cloze => "Cloze",
            StockKind::ImageOcclusion => "Image Occlusion",
        }
    }

    /// Standard or cloze (K1).
    pub const fn kind(self) -> NotetypeKind {
        match self {
            StockKind::Cloze | StockKind::ImageOcclusion => NotetypeKind::Cloze,
            _ => NotetypeKind::Standard,
        }
    }

    /// The kind's definition (K4, K5).
    fn definition(self) -> Definition {
        match self {
            StockKind::Basic
            | StockKind::BasicAndReversed
            | StockKind::BasicOptionalReversed
            | StockKind::BasicTyping => basic::definition(self),
            StockKind::Cloze => cloze::definition(),
            StockKind::ImageOcclusion => image_occlusion::definition(),
        }
    }

    /// The kind as a new note type (K3, K4) under its K1 name, with fresh
    /// random field and template ids, row stamps 0 and its card
    /// requirements left to the add (N6). Not yet normalized.
    pub fn notetype(self, random: &dyn RandomSource) -> Notetype {
        let definition = self.definition();
        let fields = definition
            .fields
            .iter()
            .map(|field| {
                let mut new = new_field(field.name, random);
                new.config.tag = field.tag;
                new.config.prevent_deletion = field.protected;
                new
            })
            .collect();
        let templates = definition
            .templates
            .iter()
            .map(|t| new_template(t.name, t.front, t.back, random))
            .collect();
        Notetype {
            id: NotetypeId(0),
            name: self.name().to_owned(),
            mtime: EpochSecs(0),
            usn: Usn(0),
            config: ProtoBlob::new(new_config(self.kind(), definition.css, self.stored())),
            fields,
            templates,
        }
    }
}

/// One field of a kind (K4): `tag` absent unless given, `protected` its
/// `prevent_deletion`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StockField {
    pub name: &'static str,
    pub tag: Option<u32>,
    pub protected: bool,
}

impl StockField {
    pub(crate) const fn plain(name: &'static str) -> StockField {
        StockField {
            name,
            tag: None,
            protected: false,
        }
    }
}

/// One template of a kind: its name, front and back (K5).
#[derive(Clone, Copy, Debug)]
pub(crate) struct StockTemplate {
    pub name: &'static str,
    pub front: &'static str,
    pub back: &'static str,
}

/// What a kind holds beyond K3's common values.
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub fields: Vec<StockField>,
    pub templates: Vec<StockTemplate>,
    pub css: String,
}

/// K2: the six kinds a new collection holds, added in K1's order as part of
/// the new collection (`mtime_secs` and `usn` 0 on their `notetypes` and
/// `templates` rows), and `curModel` set to Basic's id as a local config
/// change. The scope must write rows as given (`CallerSupplied` or `Raw`):
/// the new collection's bootstrap (U28) and the conformance runner's stand-in
/// for it (`specs/conformance/notetype-ops.md` T10.2). Returns their ids in
/// K1's order.
pub fn add_to_new_collection(
    scope: &mut WriteScope<'_>,
    random: &dyn RandomSource,
) -> Result<Vec<NotetypeId>, EngineError> {
    if !matches!(scope.mode(), StampMode::CallerSupplied | StampMode::Raw) {
        return Err(EngineError::InvalidArgument {
            message: format!(
                "a new collection's stock note types are written as given, not in a {:?} scope",
                scope.mode()
            ),
        });
    }
    let mut ids = Vec::with_capacity(StockKind::ALL.len());
    for kind in StockKind::ALL {
        ids.push(insert(scope, kind.notetype(random), Normalize::All, false)?.id);
    }
    let now = scope.now();
    scope.put_config_entry(&ConfigEntry {
        key: CURRENT_NOTETYPE_KEY.to_owned(),
        usn: Usn::PENDING,
        mtime: now.whole_secs(),
        value: ids[0].0.to_string().into_bytes(),
    })?;
    Ok(ids)
}
