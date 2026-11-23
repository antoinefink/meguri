//! Internal collection-only scenario adapter (conformance/package-ops E1-E3).
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::PathBuf,
};

use meguri_engine::{
    LongOp,
    replace::{ReplaceError, ReplaceReason, ReplacementStage, StagingError},
};
use meguri_model::clock::Clock;
use meguri_package::{
    CollectionPackageInspectContext, CollectionPackageWriteContext, CollectionPackageWriteError as PackageError,
    Finalisation, inspect_collection_package, package_meta_version, write_collection_package_from_live,
};
use meguri_storage::{OpenError, ReaderRole, atomic_file::Purpose, space::StatvfsSpace};
use serde_json::{Map, Value, json};

use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{
    CollectionPackageImportArgs, ObservationArgs, ObservationRequest, PackageExportArgs, PackageListingRequest, Step,
};

pub(super) fn register(registry: &mut Registry) {
    registry.op("export", export);
    registry.op_part("export.colpkg");
    registry.op_rejecting("import", &["too_new", "corrupt", "notetype_kind_conflict"], import);
    registry.op_part("import.colpkg");
    registry.op_part("import.apkg");
    registry.observation("package_listing", listing);
    registry.op("add_backup_files", add_backup_files);
    registry.op("force_backup", force_backup);
    registry.op("backup", backup);
    registry.op("save_backup_settings", save_backup_settings);
    registry.observation("backup_files", backup_files);
    registry.observation("backup_settings", backup_settings);
}
fn error(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("package_error", error.to_string())
}
fn invalid(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("invalid_scenario", error.to_string())
}
fn source(session: &mut Session<'_>, request: &PackageListingRequest) -> Result<PathBuf, ExecError> {
    let (path, kind) = match request {
        PackageListingRequest::Name(name) => (session.storage.dir()?.join(name), "unknown_ref"),
        PackageListingRequest::Fixture(name) => (
            super::storage::fixtures_dir(session.scenario)
                .ok_or_else(|| ExecError::new("fixture_error", "no fixture directory"))?
                .join(name),
            "fixture_error",
        ),
    };
    if !path.is_file() {
        return Err(ExecError::new(kind, "package is not a regular file"));
    }
    Ok(path)
}
fn export(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Export(args) = step else {
        return Err(invalid("expected export"));
    };
    let PackageExportArgs::Collection { name, media, legacy } =
        PackageExportArgs::from_mapping(&args.0).map_err(invalid)?
    else {
        return Err(ExecError::new("unsupported_op", "deck package export is pending"));
    };
    if media == Some(true) || legacy == Some(true) {
        return Err(ExecError::new(
            "unsupported_op",
            "collection package options are pending",
        ));
    }
    let collection = session.engine_collection()?;
    let directory = session.storage.dir()?;
    let lease = collection.reader(ReaderRole::Maintenance).map_err(error)?;
    write_collection_package_from_live(
        &lease,
        &directory.join(name),
        &CollectionPackageWriteContext {
            collection_directory: &directory,
            purpose: Purpose::Export,
            finalisation: Finalisation::Replace,
            clock: session.clock.as_ref(),
            random: session.random.as_ref(),
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .map_err(error)?;
    Ok(OpResult::Done)
}
fn import(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Import(args) = step else {
        return Err(invalid("expected import"));
    };
    if args.0.get("format").and_then(Value::as_str) == Some("apkg") {
        return import_apkg(session, &args.0);
    }
    let args = CollectionPackageImportArgs::from_mapping(&args.0).map_err(invalid)?;
    let source = source(session, &args.source)?;
    let live = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection open"))?
        .path()
        .to_owned();
    let directory = session.storage.dir()?;
    // Promote before creating staging: initial hosted open sweeps reserved
    // leftovers. Subsequent refusals keep this writer and its schema baseline.
    session.engine_collection()?;
    let inspected = match inspect_collection_package(
        &source,
        &CollectionPackageInspectContext {
            collection_directory: &directory,
            live_collection_size: fs::metadata(&live).map_err(error)?.len(),
            reserved_space_bytes: 0,
            clock: session.clock.as_ref(),
            random: session.random.as_ref(),
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    ) {
        Ok(inspected) => inspected,
        Err(PackageError::TooNew) => return Ok(OpResult::Rejected("too_new")),
        Err(PackageError::Corrupt { .. } | PackageError::SchemaUnsupported { .. }) => {
            return Ok(OpResult::Rejected("corrupt"));
        }
        Err(other) => return Err(error(other)),
    };
    let stage = ReplacementStage::from_staging(inspected.into_staged().into_directory()).map_err(error)?;
    let slot = session
        .storage
        .host()?
        .engine()
        .acquire_slot(LongOp::Restore)
        .map_err(error)?;
    // The production preparation owns R7 stamps. Never stamp in this closure.
    let collection = match slot.replace_verified(stage, ReplaceReason::Restore, |_| Ok(())) {
        Ok(collection) => collection,
        Err(ReplaceError::Staging(StagingError::StagingCorrupt)) => return Ok(OpResult::Rejected("corrupt")),
        Err(ReplaceError::Staging(StagingError::StagingRefused(
            OpenError::TooOld { .. }
            | OpenError::TooNew { .. }
            | OpenError::V1Scheduler
            | OpenError::NotACollection { .. }
            | OpenError::Corrupt { .. }
            | OpenError::UpgradeFailed { .. },
        ))) => return Ok(OpResult::Rejected("corrupt")),
        // Includes I/O and every post-activation error: none implies rollback.
        Err(other) => return Err(error(other)),
    };
    session.storage.adopt_replacement(collection)?;
    session.day.clear();
    session.review_session = None;
    session.held_queue = None;
    Ok(OpResult::Done)
}

/// E6: inspect, apply with the five options (accepting a merge preview at
/// once), close. The log names target notes by their logical ids.
fn import_apkg(session: &mut Session<'_>, args: &Map<String, Value>) -> Result<OpResult, ExecError> {
    use meguri_package::apkg::{self, ApkgContext, ApkgImportError, ImportOptions, UpdateCondition};
    let mut named = Map::new();
    for key in ["name", "fixture"] {
        if let Some(value) = args.get(key) {
            named.insert(key.to_owned(), value.clone());
        }
    }
    let request = PackageListingRequest::from_mapping(&named).map_err(invalid)?;
    let path = source(session, &request)?;
    let flag = |key: &str| args.get(key).and_then(Value::as_bool).ok_or_else(|| invalid(key));
    let rule = |key: &str| match args.get(key).and_then(Value::as_str) {
        Some("if_newer") => Ok(UpdateCondition::IfNewer),
        Some("always") => Ok(UpdateCondition::Always),
        Some("never") => Ok(UpdateCondition::Never),
        _ => Err(invalid(key)),
    };
    let options = ImportOptions {
        merge_notetypes: flag("merge_notetypes")?,
        update_notes: rule("update_notes")?,
        update_notetypes: rule("update_notetypes")?,
        with_scheduling: flag("with_scheduling")?,
        with_deck_configs: flag("with_deck_configs")?,
    };
    // Promote before creating staging, as the collection form does.
    session.engine_collection()?;
    let context = ApkgContext {
        collection_directory: session.storage.dir()?,
        clock: session.clock.clone(),
        random: session.random.clone(),
        space: std::sync::Arc::new(StatvfsSpace),
    };
    let rejected = |refusal: &ApkgImportError| match refusal {
        ApkgImportError::TooNew => Some("too_new"),
        ApkgImportError::Corrupt { .. }
        | ApkgImportError::SchemaUnsupported { .. }
        | ApkgImportError::UnsupportedFile => Some("corrupt"),
        ApkgImportError::NotetypeKindConflict { .. } => Some("notetype_kind_conflict"),
        _ => None,
    };
    let cancel = meguri_engine::CancelToken::new();
    let progress = meguri_engine::ProgressSink::new();
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("package.apkg").to_owned();
    let imported = match apkg::inspect(&path, &name, &context, &cancel, &progress) {
        Ok(imported) => imported,
        Err(refusal) => {
            return match rejected(&refusal) {
                Some(kind) => Ok(OpResult::Rejected(kind)),
                None => Err(error(refusal)),
            };
        }
    };
    let engine = session.storage.host()?.engine().clone();
    let mut token = None;
    let result = loop {
        match apkg::apply(&engine, &imported, options, token.as_ref(), &cancel, &progress) {
            Ok(result) => break result,
            Err(ApkgImportError::ConfirmationRequired { token: fresh, .. }) if token.as_ref() != Some(&fresh) => {
                token = Some(fresh);
            }
            Err(refusal) => {
                return match rejected(&refusal) {
                    Some(kind) => Ok(OpResult::Rejected(kind)),
                    None => Err(error(refusal)),
                };
            }
        }
    };
    imported.close();
    let logical = |id: i64| {
        session
            .notes
            .iter()
            .position(|known| *known == id)
            .map_or_else(|| json!(id), |at| json!(format!("#n{}", at + 1)))
    };
    let list = |entries: &[apkg::LogEntry], named: bool| -> Value {
        entries
            .iter()
            .map(|entry| {
                json!({
                    "note": if named { logical(entry.note) } else { json!(entry.note) },
                    "fields": entry.fields,
                })
            })
            .collect()
    };
    let log = &result.log;
    Ok(OpResult::Output(json!({
        "found_notes": log.found_notes,
        "new": list(&log.new, true),
        "updated": list(&log.updated, true),
        "duplicate": list(&log.duplicate, true),
        "conflicting": list(&log.conflicting, false),
        "missing_notetype": list(&log.missing_notetype, false),
        "missing_deck": list(&log.missing_deck, true),
        "first_field_match": [],
        "empty_first_field": [],
    })))
}

fn listing(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(args) = &request.args else {
        return Err(invalid("listing expects a mapping"));
    };
    let source = source(session, &PackageListingRequest::from_mapping(args).map_err(invalid)?)?;
    // General listing is deliberately separate from production's needed-entry
    // ZIP index. Entry names are keys only, never scratch paths.
    let scratch = tempfile::tempdir().map_err(error)?;
    let mut archive = zip::ZipArchive::new(File::open(source).map_err(error)?).map_err(error)?;
    let mut entries = Vec::new();
    let mut meta = Value::Null;
    let mut sqlite = Map::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(error)?;
        let name = entry.name().map_err(error)?.into_owned();
        let method = match entry.compression() {
            zip::CompressionMethod::Stored => 0,
            zip::CompressionMethod::Deflated => 8,
            _ => return Err(ExecError::new("package_error", "unsupported ZIP compression method")),
        };
        entries.push(json!({"name": name, "method": method}));
        if name == "meta" {
            let mut bytes = Vec::new();
            meta = match entry.by_ref().take(64 * 1024 + 1).read_to_end(&mut bytes) {
                Ok(_) if bytes.len() <= 64 * 1024 => package_meta_version(&bytes)
                    .map(Value::from)
                    .unwrap_or_else(|_| json!("invalid")),
                _ => json!("invalid"),
            };
            continue;
        }
        let mut header = [0; 100];
        let mut used = 0;
        while used < header.len() {
            let n = entry.read(&mut header[used..]).map_err(error)?;
            if n == 0 {
                break;
            }
            used += n;
        }
        if used < 16 || &header[..16] != b"SQLite format 3\0" {
            continue;
        }
        let entry_scratch = tempfile::Builder::new()
            .prefix("sqlite-")
            .tempdir_in(scratch.path())
            .map_err(error)?;
        let raw = entry_scratch.path().join("raw.anki2");
        let mut file = File::create(&raw).map_err(error)?;
        file.write_all(&header[..used]).map_err(error)?;
        // Stream only raw SQLite; zstd bodies are never decoded by E3.
        let cap = 8 * 1024 * 1024 * 1024u64;
        let copied = std::io::copy(&mut entry.take(cap + 1), &mut file).map_err(error)?;
        if copied > cap {
            return Err(error("raw SQLite entry exceeds limit"));
        }
        drop(file);
        sqlite.insert(
            name,
            super::storage::package_sqlite_listing(
                &raw,
                entry_scratch.path(),
                &header,
                session.clock.as_ref(),
                session.random.as_ref(),
            )?,
        );
        fs::remove_file(raw).map_err(error)?;
    }
    Ok(json!({"entries": entries, "meta_version": meta, "sqlite": sqlite}))
}

// F1: seeds, the real host provider and observations share disjoint storage.
fn backups(session: &mut Session<'_>) -> Result<PathBuf, ExecError> {
    session.storage.backup_directory()
}

fn add_backup_files(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    use crate::scenario::AddBackupFilesArgs;
    use meguri_package::backup::naming::name_from_wall_seconds;
    let Step::AddBackupFiles(args) = step else {
        return Err(invalid("expected add_backup_files"));
    };
    // The scenario model validates components, bounds, duration and duplicates.
    let names = match args {
        AddBackupFilesArgs::Names(names) => names.clone(),
        AddBackupFilesArgs::Series(series) => {
            let wall = |text: &str| {
                format!("{text}Z")
                    .parse::<jiff::Timestamp>()
                    .map(|t| t.as_second())
                    .map_err(invalid)
            };
            let mut second = wall(&series.from)?;
            let until = wall(&series.until)?;
            let every = series.every.seconds();
            if every < 1 || until < second || (until - second) / every >= 20_000 {
                return Err(invalid("invalid backup series bounds"));
            }
            let mut names = Vec::new();
            loop {
                names.push(name_from_wall_seconds(second).map_err(invalid)?);
                let Some(next) = second.checked_add(every).filter(|n| *n <= until) else {
                    break;
                };
                second = next;
            }
            names
        }
    };
    let directory = backups(session)?;
    let mut seen = std::collections::BTreeSet::new();
    for name in &names {
        if !seen.insert(name) {
            return Err(invalid("duplicate backup seed"));
        }
        match directory.join(name).symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => return Err(invalid("backup seed already exists")),
            Err(e) => return Err(error(e)),
        }
    }
    for name in names {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(name))
            .map_err(error)?;
    }
    Ok(OpResult::Done)
}

