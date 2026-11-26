//! Deck packages (`.apkg`): classifying a file (P18), reading the three
//! container formats (P17) and importing one into the open collection
//! (`package/apkg-import`, `package/notetype-merge`).
//!
//! The surface a caller uses: [`classify`], [`inspect`] (a session with its
//! preview facts), [`read_options`] (the five stored option values),
//! [`apply`] (the one undoable "Import" step) and dropping or
//! [`ImportSession::close`]-ing the session.
mod apply;
mod archive;
mod classify;
mod commit;
mod gather;
mod media;
mod merge;
mod notetypes;
mod plan;
mod session;

use std::path::PathBuf;
use std::sync::Arc;

use meguri_engine::EngineError;
use meguri_model::clock::Clock;
use meguri_model::rng::RandomSource;
use meguri_storage::space::SpaceProvider;

pub use apply::{apply, apply_confirming, apply_on, read_options};
pub use archive::PackageFormat;
pub use classify::{PackageClass, classify, classify_name};
pub(crate) use gather::CardShaper;
pub use session::{ImportSession, inspect};

/// The most rows an import may bring: the package's notes and cards, and
/// its review log when learning progress is included. An apply reads every
/// text in pages, so its memory does not grow with what the rows store; it
/// grows with their number (its own identity and decision per note, and the
/// op's undo journal, which indexes every written row), and this bounds it
/// (R1852).
pub const MAX_ROWS: u64 = 2_000_000;

/// What else a package may hold; with [`MAX_ROWS`] these keep an import's
/// memory and time bounded whatever the file stores. Each is far above
/// what a deck holds. A package past one is refused at the inspect, by
/// aggregates, before any row is read.
pub const LIMITS: meguri_storage::deck_package::DeckCandidateLimits = meguri_storage::deck_package::DeckCandidateLimits {
    rows: MAX_ROWS,
    guid_bytes: 255,
    // A page of notes is `gather::NOTE_PAGE` of these at most.
    note_bytes: 2 * 1024 * 1024,
    card_data_bytes: 64 * 1024,
    tag_uses: 4 * MAX_ROWS,
    structure_rows: 10_000,
    notetype_parts: 5000,
    structure_bytes: 64 * 1024 * 1024,
};

/// The detail of the `Io` error an apply returns when its op committed and
/// its result was lost: the rows are written, and the import is never
/// repeated (`package/transfer-sheets` X2.6).
pub const COMMITTED_WITHOUT_RESULT: &str = "the import committed without a result";

/// What an import needs from its host besides the engine.
#[derive(Clone)]
pub struct ApkgContext {
    /// The directory of `collection.anki2`: staging lives there.
    pub collection_directory: PathBuf,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub space: Arc<dyn SpaceProvider>,
}

/// I5's three-valued options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum UpdateCondition {
    #[default]
    IfNewer,
    Always,
    Never,
}

impl UpdateCondition {
    /// The JSON string the config key holds.
    pub fn stored(self) -> &'static str {
        match self {
            UpdateCondition::IfNewer => "IfNewer",
            UpdateCondition::Always => "Always",
            UpdateCondition::Never => "Never",
        }
    }

    pub(crate) fn from_stored(text: Option<&str>) -> UpdateCondition {
        match text {
            Some("Always") => UpdateCondition::Always,
            Some("Never") => UpdateCondition::Never,
            _ => UpdateCondition::IfNewer,
        }
    }
}

/// I5: the five options, in the spec's order. The default is what missing
/// keys read as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ImportOptions {
    pub merge_notetypes: bool,
    pub update_notes: UpdateCondition,
    pub update_notetypes: UpdateCondition,
    pub with_scheduling: bool,
    pub with_deck_configs: bool,
}

/// I4's `repaired`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Repaired {
    pub note_ids: u64,
    pub card_ids: u64,
    pub revlog_ids: u64,
    pub field_counts: u64,
    pub scheduler_converted: bool,
}

