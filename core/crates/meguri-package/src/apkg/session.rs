//! The import session (`package/apkg-import` I2 to I4): the owned source
//! copy, the staged and repaired package collection, the media map and the
//! preview facts. Nothing outside the session's staging directory is
//! written here, and the open collection is never read.
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use meguri_engine::{CancelToken, ProgressPhase, ProgressSink};
use meguri_model::proto::deck::DeckKind;
use meguri_storage::ReadRows;
use meguri_storage::deck_package::{self, DeckCandidateRepair};
use meguri_storage::package_inspection::{CandidateContext, CandidateError};

use super::archive::{DeckArchive, MediaEntry, PackageFormat};
use super::classify::{PackageClass, classify};
use super::{ApkgContext, ApkgImportError, ImportOutcome, ImportResult, PreviewFacts, Repaired, corrupt};
use crate::cancelled;

const MARGIN: u64 = 4 * 1024 * 1024;
/// The fixed names inside the session's staging directory (I2).
const SOURCE_COPY: &str = "source.apkg";
const COLLECTION: &str = "collection.anki2";

pub(crate) struct SessionState {
    /// Set by a commit: the session never applies again (I2).
    pub result: Option<ImportResult>,
    pub closed: bool,
}

/// An inspected deck package, ready for one [`super::apply`]. Dropping it
/// removes its staging directory.
pub struct ImportSession {
    pub(crate) id: u64,
    pub(crate) context: ApkgContext,
    staging: tempfile::TempDir,
    format: PackageFormat,
    pub(crate) media: Vec<MediaEntry>,
    facts: PreviewFacts,
    pub(crate) state: Mutex<SessionState>,
}

impl std::fmt::Debug for ImportSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImportSession").field("id", &self.id).finish_non_exhaustive()
    }
}

impl ImportSession {
    pub fn facts(&self) -> &PreviewFacts {
        &self.facts
    }

    pub fn format(&self) -> PackageFormat {
        self.format
    }

    /// The committed import's result, for paging its log (I13).
    pub fn result(&self) -> Option<ImportResult> {
        self.lock().result.clone()
    }

