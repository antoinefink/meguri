//! Deck package (`.apkg`) import over FFI (`package/apkg-import` I2 to I16,
//! `package/transfer-sheets` X2): classify a file, inspect it into a session
//! with its preview facts, read the stored option values, apply, page the
//! log and close.
//!
//! - The inspect and the apply are jobs: progress and cancellation go
//!   through the job's handle, and the typed outcome is read from the
//!   start's result object once the job's `JobFinished` arrived. A refusal
//!   is an outcome, never a `CoreError` to parse: every case of the engine's
//!   import error has its own [`DeckImportRefusal`].
//! - A session owns its staging directory. `close`, dropping the session
//!   object, and closing the collection remove it; after a kill the launch
//!   sweep does (I2).
//! - With a ready Meguri Sync binding the apply runs inside the structure
//!   job instead (`sync/meguri-provider` MP7.1): `ProviderStructureOp::Import`.
//!
//! The caller keeps the selected file's security scope and coordinated read
//! until the inspect's job ends (`package/file-intake` K3): the source copy
//! is the inspect's first stage, and the file is never read after it.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use meguri_engine::{EngineError, JobKind};
use meguri_model::ids::NotetypeId;
use meguri_package::apkg::{self, ApkgImportError, ConfirmationToken, ImportCounts, ImportOptions, ImportResult,
    ImportSession, LogCategory, MergePreview, PackageClass, PackageFormat, PreviewFacts, StructuralReason,
    UpdateCondition};
use meguri_storage::{OpenError, ReadRows, ReaderRole};

use crate::jobs::{self, JobHandle};
use crate::{CoreError, callback, runtime};

/// `package/container` P18: the flow a file belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PackageFileClass {
    /// A deck package: this import.
    DeckPackage,
    /// A whole collection: the collection package import.
    CollectionPackage,
    Unsupported,
}
impl From<PackageClass> for PackageFileClass {
    fn from(class: PackageClass) -> Self {
        match class {
            PackageClass::Apkg => Self::DeckPackage,
            PackageClass::Colpkg => Self::CollectionPackage,
            PackageClass::Unsupported => Self::Unsupported,
        }
    }
}

/// P18 step 1 alone: the class the name gives, or none when the content
/// decides. Pure; reads nothing.
#[uniffi::export]
pub fn package_file_class_of_name(name: String) -> Result<Option<PackageFileClass>, CoreError> {
    callback::enter("package_file_class_of_name");
    Ok(apkg::classify_name(&name).map(Into::into))
}

/// P18 for the file at `path`, read in place and read only (at most the
/// archive's directory). `name` is the name the user saw.
#[uniffi::export]
pub fn package_file_class(path: String, name: String) -> Result<PackageFileClass, CoreError> {
    callback::enter("package_file_class");
    Ok(apkg::classify(Path::new(&path), &name).into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckPackageFormat { Latest, Legacy2, Legacy1 }

/// I4's `repaired`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportRepairs {
    pub note_ids: u64,
    pub card_ids: u64,
    pub revlog_ids: u64,
    pub field_counts: u64,
    pub scheduler_converted: bool,
}

/// I4: what the package holds, known before any option is chosen.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportFacts {
    pub format: DeckPackageFormat,
    pub notes: u64,
    pub cards: u64,
    /// Human deck names, sorted.
    pub decks: Vec<String>,
    pub notetypes: Vec<String>,
    pub has_scheduling: bool,
    pub has_presets: bool,
    pub media_files: u64,
    pub media_bytes: u64,
    pub repaired: DeckImportRepairs,
}
impl From<&PreviewFacts> for DeckImportFacts {
    fn from(facts: &PreviewFacts) -> Self {
        Self {
            format: match facts.format {
                PackageFormat::Latest => DeckPackageFormat::Latest,
                PackageFormat::Legacy2 => DeckPackageFormat::Legacy2,
                PackageFormat::Legacy1 => DeckPackageFormat::Legacy1,
            },
            notes: facts.notes,
            cards: facts.cards,
            decks: facts.decks.clone(),
            notetypes: facts.notetypes.clone(),
            has_scheduling: facts.has_scheduling,
            has_presets: facts.has_presets,
            media_files: facts.media_files,
            media_bytes: facts.media_bytes,
            repaired: DeckImportRepairs {
                note_ids: facts.repaired.note_ids,
                card_ids: facts.repaired.card_ids,
                revlog_ids: facts.repaired.revlog_ids,
                field_counts: facts.repaired.field_counts,
                scheduler_converted: facts.repaired.scheduler_converted,
            },
        }
    }
}