/// I4: what the package holds, known before any option is chosen. The
/// option defaults are [`read_options`]'s, read from the open collection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewFacts {
    pub format: PackageFormat,
    pub notes: u64,
    pub cards: u64,
    /// Human deck names, sorted by unicase.
    pub decks: Vec<String>,
    pub notetypes: Vec<String>,
    pub has_scheduling: bool,
    pub has_presets: bool,
    pub media_files: u64,
    pub media_bytes: u64,
    pub repaired: Repaired,
}

/// One entry of the import log (I13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// The target note's id, or the package note's for `conflicting` and
    /// `missing_notetype`.
    pub note: i64,
    /// Every field, reduced as I13 says; empty once the log's text budget
    /// is spent (`ImportLog::fields_omitted`).
    pub fields: Vec<String>,
    /// For `conflicting`: the target note that holds the GUID.
    pub existing_id: Option<i64>,
}

/// I13's categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogCategory {
    New,
    Updated,
    Duplicate,
    Conflicting,
    MissingNotetype,
    MissingDeck,
    FirstFieldMatch,
    EmptyFirstField,
}

/// I13: the log of one import, each list in I9's order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportLog {
    pub found_notes: u64,
    /// Entries that carry no field text: the log holds 16 MiB of it in all
    /// (R1852), and later entries keep their ids only.
    pub fields_omitted: u64,
    pub new: Vec<LogEntry>,
    pub updated: Vec<LogEntry>,
    pub duplicate: Vec<LogEntry>,
    pub conflicting: Vec<LogEntry>,
    pub missing_notetype: Vec<LogEntry>,
    pub missing_deck: Vec<LogEntry>,
    /// Always empty for a deck package.
    pub first_field_match: Vec<LogEntry>,
    /// Always empty for a deck package.
    pub empty_first_field: Vec<LogEntry>,
}

impl ImportLog {
    pub fn category(&self, category: LogCategory) -> &[LogEntry] {
        match category {
            LogCategory::New => &self.new,
            LogCategory::Updated => &self.updated,
            LogCategory::Duplicate => &self.duplicate,
            LogCategory::Conflicting => &self.conflicting,
            LogCategory::MissingNotetype => &self.missing_notetype,
            LogCategory::MissingDeck => &self.missing_deck,
            LogCategory::FirstFieldMatch => &self.first_field_match,
            LogCategory::EmptyFirstField => &self.empty_first_field,
        }
    }

    /// A page of one category, for the result table (U133).
    pub fn page(&self, category: LogCategory, offset: usize, count: usize) -> &[LogEntry] {
        let entries = self.category(category);
        let start = offset.min(entries.len());
        &entries[start..entries.len().min(start.saturating_add(count))]
    }
}

/// I14.5's counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub notes_added: u64,
    pub notes_updated: u64,
    pub cards_added: u64,
    pub revlog_added: u64,
    pub notetypes_added: u64,
    pub notetypes_updated: u64,
    pub notetypes_copied: u64,
    /// With a merge: target notes moved between note types.
    pub notes_moved: u64,
    /// With a merge: emptied note types removed.
    pub notetypes_removed: u64,
    pub decks_added: u64,
    pub decks_updated: u64,
    pub presets_added: u64,
    pub media_written: u64,
    pub media_reused: u64,
    pub media_skipped: u64,
}

/// I14.5: what a committed import reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportResult {
    /// The op's seq; the undo step's id.
    pub seq: u64,
    pub generation: u64,
    /// The undo step's name ("Import"), when the import changed something.
    pub undo_step: Option<&'static str>,
    pub log: ImportLog,
    pub counts: ImportCounts,
    /// What the op reported about note type structure (I14.4).
    pub structure: StructureSummary,
    /// The safety backup's name, when one was taken or reused (I14.4).
    pub safety_backup: Option<String>,
    /// The rows are committed and some media files are still being put in
    /// place by the journal's recovery (`MediaCommitPending`): never
    /// import again.
    pub media_pending: bool,
}