fn force_backup(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    use meguri_model::time::EpochMillis;
    let Step::ForceBackup(args) = step else {
        return Err(invalid("expected force_backup"));
    };
    let directory = backups(session)?;
    let expected = meguri_package::backup::naming::name_at(EpochMillis(session.clock.now_ms()), session.clock.as_ref())
        .map_err(error)?;
    match directory.join(expected).symlink_metadata() {
        Ok(_) => return Err(invalid("seed collides with force_backup (F7)")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(error(e)),
    }
    let collection = session.engine_collection()?;
    let receipt = session
        .storage
        .host()?
        .backup()
        .before_colpkg_import(&collection, &meguri_engine::CancelToken::default())
        .map_err(error)?;
    if let Some(name) = &args.copy_as {
        // Copy the produced bytes, never synthesize an equivalent export.
        fs::copy(directory.join(&receipt.name), session.storage.dir()?.join(name)).map_err(error)?;
    }
    Ok(OpResult::Output(json!({"name": receipt.name})))
}

// H2: each entry appears newly created under the oracle's frozen clock.
pub(super) struct ScenarioBackupEntryTimes(pub std::sync::Arc<dyn Clock>);
impl meguri_engine::maintenance::BackupEntryTimes for ScenarioBackupEntryTimes {
    fn read(&self, directory: &std::path::Path) -> Result<Vec<i128>, meguri_engine::EngineError> {
        match fs::read_dir(directory) {
            Ok(entries) => entries
                .map(|entry| {
                    entry
                        .map(|_| i128::from(self.0.now_ms()) * 1_000_000)
                        .map_err(|e| meguri_storage::StorageError::from(e).into())
                })
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(meguri_storage::StorageError::from(e).into()),
        }
    }
}
fn backup(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Backup(args) = step else {
        return Err(invalid("expected backup"));
    };
    session.engine_collection()?;
    let host = session.storage.host()?;
    let cancel = meguri_engine::CancelToken::default();
    let written = if args.force {
        Some(host.engine().back_up_now(&cancel).map_err(error)?)
    } else {
        host.engine().backup_if_changed(&cancel).map_err(error)?
    };
    Ok(OpResult::Output(json!({"name": written.map(|w| w.receipt.name)})))
}

fn save_backup_settings(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::SaveBackupSettings(args) = step else {
        return Err(invalid("expected save_backup_settings"));
    };
    let collection = session.engine_collection()?;
    meguri_engine::backup_limits::save(
        &collection,
        meguri_engine::backup_limits::BackupLimits {
            daily: args.daily,
            weekly: args.weekly,
            monthly: args.monthly,
            minimum_interval_mins: args.minimum_interval_mins,
        },
    )
    .map_err(error)?;
    Ok(OpResult::Done)
}

fn backup_files(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(backups(session)?).map_err(error)? {
        let entry = entry.map_err(error)?;
        match fs::symlink_metadata(entry.path()) {
            Ok(metadata) if metadata.file_type().is_file() => {
                names.push(
                    entry
                        .file_name()
                        .into_string()
                        .map_err(|_| error("non-UTF8 session name"))?,
                );
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(error(e)),
        }
    }
    names.sort_unstable();
    Ok(json!(names))
}

fn backup_settings(session: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection open"))?;
    let reader = collection.reader()?;
    let limits = meguri_engine::backup_limits::read(&*reader).map_err(error)?;
    Ok(
        json!({"daily": limits.daily, "weekly": limits.weekly, "monthly": limits.monthly,
        "minimum_interval_mins": limits.minimum_interval_mins}),
    )
}