/// I5's three-valued options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckImportUpdate { IfNewer, Always, Never }
impl From<DeckImportUpdate> for UpdateCondition {
    fn from(rule: DeckImportUpdate) -> Self {
        match rule {
            DeckImportUpdate::IfNewer => Self::IfNewer,
            DeckImportUpdate::Always => Self::Always,
            DeckImportUpdate::Never => Self::Never,
        }
    }
}
impl From<UpdateCondition> for DeckImportUpdate {
    fn from(rule: UpdateCondition) -> Self {
        match rule {
            UpdateCondition::IfNewer => Self::IfNewer,
            UpdateCondition::Always => Self::Always,
            UpdateCondition::Never => Self::Never,
        }
    }
}

/// I5: the five options.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportOptions {
    pub merge_notetypes: bool,
    pub update_notes: DeckImportUpdate,
    pub update_notetypes: DeckImportUpdate,
    pub with_scheduling: bool,
    pub with_deck_configs: bool,
}
impl From<DeckImportOptions> for ImportOptions {
    fn from(options: DeckImportOptions) -> Self {
        Self {
            merge_notetypes: options.merge_notetypes,
            update_notes: options.update_notes.into(),
            update_notetypes: options.update_notetypes.into(),
            with_scheduling: options.with_scheduling,
            with_deck_configs: options.with_deck_configs,
        }
    }
}
impl From<ImportOptions> for DeckImportOptions {
    fn from(options: ImportOptions) -> Self {
        Self {
            merge_notetypes: options.merge_notetypes,
            update_notes: options.update_notes.into(),
            update_notetypes: options.update_notetypes.into(),
            with_scheduling: options.with_scheduling,
            with_deck_configs: options.with_deck_configs,
        }
    }
}

/// I5: the five option values as the open collection stores them; a pure
/// read made when the sheet opens (I4).
#[uniffi::export]
pub fn deck_import_stored_options() -> Result<DeckImportOptions, CoreError> {
    callback::enter("deck_import_stored_options");
    let collection = runtime::host()?.collection()?;
    Ok(apkg::read_options(&collection)?.into())
}

/// I14.4: a reason that makes the import a schema change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckImportStructuralReason { SortFieldChanged, NotetypesMerged }
impl From<StructuralReason> for DeckImportStructuralReason {
    fn from(reason: StructuralReason) -> Self {
        match reason {
            StructuralReason::SortFieldChanged => Self::SortFieldChanged,
            StructuralReason::NotetypesMerged => Self::NotetypesMerged,
        }
    }
}

/// The package collection's gate refusal (`SchemaUnsupported`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckImportSchemaGate { TooOld, TooNew, Other }

/// I15.1: the limit a package is past.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckImportLimit {
    Rows,
    GuidBytes,
    NoteBytes,
    TagUses,
    CardDataBytes,
    StructureRows,
    NotetypeParts,
    StructureBytes,
    /// A limit this build's bridge does not name.
    Other,
}
fn limit(what: &str) -> DeckImportLimit {
    match what {
        "rows" => DeckImportLimit::Rows,
        "guid bytes" => DeckImportLimit::GuidBytes,
        "note bytes" => DeckImportLimit::NoteBytes,
        "tag uses" => DeckImportLimit::TagUses,
        "card data bytes" => DeckImportLimit::CardDataBytes,
        "structure rows" => DeckImportLimit::StructureRows,
        "note type parts" => DeckImportLimit::NotetypeParts,
        "structure bytes" => DeckImportLimit::StructureBytes,
        _ => DeckImportLimit::Other,
    }
}

/// I16, one case per engine error. Every case but the last leaves the
/// collection, its media folder and its backups as they were (a published
/// safety backup stays).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckImportRefusal {
    UnsupportedFile,
    /// P18 classified the file a whole collection (`.colpkg`,
    /// `collection.apkg`, `backup-*.apkg`): the collection package flow
    /// imports it, with its replacement confirmation
    /// (`package/file-intake` K5). Nothing was copied.
    CollectionPackage,
    TooNew,
    /// `detail` is for Copy Debug Info, never shown.
    Corrupt { detail: String },
    SchemaUnsupported { gate: DeckImportSchemaGate },
    InsufficientSpace { needed: u64, available: u64 },
    DiskFull,
    Stale,
    NotetypeKindConflict { incoming: i64, target: i64 },
    DeckTooDeep { name: String },
    TooLarge { what: DeckImportLimit, found: u64, limit: u64 },
    SessionClosed,
    Cancelled,
    Io { detail: String },
    Busy { running_op: String },
    CollectionUnavailable { reason: String },
    StaleGeneration,
    SafetyBackupFailed,
    /// Any other engine refusal before the commit.
    Engine { error: CoreError },
    /// The rows are committed and the result cannot be shown: never say
    /// that the collection is unchanged, never import again (X2.6).
    CommittedWithoutResult,
}

