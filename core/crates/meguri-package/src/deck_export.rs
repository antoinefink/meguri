//! Production latest-format deck export (package/apkg-export A1–A12).
//! Preflight owns no read transaction across calls. The writer repeats all
//! selection checks on one pinned snapshot before creating its fresh database.
use crate::{
    CollectionPackageWriteContext, Error, LiveCollectionPackageWritten, MediaCollectionExportWritten, Result,
    cancelled,
    export_capture::{self, ExportSourceValidation},
    writer,
};
use meguri_model::{
    card::{CardQueue, CardType},
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

#[derive(Clone, Copy)]
pub struct DeckExportOptions {
    pub include_scheduling: bool,
    pub include_presets: bool,
    pub include_media: bool,
}
pub struct DeckExportPlan {
    source: PathBuf,
    source_identity: (u64, u64),
    directory: PathBuf,
    destination: PathBuf,
    deck_id: i64,
    options: DeckExportOptions,
    instant: i64,
    references: BTreeSet<String>,
    validation: ExportSourceValidation,
    name: String,
    notes: u64,
    cards: u64,
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
        let name = meguri_model::deck::human_name(&self.name);
        format!(
            "{}.apkg",
            name.chars()
                .map(|c| if "\\/?<>:*|\"^".contains(c) { '_' } else { c })
                .collect::<String>()
        )
    }
}
fn read_error(error: DeckExportReadError) -> Error {
    match error {
        DeckExportReadError::Cancelled => Error::Cancelled,
        DeckExportReadError::DeckNotFound(id) => Error::ExportSourceInvalid {
            detail: format!("DeckNotFound: deck {id} no longer exists"),
        },
        DeckExportReadError::InvalidIds => Error::ExportSourceInvalid {
            detail: "InvalidIds: run Check Database before exporting".into(),
        },
        DeckExportReadError::MissingReference { table, id } => Error::ExportSourceInvalid {
            detail: format!("NeedsCheckDatabase: missing {table} record {id}"),
        },
        DeckExportReadError::Storage(meguri_storage::StorageError::Row(error)) => Error::ExportSourceInvalid {
            detail: format!("NeedsCheckDatabase: unreadable record: {error}"),
        },
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
fn references(
    snapshot: &ReadSnapshot<'_>,
    selection: &DeckExportSelection,
    enabled: bool,
    check: &(dyn Fn() -> bool + Sync),
) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    if enabled {
        for nt in selection.notetypes.values() {
            cancelled(check)?;
            crate::deck_export_media::notetype(&mut names, nt);
        }
        for id in &selection.notes {
            cancelled(check)?;
            let note = snapshot.note(NoteId(*id))?.expect("validated snapshot note");
            let nt = &selection.notetypes[&note.notetype_id.0];
            for field in note.field_values() {
                cancelled(check)?;
                crate::deck_export_media::fields(&mut names, field, nt.config.latex_svg);
            }
        }
    }
    Ok(names)
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
    let references = references(&snapshot, &selection, options.include_media, context.is_cancelled)?;
    drop(snapshot);
    preflight_build(size, context)?;
    let validation = ExportSourceValidation::referenced(
        context.collection_directory,
        protected,
        references.clone(),
        context.is_cancelled,
    )?;
    Ok(DeckExportPlan {
        source,
        source_identity: reader.native_identity(),
        directory: context.collection_directory.to_owned(),
        destination: destination.to_owned(),
        deck_id,
        options,
        instant,
        references,
        validation,
        name: selection.name,
        notes: selection.notes.len() as u64,
        cards: selection.cards.len() as u64,
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
) -> Result<EpochMillis> {
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
    let mut config = Vec::new();
    for (key, value) in [
        ("activeDecks", "[1]"),
        ("addToCur", "true"),
        ("collapseTime", "1200"),
        ("curDeck", "1"),
        ("dayLearnFirst", "false"),
        ("dueCounts", "true"),
        ("estTimes", "true"),
        ("newSpread", "0"),
        ("nextPos", "1"),
        ("sched2021", "true"),
        ("schedVer", "2"),
        ("sortBackwards", "false"),
        ("sortType", "\"noteFld\""),
        ("timeLim", "0"),
    ] {
        config.push(ConfigEntry {
            key: key.into(),
            value: value.as_bytes().to_vec(),
            usn: Usn(0),
            mtime: EpochSecs(0),
        });
    }
    if let Some(offset) = selected.creation_offset {
        config.push(ConfigEntry {
            key: "creationOffset".into(),
            value: offset.to_string().into_bytes(),
            usn: Usn(-1),
            mtime: EpochSecs(instant.div_euclid(1000)),
        });
    }
    let mut next = selected.next_position;
    let builder = CollectionBuilder::create(path, &col, &config, &OpenOptions::new(&frozen, context.random))?;
    let mut scope = builder.begin(StampMode::Raw, &frozen)?;
    builder.analyze_initial()?;
    // Author factory rows only when no selected row replaces them. Never copy
    // any source config/stock type, and never delete source content from a copy.
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
        scope.put_notetype(nt)?;
    }
    for id in &selected.notes {
        cancelled(context.is_cancelled)?;
        let mut note = snapshot.note(NoteId(*id))?.expect("validated snapshot note");
        let tags: Vec<_> = meguri_engine::tags::canonical::split_text(&note.tags)
            .filter(|t| {
                options.include_scheduling || !(t.eq_ignore_ascii_case("marked") || t.eq_ignore_ascii_case("leech"))
            })
            .collect();
        note.tags = if tags.is_empty() {
            String::new()
        } else {
            format!(" {} ", tags.join(" "))
        };
        note.flags = 0;
        note.data.clear();
        scope.put(&note)?;
    }
    for id in &selected.cards {
        cancelled(context.is_cancelled)?;
        let mut card = snapshot.card(CardId(*id))?.expect("validated snapshot card");
        if options.include_scheduling {
            if selected.return_home && card.original_deck_id.0 > 0 {
                card.original_deck_id = DeckId(0);
                if card.original_due != 0 {
                    card.due = card.original_due;
                }
                if card.queue.stored() >= 0 {
                    card.queue = meguri_sched::model::card_sched::queue_for_type(card.card_type, card.due);
                }
                card.original_due = 0;
            }
        } else {
            let last = if card.card_type == CardType::New {
                Some(if card.original_deck_id.0 > 0 {
                    card.original_due
                } else {
                    card.due
                } as u32)
            } else {
                card.data.new_position
            };
            card.due = last.unwrap_or_else(|| {
                let position = next;
                next = next.wrapping_add(1);
                position
            }) as i32;
            card.card_type = CardType::New;
            card.queue = CardQueue::New;
            card.interval = 0;
            card.ease_factor = 0;
            card.reps = 0;
            card.lapses = 0;
            if card.original_deck_id.0 != 0 {
                card.original_due = 0;
            }
            card.original_deck_id = DeckId(0);
            card.flags = 0;
            card.data.new_position = None;
            card.data.memory_state = None;
        }
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
    Ok(modified)
}

pub fn write_deck_export_from_live(
    reader: &Reader,
    destination: &Path,
    context: &CollectionPackageWriteContext<'_>,
    mut plan: DeckExportPlan,
) -> Result<MediaCollectionExportWritten> {
    cancelled(context.is_cancelled)?;
    let (source, size) = reader.live_snapshot_source()?;
    if source != plan.source
        || reader.native_identity() != plan.source_identity
        || context.collection_directory != plan.directory.as_path()
        || destination != plan.destination
    {
        return Err(Error::ExportSourceChanged);
    }
    plan.validation.validate(context.is_cancelled)?;
    writer::check_exclusive_destination(destination, context.finalisation)?;
    let snapshot = reader.begin_snapshot()?;
    let selection = select(
        &snapshot,
        plan.deck_id,
        plan.options,
        plan.instant,
        context.is_cancelled,
    )?;
    if references(&snapshot, &selection, plan.options.include_media, context.is_cancelled)? != plan.references {
        return Err(Error::ExportSourceChanged);
    }
    preflight_build(size, context)?;
    cancelled(context.is_cancelled)?;
    let staging = staging_directory(context.collection_directory)?;
    let path = staging.path().join("snapshot.anki2");
    let snapshot_modified = build(&snapshot, &selection, &path, plan.options, plan.instant, context)?;
    drop(snapshot);
    export_capture::preflight_deck(
        std::fs::metadata(&path)?.len(),
        &plan.validation.files,
        destination,
        context,
    )?;
    plan.validation.capture(staging.path(), context.is_cancelled)?;
    writer::publish_snapshot_with_media(&path, staging.path(), destination, context, Some(&plan.validation))?;
    Ok(MediaCollectionExportWritten {
        written: LiveCollectionPackageWritten { snapshot_modified },
        source_validation: plan.validation,
    })
}