/// The structure report an import committed with (`engine/change-stream`
/// S2.5, `sync/cloudkit-records` K19.1): its reason codes, the note types
/// with a layout entry, the removed ones and the (old, new) type changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StructureSummary {
    /// 8 `apkg_apply` always; 3 `sort_field_changed`; 9 `notetypes_merged`.
    pub reasons: Vec<u32>,
    /// Per note type with a layout entry: its id and whether the entry is
    /// the identity (names changed, positions kept).
    pub layouts: Vec<(i64, bool)>,
    pub removed: Vec<i64>,
    pub type_changes: Vec<(i64, i64)>,
}

impl StructureSummary {
    pub(crate) fn of(report: &meguri_storage::journal::StructureReport) -> StructureSummary {
        StructureSummary {
            reasons: report.reasons.iter().map(|reason| *reason as u32).collect(),
            layouts: report.layouts.iter().map(|(id, layout)| (*id, layout.is_identity())).collect(),
            removed: report.removed.iter().copied().collect(),
            type_changes: report.type_changes.keys().copied().collect(),
        }
    }
}

/// J10: one field position whose content an update would drop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptiedField {
    pub notetype: i64,
    pub notetype_name: String,
    pub position: u32,
    pub field_name: String,
    pub notes: u64,
}

/// J10: a note type the merge removes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedNotetype {
    pub notetype: i64,
    pub name: String,
    pub moved_to: i64,
    pub notes: u64,
}

/// J10: notes storing more values than their note type has fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DroppedValues {
    pub notetype: i64,
    pub notes: u64,
}

/// J10: what a merge import would drop. Empty when every list is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergePreview {
    pub emptied_fields: Vec<EmptiedField>,
    pub removed_notetypes: Vec<RemovedNotetype>,
    pub dropped_values: Vec<DroppedValues>,
}

impl MergePreview {
    pub fn is_empty(&self) -> bool {
        self.emptied_fields.is_empty() && self.removed_notetypes.is_empty() && self.dropped_values.is_empty()
    }

    /// SHA-256 of the preview's canonical encoding (I8's digest).
    pub(crate) fn digest(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        let mut text = |tag: &str, value: &str| {
            hash.update(tag.as_bytes());
            hash.update((value.len() as u64).to_le_bytes());
            hash.update(value.as_bytes());
        };
        for entry in &self.emptied_fields {
            text("e", &format!("{}:{}:{}", entry.notetype, entry.position, entry.notes));
            text("n", &entry.notetype_name);
            text("f", &entry.field_name);
        }
        for entry in &self.removed_notetypes {
            text("r", &format!("{}:{}:{}", entry.notetype, entry.moved_to, entry.notes));
            text("n", &entry.name);
        }
        for entry in &self.dropped_values {
            text("d", &format!("{}:{}", entry.notetype, entry.notes));
        }
        hash.finalize().into()
    }
}

/// I14.4: a reason that makes an import a schema change. A plan that
/// carries one sets `scm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StructuralReason {
    /// An update that makes another field the sort field (I7).
    SortFieldChanged,
    /// A merge with an effect (`package/notetype-merge` J9).
    NotetypesMerged,
}

impl StructuralReason {
    /// The plan's reasons, sorted, each once.
    pub(crate) fn of(reasons: &[meguri_engine::schema_gate::SchemaReason]) -> Vec<StructuralReason> {
        use meguri_engine::schema_gate::SchemaReason;
        let mut found: Vec<StructuralReason> = reasons
            .iter()
            .filter_map(|reason| match reason {
                SchemaReason::SortFieldChanged => Some(StructuralReason::SortFieldChanged),
                SchemaReason::NotetypesMerged => Some(StructuralReason::NotetypesMerged),
                _ => None,
            })
            .collect();
        found.sort();
        found.dedup();
        found
    }
}

/// I8: proof that the caller saw a merge preview and, when it asked for
/// them, the plan's structural reasons. Opaque to callers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationToken {
    pub(crate) session: u64,
    pub(crate) options: ImportOptions,
    pub(crate) digest: [u8; 32],
    /// The structural reasons the confirmation showed; `None` when the
    /// caller did not ask to confirm structure.
    pub(crate) structure: Option<Vec<StructuralReason>>,
    /// Diagnostics only; never checked.
    pub(crate) snapshot_seq: u64,
}