fn refusal(error: ApkgImportError) -> DeckImportRefusal {
    use DeckImportRefusal as R;
    match error {
        ApkgImportError::UnsupportedFile => R::UnsupportedFile,
        ApkgImportError::TooNew => R::TooNew,
        ApkgImportError::Corrupt { detail } => R::Corrupt { detail },
        ApkgImportError::SchemaUnsupported { gate } => R::SchemaUnsupported {
            gate: match *gate {
                OpenError::TooOld { .. } => DeckImportSchemaGate::TooOld,
                OpenError::TooNew { .. } => DeckImportSchemaGate::TooNew,
                _ => DeckImportSchemaGate::Other,
            },
        },
        ApkgImportError::InsufficientSpace { needed, available } => R::InsufficientSpace { needed, available },
        ApkgImportError::DiskFull => R::DiskFull,
        ApkgImportError::Stale => R::Stale,
        // Both are outcomes of their own; a caller that reaches this with
        // one lost its result.
        ApkgImportError::ConfirmationRequired { .. } => R::Engine {
            error: CoreError::Internal { message: "an import confirmation was read as a refusal".into() },
        },
        ApkgImportError::MediaCommitPending(_) => R::CommittedWithoutResult,
        ApkgImportError::NotetypeKindConflict { incoming, target } => R::NotetypeKindConflict { incoming, target },
        ApkgImportError::DeckTooDeep { name } => R::DeckTooDeep { name },
        ApkgImportError::TooLarge { what, found, limit: most } => R::TooLarge { what: limit(what), found, limit: most },
        ApkgImportError::SessionClosed => R::SessionClosed,
        ApkgImportError::Cancelled => R::Cancelled,
        ApkgImportError::Io { detail } if detail == apkg::COMMITTED_WITHOUT_RESULT => R::CommittedWithoutResult,
        ApkgImportError::Io { detail } => R::Io { detail },
        ApkgImportError::Engine(error) => match *error {
            EngineError::Busy { running_op } => R::Busy { running_op },
            EngineError::CollectionUnavailable { reason } => R::CollectionUnavailable { reason },
            EngineError::StaleGeneration { .. } => R::StaleGeneration,
            EngineError::SafetyBackupFailed(_) => R::SafetyBackupFailed,
            EngineError::Cancelled => R::Cancelled,
            EngineError::CommitCompleted { .. } | EngineError::MediaCommitPending { .. } => R::CommittedWithoutResult,
            other => R::Engine { error: other.into() },
        },
    }
}

/// I14.5's counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportCounts {
    pub notes_added: u64,
    pub notes_updated: u64,
    pub cards_added: u64,
    pub revlog_added: u64,
    pub notetypes_added: u64,
    pub notetypes_updated: u64,
    pub notetypes_copied: u64,
    pub notes_moved: u64,
    pub notetypes_removed: u64,
    pub decks_added: u64,
    pub decks_updated: u64,
    pub presets_added: u64,
    pub media_written: u64,
    pub media_reused: u64,
    pub media_skipped: u64,
}
impl From<ImportCounts> for DeckImportCounts {
    fn from(counts: ImportCounts) -> Self {
        Self {
            notes_added: counts.notes_added,
            notes_updated: counts.notes_updated,
            cards_added: counts.cards_added,
            revlog_added: counts.revlog_added,
            notetypes_added: counts.notetypes_added,
            notetypes_updated: counts.notetypes_updated,
            notetypes_copied: counts.notetypes_copied,
            notes_moved: counts.notes_moved,
            notetypes_removed: counts.notetypes_removed,
            decks_added: counts.decks_added,
            decks_updated: counts.decks_updated,
            presets_added: counts.presets_added,
            media_written: counts.media_written,
            media_reused: counts.media_reused,
            media_skipped: counts.media_skipped,
        }
    }
}