    /// Ends the session and removes its staging directory (I2).
    pub fn close(self) {
        self.lock().closed = true;
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, SessionState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn source_path(&self) -> PathBuf {
        self.staging.path().join(SOURCE_COPY)
    }

    pub(crate) fn collection_path(&self) -> PathBuf {
        self.staging.path().join(COLLECTION)
    }

    pub(crate) fn staging_path(&self) -> &Path {
        self.staging.path()
    }

    /// The size of the staged package collection, for I15's media preflight.
    pub(crate) fn collection_size(&self) -> u64 {
        std::fs::metadata(self.collection_path()).map_or(0, |m| m.len())
    }
}

fn preflight(context: &ApkgContext, needed: u64) -> ImportOutcome<()> {
    meguri_storage::space::require_space(context.space.as_ref(), &context.collection_directory, needed).map_err(
        |error| match error {
            meguri_storage::StorageError::InsufficientSpace { required, available } => {
                ApkgImportError::InsufficientSpace {
                    needed: required,
                    available,
                }
            }
            other => other.into(),
        },
    )
}

fn candidate_error(error: CandidateError) -> ApkgImportError {
    use meguri_storage::OpenError;
    match error {
        CandidateError::Gate(gate @ (OpenError::TooOld { .. } | OpenError::TooNew { .. })) => {
            ApkgImportError::SchemaUnsupported { gate: Box::new(gate) }
        }
        CandidateError::Gate(OpenError::Io { detail }) => ApkgImportError::Io { detail },
        CandidateError::Gate(gate) => corrupt(format!("candidate gate: {gate}")),
        CandidateError::Storage(error) => match error {
            meguri_storage::StorageError::Sqlite { .. } | meguri_storage::StorageError::Row(_) => {
                corrupt(format!("candidate: {error}"))
            }
            other => other.into(),
        },
        CandidateError::Corrupt { detail } => corrupt(detail),
        CandidateError::InsufficientSpace { needed, available } => {
            ApkgImportError::InsufficientSpace { needed, available }
        }
        CandidateError::DiskFull => ApkgImportError::DiskFull,
        CandidateError::Cancelled => ApkgImportError::Cancelled,
    }
}

/// I2 and I3: copies `source` into a fresh staging directory, reads and
/// checks the package there and returns the session with I4's facts.
/// `name` is the file's name as the user saw it (P18). The caller keeps the
/// file's security scope and coordinated read until this returns
/// (`package/file-intake` K3). A failure or a cancel removes the staging
/// directory; the selected file is never changed.
pub fn inspect(
    source: &Path,
    name: &str,
    context: &ApkgContext,
    cancel: &CancelToken,
    progress: &ProgressSink,
) -> ImportOutcome<ImportSession> {
    let check = || cancel.is_cancelled();
    let check: &(dyn Fn() -> bool + Sync) = &check;
    cancelled(check)?;
    if classify(source, name) == PackageClass::Unsupported {
        return Err(ApkgImportError::UnsupportedFile);
    }
    progress.set_phase(ProgressPhase::Named("extracting"));
    let mut input = File::open(source)?;
    if !input.metadata()?.is_file() {
        return Err(ApkgImportError::UnsupportedFile);
    }
    let size = input.metadata()?.len();
    preflight(context, size.checked_add(MARGIN).ok_or_else(|| corrupt("source size"))?)?;
    let staging = meguri_storage::leftovers::staging_directory(&context.collection_directory)?;
    let copy = staging.path().join(SOURCE_COPY);
    {
        let mut output = File::options().write(true).create_new(true).open(&copy)?;
        let mut buffer = [0; 64 * 1024];
        let mut copied = 0_u64;
        loop {
            cancelled(check)?;
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            copied += read as u64;
            if copied > size {
                return Err(corrupt("source changed during the copy"));
            }
            output.write_all(&buffer[..read]).map_err(disk)?;
            progress.report(copied, Some(size));
        }
        if copied != size {
            return Err(corrupt("source changed during the copy"));
        }
        output.flush().map_err(disk)?;
    }
    drop(input);

    // Step 1: archive and format. Step 2: preflight and extraction.
    let mut archive = DeckArchive::open(&copy, check)?;
    let format = archive.format;
    let estimate = archive.collection_estimate(0)?;
    preflight(context, estimate.checked_add(MARGIN).ok_or_else(|| corrupt("inspect preflight"))?)?;
    let collection = staging.path().join(COLLECTION);
    {
        let mut output = File::options().write(true).create_new(true).open(&collection)?;
        archive.extract_collection(&mut output, check).map_err(|error| match error {
            crate::Error::Io { detail } if detail.contains("No space left") => ApkgImportError::DiskFull,
            other => other.into(),
        })?;
    }
    cancelled(check)?;

    // Steps 3 to 7 on the staged copy.
    progress.set_phase(ProgressPhase::Named("analysing"));
    let clock = context.clock.clone();
    let minutes_west = move |at: i64| meguri_sched::timing::minutes_west(clock.utc_offset_at(at.saturating_mul(1000)));
    let repair_fields = |mut values: Vec<String>, count: usize| {
        // F1 never fails for a count above zero, which the caller checked.
        let _ = meguri_engine::notes::fields::repair(&mut values, count);
        values
    };
    let prepared = deck_package::prepare_deck_candidate(
        &collection,
        &CandidateContext {
            clock: context.clock.as_ref(),
            random: context.random.as_ref(),
            space: context.space.as_ref(),
            is_cancelled: check,
        },
        &DeckCandidateRepair {
            minutes_west_at: &minutes_west,
            repair_fields: &repair_fields,
        },
    )
    .map_err(candidate_error)?;

    // Step 8: the media map.
    let media = archive.media_map(check)?;
    drop(archive);
    cancelled(check)?;

    // Step 9: the facts, from the package collection as the repairs left it.
    let opened = deck_package::open_deck_candidate(&collection, context.clock.as_ref(), context.random.as_ref())
        .map_err(|gate| corrupt(format!("candidate gate: {gate}")))?;
    let facts = (|| -> ImportOutcome<PreviewFacts> {
        let refs = deck_package::deck_candidate_refs(&opened)?;
        let rows = opened.writer();
        let decks = rows.deck_rows().map_err(unreadable)?;
        let mut named: Vec<&meguri_model::deck::Deck> = Vec::new();
        for id in &refs.decks {
            let Some(deck) = decks.iter().find(|deck| deck.id.0 == *id) else {
                continue;
            };
            let mut next = Some(deck);
            while let Some(found) = next {
                if named.iter().any(|known| known.id == found.id) {
                    break;
                }
                named.push(found);
                next = meguri_engine::decks::names::parent(&found.name)
                    .and_then(|parent| meguri_engine::decks::names::find(&decks, parent));
            }
        }
        let has_presets = named
            .iter()
            .any(|deck| matches!(&deck.kind.kind, DeckKind::Normal(normal) if normal.config_id != 1));
        let mut names: Vec<String> = named.iter().map(|deck| deck.human_name()).collect();
        names.sort_by(|a, b| meguri_text::fold::compare_folded(a, b));
        let mut notetypes = Vec::new();
        for id in &refs.notetypes {
            if let Some(notetype) = rows.notetype(meguri_model::ids::NotetypeId(*id)).map_err(unreadable)? {
                notetypes.push(notetype.name);
            }
        }
        Ok(PreviewFacts {
            format,
            notes: prepared.notes,
            cards: prepared.cards,
            decks: names,
            notetypes,
            has_scheduling: refs.studied_card || refs.revlog_row,
            has_presets,
            media_files: media.len() as u64,
            media_bytes: media.iter().map(|entry| entry.size).sum(),
            repaired: Repaired {
                note_ids: prepared.new_note_ids,
                card_ids: prepared.new_card_ids,
                revlog_ids: prepared.new_revlog_ids,
                field_counts: prepared.repaired_field_counts,
                scheduler_converted: prepared.scheduler_converted,
            },
        })
    })();
    let closed = opened.close();
    let facts = facts?;
    closed?;
    cancelled(check)?;
    Ok(ImportSession {
        id: context.random.next_u64(),
        context: context.clone(),
        staging,
        format,
        media,
        facts,
        state: Mutex::new(SessionState {
            result: None,
            closed: false,
        }),
    })
}

/// A package row the typed readers refuse is a damaged package.
pub(crate) fn unreadable(error: meguri_storage::StorageError) -> ApkgImportError {
    match error {
        meguri_storage::StorageError::Row(_) | meguri_storage::StorageError::Sqlite { .. } => {
            corrupt(format!("package collection: {error}"))
        }
        other => other.into(),
    }
}

fn disk(error: std::io::Error) -> ApkgImportError {
    if error.kind() == std::io::ErrorKind::StorageFull {
        ApkgImportError::DiskFull
    } else {
        error.into()
    }
}