impl ConfirmationToken {
    /// This confirmation as it covers the preview alone. An apply that asks
    /// to confirm structure then answers `ConfirmationRequired` for every
    /// plan with a structural reason: the caller that runs such a plan
    /// elsewhere (`sync/meguri-provider` MP7.1's job) uses it for an apply
    /// that must not commit one.
    pub fn without_structure(&self) -> ConfirmationToken {
        ConfirmationToken {
            structure: None,
            ..self.clone()
        }
    }
}

/// I16.
#[derive(Clone, Debug, PartialEq)]
pub enum ApkgImportError {
    /// P18 classified the file `Unsupported`.
    UnsupportedFile,
    TooNew,
    Corrupt {
        detail: String,
    },
    SchemaUnsupported {
        gate: Box<meguri_storage::OpenError>,
    },
    InsufficientSpace {
        needed: u64,
        available: u64,
    },
    DiskFull,
    /// After three replans (I14.2).
    Stale,
    /// I8. `reasons` is filled only for a caller that asked to confirm
    /// structure (`sync/meguri-provider` MP7.1); the preview can then be
    /// empty.
    ConfirmationRequired {
        preview: Box<MergePreview>,
        reasons: Vec<StructuralReason>,
        token: Box<ConfirmationToken>,
    },
    /// A standard and a cloze note type would merge (R1606).
    NotetypeKindConflict {
        incoming: i64,
        target: i64,
    },
    DeckTooDeep {
        name: String,
    },
    /// The package is past one of [`LIMITS`], which `what` names: refused
    /// at the inspect before any row is read, or, for the rows with the
    /// review log counted, at the start of an apply, before anything is
    /// planned or staged.
    TooLarge {
        what: &'static str,
        found: u64,
        limit: u64,
    },
    SessionClosed,
    Cancelled,
    /// The rows are committed; media publication finishes through recovery.
    /// The result is the import's, with `media_pending` set.
    MediaCommitPending(Box<ImportResult>),
    Io {
        detail: String,
    },
    /// `Busy`, `CollectionUnavailable`, `StaleGeneration`,
    /// `SafetyBackupFailed` and any other engine refusal, as the engine
    /// words it.
    Engine(Box<EngineError>),
}

impl std::fmt::Display for ApkgImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ApkgImportError {}

pub(crate) type ImportOutcome<T> = std::result::Result<T, ApkgImportError>;

impl From<crate::Error> for ApkgImportError {
    fn from(error: crate::Error) -> Self {
        use crate::Error as E;
        match error {
            E::TooNew => Self::TooNew,
            E::Corrupt { detail } => Self::Corrupt { detail },
            E::SchemaUnsupported { gate } => Self::SchemaUnsupported { gate: Box::new(gate) },
            E::InsufficientSpace { needed, available } => Self::InsufficientSpace { needed, available },
            E::DiskFull => Self::DiskFull,
            E::Cancelled => Self::Cancelled,
            E::Io { detail } => Self::Io { detail },
            other => Self::Corrupt {
                detail: other.to_string(),
            },
        }
    }
}

impl From<EngineError> for ApkgImportError {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::Cancelled => Self::Cancelled,
            EngineError::Stale { .. } => Self::Stale,
            other => Self::Engine(Box::new(other)),
        }
    }
}

impl From<meguri_storage::StorageError> for ApkgImportError {
    fn from(error: meguri_storage::StorageError) -> Self {
        crate::Error::from(error).into()
    }
}

impl From<std::io::Error> for ApkgImportError {
    fn from(error: std::io::Error) -> Self {
        crate::Error::from(error).into()
    }
}

pub(crate) fn corrupt(detail: impl Into<String>) -> ApkgImportError {
    ApkgImportError::Corrupt { detail: detail.into() }
}