/// I13: how many entries each category of the log holds. The entries are
/// read in pages (`DeckImportSession::log_page`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportLogCounts {
    pub found_notes: u64,
    /// Entries that carry no field text (the log's text budget).
    pub fields_omitted: u64,
    pub new: u64,
    pub updated: u64,
    pub duplicate: u64,
    pub conflicting: u64,
    pub missing_notetype: u64,
    pub missing_deck: u64,
    pub first_field_match: u64,
    pub empty_first_field: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DeckImportLogCategory {
    New,
    Updated,
    Duplicate,
    Conflicting,
    MissingNotetype,
    MissingDeck,
    FirstFieldMatch,
    EmptyFirstField,
}
impl From<DeckImportLogCategory> for LogCategory {
    fn from(category: DeckImportLogCategory) -> Self {
        match category {
            DeckImportLogCategory::New => Self::New,
            DeckImportLogCategory::Updated => Self::Updated,
            DeckImportLogCategory::Duplicate => Self::Duplicate,
            DeckImportLogCategory::Conflicting => Self::Conflicting,
            DeckImportLogCategory::MissingNotetype => Self::MissingNotetype,
            DeckImportLogCategory::MissingDeck => Self::MissingDeck,
            DeckImportLogCategory::FirstFieldMatch => Self::FirstFieldMatch,
            DeckImportLogCategory::EmptyFirstField => Self::EmptyFirstField,
        }
    }
}

/// One entry of the log (I13).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportLogEntry {
    /// The target note's id, or the package note's for `Conflicting` and
    /// `MissingNotetype`.
    pub note_id: i64,
    /// Empty once the log's text budget is spent.
    pub fields: Vec<String>,
    /// For `Conflicting`: the target note that holds the GUID.
    pub existing_id: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportLayout {
    pub notetype_id: i64,
    /// Names changed, positions kept.
    pub identity: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportTypeChange {
    pub from: i64,
    pub to: i64,
}
/// What the op reported about note type structure (I14.4).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportStructure {
    /// Empty for an import that set no `scm`.
    pub reasons: Vec<DeckImportStructuralReason>,
    pub layouts: Vec<DeckImportLayout>,
    pub removed: Vec<i64>,
    pub type_changes: Vec<DeckImportTypeChange>,
}

/// I14.5: what a committed import reports.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportResult {
    /// The op's seq; the undo step's id.
    pub seq: u64,
    pub generation: u64,
    /// The undo step's name, when the import changed something.
    pub undo_step: Option<String>,
    pub counts: DeckImportCounts,
    pub log: DeckImportLogCounts,
    pub structure: DeckImportStructure,
    /// The safety backup's name, when one was taken or reused.
    pub safety_backup: Option<String>,
    /// The rows are committed and some media files are still being put in
    /// place by the journal's recovery: never import again.
    pub media_pending: bool,
}
impl From<&ImportResult> for DeckImportResult {
    fn from(result: &ImportResult) -> Self {
        let log = &result.log;
        let count = |entries: &Vec<apkg::LogEntry>| entries.len() as u64;
        Self {
            seq: result.seq,
            generation: result.generation,
            undo_step: result.undo_step.map(str::to_owned),
            counts: result.counts.into(),
            log: DeckImportLogCounts {
                found_notes: log.found_notes,
                fields_omitted: log.fields_omitted,
                new: count(&log.new),
                updated: count(&log.updated),
                duplicate: count(&log.duplicate),
                conflicting: count(&log.conflicting),
                missing_notetype: count(&log.missing_notetype),
                missing_deck: count(&log.missing_deck),
                first_field_match: count(&log.first_field_match),
                empty_first_field: count(&log.empty_first_field),
            },
            structure: DeckImportStructure {
                reasons: result.structure.reasons.iter().filter_map(|code| match code {
                    3 => Some(DeckImportStructuralReason::SortFieldChanged),
                    9 => Some(DeckImportStructuralReason::NotetypesMerged),
                    _ => None,
                }).collect(),
                layouts: result.structure.layouts.iter()
                    .map(|(notetype_id, identity)| DeckImportLayout { notetype_id: *notetype_id, identity: *identity }).collect(),
                removed: result.structure.removed.clone(),
                type_changes: result.structure.type_changes.iter()
                    .map(|(from, to)| DeckImportTypeChange { from: *from, to: *to }).collect(),
            },
            safety_backup: result.safety_backup.clone(),
            media_pending: result.media_pending,
        }
    }
}

/// J10: one field position whose content the import would delete.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportEmptiedField {
    pub notetype_id: i64,
    pub notetype_name: String,
    pub position: u32,
    pub field_name: String,
    pub notes: u64,
}
/// J10: a note type the merge removes.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportRemovedNotetype {
    pub notetype_id: i64,
    pub name: String,
    pub moved_to: i64,
    /// The open collection's name for `moved_to`, when it has that note type.
    pub moved_to_name: Option<String>,
    pub notes: u64,
}
/// J10: notes storing more values than their note type has fields.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportDroppedValues {
    pub notetype_id: i64,
    /// The open collection's name for the note type, when it has it.
    pub notetype_name: Option<String>,
    pub notes: u64,
}
/// J10: what the import would delete, in the preview's order.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DeckImportPreview {
    pub emptied_fields: Vec<DeckImportEmptiedField>,
    pub removed_notetypes: Vec<DeckImportRemovedNotetype>,
    pub dropped_values: Vec<DeckImportDroppedValues>,
}

