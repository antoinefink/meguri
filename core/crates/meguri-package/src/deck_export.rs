//! Production latest-format deck export (package/apkg-export A1–A12).
//! Preparation builds the fresh database from one pinned selection, then ends
//! the read transaction before media inventory, capture and publication.
use crate::{
    CollectionPackageWriteContext, Error, LiveCollectionPackageWritten, MediaCollectionExportWritten, Result,
    cancelled,
    export_capture::{self, ExportSourceValidation},
    writer,
};
use meguri_model::{
    clock::FixedClock,
    col::Col,
    config::ConfigEntry,
    ids::{CardId, DeckId, NoteId, RevlogId},
    proto::{
        ProtoBlob,
        deck::{DeckCommon, DeckKind, DeckKindMessage, NormalDeck},
    },
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_storage::{
    CollectionBuilder, OpenOptions, ReadRows, ReadSnapshot, Reader, StampMode,
    deck_export::{DeckExportReadError, DeckExportSelection},
    leftovers::staging_directory,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// Counts from the pinned selection actually written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeckExportCounts {
    pub notes: u64,
    pub cards: u64,
    pub media: u64,
}
#[derive(Clone, Copy)]
pub struct DeckExportOptions {
    pub include_scheduling: bool,
    pub include_presets: bool,
    pub include_media: bool,
}
pub struct DeckExportPlan {
    staging: tempfile::TempDir,
    database: PathBuf,
    snapshot_modified: EpochMillis,
    name: String,
    notes: u64,
    cards: u64,
    reader_identity: std::sync::Arc<()>,
    source: PathBuf,
    source_identity: (u64, u64),
    directory: PathBuf,
    directory_identity: (u64, u64),
    destination: PathBuf,
    validation: ExportSourceValidation,
}
impl DeckExportPlan {
    pub fn protect_retained_outputs(&mut self, paths: &[PathBuf], check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
        for retained in paths {
            cancelled(check)?;
            for suffix in ["", "-wal", "-shm", "-journal"] {
                let mut source = self.source.as_os_str().to_owned();
                source.push(suffix);
                writer::check_source_alias(Path::new(&source), retained)?;
            }
            for name in [
                "meguri-media.db",
                "meguri-media.db-wal",
                "meguri-media.db-shm",
                "meguri-sidecar.json",
                "meguri-derived.db",
                "meguri.lock",
            ] {
                writer::check_source_alias(&self.directory.join(name), retained)?;
            }
        }
        self.validation.protect(paths, check)
    }
    pub fn note_count(&self) -> u64 {
        self.notes
    }
    pub fn card_count(&self) -> u64 {
        self.cards
    }
    pub fn media_count(&self) -> u64 {
        self.validation.files.len() as u64
    }
    pub fn suggested_name(&self) -> String {
        Self::suggested_name_for_deck(&self.name)
    }
    /// A single filesystem component that also fits the line-based ownership
    /// marker. Reserve the suffix before truncating at a UTF-8 boundary.
    pub fn suggested_name_for_deck(native_name: &str) -> String {
        const SUFFIX: &str = ".apkg";
        let human_name = meguri_model::deck::human_name(native_name);
        let mut stem = String::new();
        for c in human_name.chars().filter(|c| !c.is_control()) {
            let c = if "\\/?<>:*|\"^".contains(c) { '_' } else { c };
            if stem.len() + c.len_utf8() > 255 - SUFFIX.len() {
                break;
            }
            stem.push(c);
        }
        let stem = stem.trim_matches(|c| c == '.' || c == ' ');
        format!("{}{SUFFIX}", if stem.is_empty() { "Meguri-Deck" } else { stem })
    }
}
fn read_error(error: DeckExportReadError) -> Error {
    match error {
        DeckExportReadError::Cancelled => Error::Cancelled,
        DeckExportReadError::DeckNotFound(id) => Error::DeckNotFound { id },
        DeckExportReadError::InvalidIds => Error::ExportInvalidIds,
        DeckExportReadError::MissingReference { .. }
        | DeckExportReadError::Storage(meguri_storage::StorageError::Row(_)) => Error::ExportNeedsCheckDatabase,
        DeckExportReadError::Storage(error) => error.into(),
    }
}
fn select(
    snapshot: &ReadSnapshot<'_>,
    deck_id: i64,
    options: DeckExportOptions,
    instant: i64,
    check: &(dyn Fn() -> bool + Sync),
) -> Result<DeckExportSelection> {
    snapshot
        .select_deck_export(
            deck_id,
            options.include_scheduling,
            options.include_presets,
            instant,
            check,
        )
        .map_err(read_error)
}
fn preflight_build(size: u64, context: &CollectionPackageWriteContext<'_>) -> Result<()> {
    let needed = size.checked_add(4 * 1024 * 1024).ok_or(Error::ExportFormatLimit)?;
    let available = context.space.available_bytes(context.collection_directory)?;
    if needed > available {
        return Err(Error::InsufficientSpace { needed, available });
    }
    Ok(())
}
pub fn preflight_deck_export_from_live(
    reader: &Reader,
    destination: &Path,
    context: &CollectionPackageWriteContext<'_>,
    deck_id: i64,
    options: DeckExportOptions,
) -> Result<DeckExportPlan> {
    use std::os::unix::fs::MetadataExt;
    cancelled(context.is_cancelled)?;
    let (source, size) = reader.live_snapshot_source()?;
    let parent = std::fs::metadata(source.parent().ok_or_else(|| crate::corrupt("source parent"))?)?;
    let directory = std::fs::metadata(context.collection_directory)?;
    if (parent.dev(), parent.ino()) != (directory.dev(), directory.ino()) {
        return Err(Error::ExportSourceInvalid {
            detail: "export directory does not belong to the admitted collection".into(),
        });
    }
    let output_parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let existing = writer::existing_directory(output_parent)?;
    let output = existing.canonicalize()?.join(
        output_parent
            .strip_prefix(&existing)
            .map_err(|_| crate::corrupt("output ancestry"))?,
    );
    let mut resolved = PathBuf::new();
    for component in output.components() {
        match component {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    let output = resolved.join(
        destination
            .file_name()
            .ok_or_else(|| crate::corrupt("output filename"))?,
    );
    let collection = context.collection_directory.canonicalize()?;
    for internal in ["collection.media", "media.staging", "media.trash"] {
        if output.starts_with(collection.join(internal)) {
            return Err(Error::ExportSourceInvalid {
                detail: "output overlaps collection media state".into(),
            });
        }
    }
    let mut protected = vec![source.clone(), destination.to_owned()];
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut path = source.as_os_str().to_owned();
        path.push(suffix);
        writer::check_source_alias(Path::new(&path), destination)?;
        protected.push(path.into());
    }
    for file in [
        "meguri-media.db",
        "meguri-media.db-wal",
        "meguri-media.db-shm",
        "meguri-sidecar.json",
        "meguri-derived.db",
        "meguri.lock",
    ] {
        let path = collection.join(file);
        writer::check_source_alias(&path, destination)?;
        protected.push(path);
    }
    let instant = context.clock.now_ms();
    let snapshot = reader.begin_snapshot()?;
    let selection = select(&snapshot, deck_id, options, instant, context.is_cancelled)?;
    preflight_build(size, context)?;
    cancelled(context.is_cancelled)?;
    let staging = staging_directory(context.collection_directory)?;
    let database = staging.path().join("snapshot.anki2");
    let (snapshot_modified, references) = build(&snapshot, &selection, &database, options, instant, context)?;
    drop(snapshot);
    let validation = ExportSourceValidation::referenced(
        context.collection_directory,
        protected,
        references,
        context.is_cancelled,
    )?;
    Ok(DeckExportPlan {
        staging,
        database,
        snapshot_modified,
        name: selection.name,
        notes: selection.notes.len() as u64,
        cards: selection.cards.len() as u64,
        reader_identity: reader.connection_identity(),
        source,
        source_identity: reader.native_identity(),
        directory: context.collection_directory.to_owned(),
        directory_identity: (directory.dev(), directory.ino()),
        destination: destination.to_owned(),
        validation,
    })
}

fn normal_default() -> ProtoBlob<DeckKindMessage> {
    let mut normal = NormalDeck::default();
    normal.config_id = 1;
    ProtoBlob::new(DeckKindMessage::new(DeckKind::Normal(normal)))
}
fn build(
    snapshot: &ReadSnapshot<'_>,
    selected: &DeckExportSelection,
    path: &Path,
    options: DeckExportOptions,
    instant: i64,
    context: &CollectionPackageWriteContext<'_>,
) -> Result<(EpochMillis, BTreeSet<String>)> {
    let mut references = BTreeSet::new();
    let frozen = FixedClock::new(instant, context.clock.utc_offset_at(instant));
    let col = Col {
        version: Col::SCHEMA_VERSION,
        created: selected.created,
        modified: EpochMillis(0),
        schema_modified: EpochMillis(instant),
        last_sync: EpochMillis(0),
        usn: Usn(0),
        dirty: 0,
        legacy: Default::default(),
    };
    let mut config = meguri_engine::bootstrap::base_config();
    if let Some(offset) = selected.creation_offset {
        config.push(ConfigEntry {
            key: "creationOffset".into(),
            value: offset.to_string().into_bytes(),
            usn: Usn(-1),
            mtime: EpochSecs(instant.div_euclid(1000)),
        });
    }
    let builder = CollectionBuilder::create(path, &col, &config, &OpenOptions::new(&frozen, context.random))?;
    let mut scope = builder.begin(StampMode::Raw, &frozen)?;
    builder.analyze_initial()?;
    if !selected.decks.contains_key(&1)
        && !selected
            .decks
            .values()
            .any(|d| meguri_text::fold::compare_folded(&d.name, "Default").is_eq())
    {
        let mut default = meguri_engine::decks::new_deck("Default".into());
        default.id = DeckId(1);
        scope.put(&default)?;
    }
    if !selected.presets.contains_key(&1) {
        scope.put(&meguri_engine::deck_config::defaults())?;
    }
    for preset in selected.presets.values() {
        cancelled(context.is_cancelled)?;
        let mut preset = preset.clone();
        if !options.include_scheduling {
            preset.config.clear_fsrs_params();
        }
        scope.put(&preset)?;
    }
    for deck in selected.decks.values() {
        cancelled(context.is_cancelled)?;
        let mut deck = deck.clone();
        if !options.include_scheduling {
            deck.common = ProtoBlob::new(DeckCommon::default());
            match &mut deck.kind.kind {
                DeckKind::Normal(normal) => {
                    normal.extend_new = 0;
                    normal.extend_review = 0;
                    normal.review_limit = None;
                    normal.new_limit = None;
                    normal.review_limit_today = None;
                    normal.new_limit_today = None;
                }
                DeckKind::Filtered(_) => deck.kind = normal_default(),
            }
        } else if selected.return_home && matches!(deck.kind.kind, DeckKind::Filtered(_)) {
            deck.kind = normal_default();
        }
        if !options.include_presets
            && let DeckKind::Normal(normal) = &mut deck.kind.kind
        {
            normal.config_id = 1;
        }
        scope.put(&deck)?;
    }
    for nt in selected.notetypes.values() {
        cancelled(context.is_cancelled)?;
        if options.include_media {
            crate::deck_export_media::notetype(&mut references, nt);
        }
        scope.put_notetype(nt)?;
    }
    for id in &selected.notes {
        cancelled(context.is_cancelled)?;
        let mut note = snapshot.note(NoteId(*id))?.expect("validated snapshot note");
        if options.include_media {
            let nt = &selected.notetypes[&note.notetype_id.0];
            for field in note.field_values() {
                cancelled(context.is_cancelled)?;
                crate::deck_export_media::fields(&mut references, field, nt.config.latex_svg);
            }
        }
        let tags: Vec<_> = meguri_engine::tags::canonical::split_text(&note.tags)
            .filter(|t| {
                options.include_scheduling || !(t.eq_ignore_ascii_case("marked") || t.eq_ignore_ascii_case("leech"))
            })
            .collect();
        note.tags = meguri_engine::tags::canonical::join_note_tags(&tags);
        note.flags = 0;
        note.data.clear();
        scope.put(&note)?;
    }
    let mut shaper = crate::apkg::CardShaper::new(
        options.include_scheduling,
        selected.return_home,
        selected.next_position,
    );
    for id in &selected.cards {
        cancelled(context.is_cancelled)?;
        let mut card = snapshot.card(CardId(*id))?.expect("validated snapshot card");
        shaper.shape(&mut card);
        scope.put(&card)?;
    }
    for id in &selected.revlog {
        cancelled(context.is_cancelled)?;
        scope.put(
            &snapshot
                .revlog_entry(RevlogId(*id))?
                .expect("validated snapshot history"),
        )?;
    }
    cancelled(context.is_cancelled)?;
    let modified = EpochMillis(context.clock.now_ms());
    scope.set_col_mod(modified)?;
    scope.commit()?;
    builder.finish()?;
    meguri_storage::collection_package::finalise_export_snapshot(path)?;
    Ok((modified, references))
}

pub fn write_deck_export_from_live(
    reader: &Reader,
    destination: &Path,
    context: &CollectionPackageWriteContext<'_>,
    mut plan: DeckExportPlan,
) -> Result<MediaCollectionExportWritten> {
    use std::os::unix::fs::MetadataExt;
    cancelled(context.is_cancelled)?;
    let source = std::fs::metadata(&plan.source)?;
    let directory = std::fs::metadata(context.collection_directory)?;
    if !std::sync::Arc::ptr_eq(&reader.connection_identity(), &plan.reader_identity)
        || reader.native_identity() != plan.source_identity
        || (source.dev(), source.ino()) != plan.source_identity
        || context.collection_directory != plan.directory.as_path()
        || (directory.dev(), directory.ino()) != plan.directory_identity
        || destination != plan.destination
    {
        return Err(Error::ExportSourceChanged);
    }
    plan.validation.validate(context.is_cancelled)?;
    writer::check_exclusive_destination(destination, context.finalisation)?;
    cancelled(context.is_cancelled)?;
    export_capture::preflight_deck(
        std::fs::metadata(&plan.database)?.len(),
        &plan.validation.files,
        destination,
        context,
    )?;
    plan.validation.capture(plan.staging.path(), context.is_cancelled)?;
    writer::publish_snapshot_with_media(
        &plan.database,
        plan.staging.path(),
        destination,
        context,
        Some(&plan.validation),
    )?;
    Ok(MediaCollectionExportWritten {
        written: LiveCollectionPackageWritten {
            snapshot_modified: plan.snapshot_modified,
        },
        deck_counts: Some(DeckExportCounts {
            notes: plan.notes,
            cards: plan.cards,
            media: plan.validation.files.len() as u64,
        }),
        source_validation: plan.validation,
    })
}