/// I8's token: proof of what a confirmation showed. Opaque.
#[derive(Debug, uniffi::Object)]
pub struct DeckImportConfirmation {
    token: ConfirmationToken,
}

/// I8: the apply wrote nothing and asks first.
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckImportConfirmationRequest {
    /// Possibly empty when `reasons` is not.
    pub preview: DeckImportPreview,
    /// The plan's structural reasons, for a caller that asked to confirm
    /// structure (MP7.1); empty otherwise.
    pub reasons: Vec<DeckImportStructuralReason>,
    /// Pass it to the next apply with the same options.
    pub confirmation: Arc<DeckImportConfirmation>,
}

#[allow(clippy::large_enum_variant)] // An owned boundary value; the common case is the result.
#[derive(Clone, Debug, uniffi::Enum)]
pub enum DeckImportApplyOutcome {
    /// Committed. With `media_pending` the engine reported
    /// `MediaCommitPending`: the rows are in, and the import never repeats.
    Imported { result: DeckImportResult },
    ConfirmationRequired { request: DeckImportConfirmationRequest },
    Refused { refusal: DeckImportRefusal },
}

/// The names the preview lacks, read from the open collection (X2.3).
fn preview(preview: &MergePreview) -> DeckImportPreview {
    let collection = runtime::host().ok().and_then(|host| host.collection().ok());
    let reader = collection.as_ref().and_then(|collection| collection.reader(ReaderRole::Query).ok());
    let name = |id: i64| {
        reader.as_ref().and_then(|reader| reader.notetype(NotetypeId(id)).ok().flatten()).map(|notetype| notetype.name)
    };
    DeckImportPreview {
        emptied_fields: preview.emptied_fields.iter().map(|entry| DeckImportEmptiedField {
            notetype_id: entry.notetype, notetype_name: entry.notetype_name.clone(), position: entry.position,
            field_name: entry.field_name.clone(), notes: entry.notes }).collect(),
        removed_notetypes: preview.removed_notetypes.iter().map(|entry| DeckImportRemovedNotetype {
            notetype_id: entry.notetype, name: entry.name.clone(), moved_to: entry.moved_to,
            moved_to_name: name(entry.moved_to), notes: entry.notes }).collect(),
        dropped_values: preview.dropped_values.iter().map(|entry| DeckImportDroppedValues {
            notetype_id: entry.notetype, notetype_name: name(entry.notetype), notes: entry.notes }).collect(),
    }
}

/// The apply's answer as the sheet reads it.
pub(crate) fn apply_outcome(answer: Result<ImportResult, ApkgImportError>) -> DeckImportApplyOutcome {
    match answer {
        Ok(result) => DeckImportApplyOutcome::Imported { result: (&result).into() },
        Err(ApkgImportError::MediaCommitPending(result)) => DeckImportApplyOutcome::Imported { result: (&*result).into() },
        Err(ApkgImportError::ConfirmationRequired { preview: shown, reasons, token }) =>
            DeckImportApplyOutcome::ConfirmationRequired { request: DeckImportConfirmationRequest {
                preview: preview(&shown),
                reasons: reasons.into_iter().map(Into::into).collect(),
                confirmation: Arc::new(DeckImportConfirmation { token: *token }),
            } },
        Err(error) => DeckImportApplyOutcome::Refused { refusal: refusal(error) },
    }
}

/// Every live session, so that closing the collection ends them (I2).
static SESSIONS: Mutex<Vec<Weak<DeckImportSession>>> = Mutex::new(Vec::new());

/// I2: a session whose engine closed is closed, and its staging removed.
pub(crate) fn close_sessions() {
    let sessions: Vec<Arc<DeckImportSession>> = {
        let mut known = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
        let live = known.iter().filter_map(Weak::upgrade).collect();
        known.clear();
        live
    };
    for session in sessions { session.end(); }
}

/// An inspected deck package (I2). It commits at most once.
#[derive(Debug, uniffi::Object)]
pub struct DeckImportSession {
    inner: Mutex<Option<Arc<ImportSession>>>,
}
impl DeckImportSession {
    pub(crate) fn register(session: ImportSession) -> Arc<Self> {
        let session = Arc::new(Self { inner: Mutex::new(Some(Arc::new(session))) });
        let mut known = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
        known.retain(|other| other.strong_count() > 0);
        known.push(Arc::downgrade(&session));
        session
    }
    /// The engine's session, while this one is open.
    pub(crate) fn shared(&self) -> Option<Arc<ImportSession>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
    fn end(&self) {
        // A running apply keeps its own reference: the staging directory
        // goes when that apply ends.
        // Taken out first: the staging is never removed under the lock.
        let session = self.inner.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(session) = session { session.end(); }
    }
}
impl Drop for DeckImportSession {
    /// Letting the object go is `close`: an apply still queued is refused.
    fn drop(&mut self) {
        self.end();
    }
}
impl DeckImportConfirmation {
    pub(crate) fn token(&self) -> ConfirmationToken { self.token.clone() }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum DeckImportInspectOutcome {
    Inspected { session: Arc<DeckImportSession>, facts: DeckImportFacts },
    /// The session's staging is removed; the selected file is untouched.
    Refused { refusal: DeckImportRefusal },
}
#[derive(Debug, uniffi::Object)]
pub struct DeckImportInspectJob {
    outcome: Arc<Mutex<Option<DeckImportInspectOutcome>>>,
}
#[uniffi::export]
impl DeckImportInspectJob {
    /// The outcome once the job finished; none while it runs. Taken once:
    /// an outcome nobody takes closes its session with this object.
    pub fn outcome(&self) -> Result<Option<DeckImportInspectOutcome>, CoreError> {
        callback::enter("DeckImportInspectJob::outcome");
        Ok(self.outcome.lock().unwrap_or_else(PoisonError::into_inner).take())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckImportInspectStart {
    pub handle: JobHandle,
    pub result: Arc<DeckImportInspectJob>,
}

/// I2 to I4 as a job: the owned source copy and the collection entry
/// (stage `extracting`), then the gate, the repairs and the facts (stage
/// `analysing`). It reads `path` only during `extracting`, never writes
/// it, and never reads the open collection. Cancelling removes the staging.
#[uniffi::export]
pub fn start_deck_import_inspect(path: String, name: String) -> Result<DeckImportInspectStart, CoreError> {
    callback::enter("start_deck_import_inspect");
    let host = runtime::host()?;
    let context = host.deck_package_context();
    let outcome = Arc::new(Mutex::new(None));
    let out = outcome.clone();
    let handle = jobs::start("deck package inspect", runtime::engine()?.generation().0, move |job| {
        // K5: a whole-collection name never becomes a deck import, whatever
        // the caller's own routing did.
        let whole = Path::new(&path).is_file() && apkg::classify(Path::new(&path), &name) == PackageClass::Colpkg;
        let inspected = if whole { None } else {
            Some(apkg::inspect(Path::new(&path), &name, &context, job.cancel.engine_token(), &job.progress.engine_sink()))
        };
        let cancelled = matches!(inspected, Some(Err(ApkgImportError::Cancelled)));
        let value = match inspected {
            None => DeckImportInspectOutcome::Refused { refusal: DeckImportRefusal::CollectionPackage },
            Some(Ok(session)) => {
                let facts = DeckImportFacts::from(session.facts());
                DeckImportInspectOutcome::Inspected { session: DeckImportSession::register(session), facts }
            }
            Some(Err(error)) => DeckImportInspectOutcome::Refused { refusal: refusal(error) },
        };
        *out.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
        if cancelled { Err(CoreError::Cancelled) } else { Ok(()) }
    })?;
    Ok(DeckImportInspectStart { handle, result: Arc::new(DeckImportInspectJob { outcome }) })
}

#[derive(Debug, uniffi::Object)]
pub struct DeckImportApplyJob {
    outcome: Arc<Mutex<Option<DeckImportApplyOutcome>>>,
}
#[uniffi::export]
impl DeckImportApplyJob {
    /// The outcome once the job finished; none while it runs.
    pub fn outcome(&self) -> Result<Option<DeckImportApplyOutcome>, CoreError> {
        callback::enter("DeckImportApplyJob::outcome");
        Ok(self.outcome.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeckImportApplyStart {
    pub handle: JobHandle,
    pub result: Arc<DeckImportApplyJob>,
}

#[uniffi::export]
impl DeckImportSession {
    /// I14 as a job, for an import that runs alone: phase `Planning`, the
    /// stages `media` and `backup`, then `Committing`, which cannot be
    /// cancelled. `confirmation` is the one a previous apply's
    /// `ConfirmationRequired` returned. A refusal or a cancel before the
    /// commit leaves the session open for another apply.
    ///
    /// `ask_structure` is true while a ready Meguri Sync binding would have
    /// to upload the collection after a structural import (MP7.1). This
    /// call then never commits a plan with a structural reason, whatever
    /// `confirmation` covers: it answers `ConfirmationRequired` with the
    /// reasons, and the accepted confirmation goes to the structure job
    /// (`ProviderStructureOp::Import`). A plan without one commits here.
    pub fn start_apply(&self, options: DeckImportOptions, confirmation: Option<Arc<DeckImportConfirmation>>,
        ask_structure: bool) -> Result<DeckImportApplyStart, CoreError> {
        callback::enter("DeckImportSession::start_apply");
        let engine = runtime::engine()?;
        // The caller's reading of the service can be stale: a ready Meguri
        // Sync binding always asks, so this call never commits a structural
        // plan that the binding would then have to upload.
        let ask_structure = ask_structure
            || matches!(runtime::host()?.meguri_readiness(), Ok(meguri_sync::provider::MeguriReadiness::Ready));
        let session = self.shared();
        let token = confirmation.map(|confirmation| {
            if ask_structure { confirmation.token.without_structure() } else { confirmation.token() }
        });
        let outcome = Arc::new(Mutex::new(None));
        let out = outcome.clone();
        let handle = engine.start_job(JobKind::BulkOp, move |job| {
            let answer = match &session {
                Some(session) => apkg::apply_confirming(engine, session, options.into(), token.as_ref(), ask_structure,
                    &job.cancel, &job.progress),
                None => Err(ApkgImportError::SessionClosed),
            };
            let cancelled = matches!(answer, Err(ApkgImportError::Cancelled));
            *out.lock().unwrap_or_else(PoisonError::into_inner) = Some(apply_outcome(answer));
            if cancelled { Err(EngineError::Cancelled) } else { Ok(()) }
        })?;
        Ok(DeckImportApplyStart { handle: jobs::wrap(handle), result: Arc::new(DeckImportApplyJob { outcome }) })
    }

    /// The committed import's result, for a sheet that lost it; none before
    /// a commit or after `close`.
    pub fn result(&self) -> Result<Option<DeckImportResult>, CoreError> {
        callback::enter("DeckImportSession::result");
        Ok(self.shared().and_then(|session| session.result()).map(|result| (&result).into()))
    }

    /// A page of one category of the committed import's log (I13), in the
    /// log's order. Empty before a commit, after `close` and past the end.
    pub fn log_page(&self, category: DeckImportLogCategory, offset: u64, count: u32)
        -> Result<Vec<DeckImportLogEntry>, CoreError> {
        callback::enter("DeckImportSession::log_page");
        let offset = usize::try_from(offset).unwrap_or(usize::MAX);
        let page = self.shared().and_then(|session| session.log_page(category.into(), offset, count as usize));
        Ok(page.unwrap_or_default().into_iter().map(|entry| DeckImportLogEntry {
            note_id: entry.note, fields: entry.fields, existing_id: entry.existing_id }).collect())
    }

    /// Ends the session and removes its staging directory (I2). Idempotent.
    /// Dropping the object does the same.
    pub fn close(&self) -> Result<(), CoreError> {
        callback::enter("DeckImportSession::close");
        self.end();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_engine::Generation;
    use meguri_package::apkg::{ImportLog, StructureSummary};

    fn engine(error: EngineError) -> ApkgImportError { ApkgImportError::Engine(Box::new(error)) }

    /// I16: every case of the engine's import error has its own refusal,
    /// and nothing the sheet must parse.
    #[test]
    fn every_import_error_has_its_own_typed_refusal() {
        use DeckImportRefusal as R;
        let gate = |gate| ApkgImportError::SchemaUnsupported { gate: Box::new(gate) };
        let cases = [
            (ApkgImportError::UnsupportedFile, R::UnsupportedFile),
            (ApkgImportError::TooNew, R::TooNew),
            (ApkgImportError::Corrupt { detail: "d".into() }, R::Corrupt { detail: "d".into() }),
            (gate(OpenError::TooOld { version: 9 }), R::SchemaUnsupported { gate: DeckImportSchemaGate::TooOld }),
            (gate(OpenError::TooNew { version: 19 }), R::SchemaUnsupported { gate: DeckImportSchemaGate::TooNew }),
            (ApkgImportError::InsufficientSpace { needed: 9, available: 4 }, R::InsufficientSpace { needed: 9, available: 4 }),
            (ApkgImportError::DiskFull, R::DiskFull),
            (ApkgImportError::Stale, R::Stale),
            (ApkgImportError::NotetypeKindConflict { incoming: 5, target: 6 }, R::NotetypeKindConflict { incoming: 5, target: 6 }),
            (ApkgImportError::DeckTooDeep { name: "a::b".into() }, R::DeckTooDeep { name: "a::b".into() }),
            (ApkgImportError::SessionClosed, R::SessionClosed),
            (ApkgImportError::Cancelled, R::Cancelled),
            (ApkgImportError::Io { detail: "x".into() }, R::Io { detail: "x".into() }),
            (ApkgImportError::Io { detail: apkg::COMMITTED_WITHOUT_RESULT.into() }, R::CommittedWithoutResult),
            (engine(EngineError::Busy { running_op: "Check Database".into() }), R::Busy { running_op: "Check Database".into() }),
            (engine(EngineError::CollectionUnavailable { reason: "closed".into() }), R::CollectionUnavailable { reason: "closed".into() }),
            (engine(EngineError::StaleGeneration { planned: Generation(1), current: Generation(2) }), R::StaleGeneration),
            (engine(EngineError::CommitCompleted { seq: meguri_engine::Seq(4), message: "m".into() }), R::CommittedWithoutResult),
            (engine(EngineError::MediaCommitPending { seq: meguri_engine::Seq(4), message: "m".into() }), R::CommittedWithoutResult),
            (engine(EngineError::Internal { message: "bug".into() }), R::Engine { error: CoreError::Internal { message: "bug".into() } }),
            (engine(EngineError::Cancelled), R::Cancelled),
            (ApkgImportError::MediaCommitPending(Box::new(result(true))), R::CommittedWithoutResult),
        ];
        for (error, expected) in cases { assert_eq!(refusal(error.clone()), expected, "{error:?}"); }
        // Each limit of I15.1 by its name.
        for (what, expected) in [("rows", DeckImportLimit::Rows), ("guid bytes", DeckImportLimit::GuidBytes),
            ("note bytes", DeckImportLimit::NoteBytes), ("tag uses", DeckImportLimit::TagUses),
            ("card data bytes", DeckImportLimit::CardDataBytes), ("structure rows", DeckImportLimit::StructureRows),
            ("note type parts", DeckImportLimit::NotetypeParts), ("structure bytes", DeckImportLimit::StructureBytes)] {
            assert_eq!(refusal(ApkgImportError::TooLarge { what, found: 3, limit: 2 }),
                R::TooLarge { what: expected, found: 3, limit: 2 });
        }
    }

    fn result(pending: bool) -> ImportResult {
        ImportResult { seq: 7, generation: 2, undo_step: Some("Import"), log: ImportLog { found_notes: 3, ..ImportLog::default() },
            counts: ImportCounts { notes_added: 3, media_skipped: 1, ..ImportCounts::default() },
            structure: StructureSummary { reasons: vec![3, 8, 9], layouts: vec![(5, true)], removed: vec![6], type_changes: vec![(6, 5)] },
            safety_backup: Some("backup-name".into()), media_pending: pending }
    }

    /// The apply's three outcomes: a result (also after `MediaCommitPending`,
    /// never as a refusal), the confirmation with its reasons, a refusal.
    #[test]
    fn the_apply_answer_crosses_as_a_result_a_confirmation_or_a_refusal() {
        let DeckImportApplyOutcome::Imported { result: plain } = apply_outcome(Ok(result(false))) else { panic!("a result") };
        assert_eq!((plain.seq, plain.undo_step.as_deref(), plain.counts.notes_added, plain.log.found_notes), (7, Some("Import"), 3, 3));
        assert_eq!(plain.structure.reasons, [DeckImportStructuralReason::SortFieldChanged, DeckImportStructuralReason::NotetypesMerged]);
        assert_eq!(plain.structure.layouts, [DeckImportLayout { notetype_id: 5, identity: true }]);
        assert_eq!(plain.structure.type_changes, [DeckImportTypeChange { from: 6, to: 5 }]);
        assert!(!plain.media_pending && plain.safety_backup.as_deref() == Some("backup-name"));
        let pending = apply_outcome(Err(ApkgImportError::MediaCommitPending(Box::new(result(true)))));
        assert!(matches!(pending, DeckImportApplyOutcome::Imported { result } if result.media_pending && result.counts.media_skipped == 1));
        assert!(matches!(apply_outcome(Err(ApkgImportError::Stale)),
            DeckImportApplyOutcome::Refused { refusal: DeckImportRefusal::Stale }));
    }
}
