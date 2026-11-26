//! Host inputs for the media-inclusive user export worker. This never gates on study admission.
use crate::{CoreHost, HostConfig};
use meguri_engine::{CancelToken, Collection, EngineError};
use meguri_package::{CollectionPackageWriteContext, Finalisation, LiveCollectionPackageWritten};
use meguri_storage::{ReadRows, ReaderRole, atomic_file::Purpose};
use std::{
    fs,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
};

pub(crate) struct ExportInputs {
    collection_directory: PathBuf,
    backups_directory: PathBuf,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    random: std::sync::Arc<dyn meguri_model::rng::RandomSource>,
    disk_space_provider: std::sync::Arc<dyn meguri_storage::space::SpaceProvider>,
}
impl ExportInputs {
    pub(crate) fn new(config: &HostConfig) -> Self {
        Self {
            collection_directory: config.collection_directory.clone(),
            backups_directory: config.backups_directory.clone(),
            clock: config.clock.clone(),
            random: config.random.clone(),
            disk_space_provider: config.disk_space_provider.clone(),
        }
    }
}

pub struct WrittenCollectionExport {
    pub directory: PathBuf,
    pub path: PathBuf,
    pub protected_directories: Vec<PathBuf>,
    pub space: std::sync::Arc<dyn meguri_storage::space::SpaceProvider>,
    pub written: LiveCollectionPackageWritten,
    pub suggested_name: String,
    pub deck_counts: Option<meguri_package::DeckExportCounts>,
    source_validation: meguri_package::ExportSourceValidation,
}

impl WrittenCollectionExport {
    /// Delivery boundary: run after the completed package's final hash, before Ready.
    pub fn validate_sources(&self, cancel: &CancelToken) -> Result<(), EngineError> {
        self.source_validation
            .validate(&|| cancel.is_cancelled())
            .map_err(package_error)
    }
}

impl CoreHost {
    /// Runs only on the bounded export worker. The fixed owned slot also fences
    /// ambiguous output left by a previous process; it is never swept on open.
    pub fn write_collection_export(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
        progress: &meguri_engine::ProgressSink,
    ) -> Result<WrittenCollectionExport, EngineError> {
        self.write_package_export(collection, cancel, progress, None)
    }

    /// Selected-deck latest-format export, with the same owned output lifecycle.
    pub fn write_deck_export(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
        progress: &meguri_engine::ProgressSink,
        deck_id: i64,
        options: meguri_package::DeckExportOptions,
    ) -> Result<WrittenCollectionExport, EngineError> {
        self.write_package_export(collection, cancel, progress, Some((deck_id, options)))
    }

    fn write_package_export(
        &self,
        collection: &Collection,
        cancel: &CancelToken,
        progress: &meguri_engine::ProgressSink,
        deck: Option<(i64, meguri_package::DeckExportOptions)>,
    ) -> Result<WrittenCollectionExport, EngineError> {
        collection.open_evidence()?;
        cancel.check()?;
        let config = &self.export;
        let parent = config
            .collection_directory
            .parent()
            .ok_or_else(|| EngineError::InvalidArgument {
                message: "collection has no private parent".into(),
            })?;
        let root = normalized(&parent.join("CollectionExports"))?;
        let protected = vec![config.collection_directory.clone(), config.backups_directory.clone()];
        check_layout(&root, &protected)?;
        let directory = root.join("owned");
        // Read-only refusal, not a reservation: recheck ownership after preflight.
        if directory.symlink_metadata().is_ok() && !ownership(&directory).is_ok_and(|(_, state, _)| state == "safe") {
            return Err(EngineError::Busy { running_op: "a retained Files export; finish its provider operation before retrying (ownership cannot be proven after restart)".into() });
        }
        self.review().flush_for_background()?;
        progress.set_phase(meguri_engine::ProgressPhase::Named(match deck {
            Some((_, options)) if options.include_media => "Capturing and writing deck and media",
            Some(_) => "Capturing and writing deck",
            None => "Capturing and writing collection and media",
        }));
        let operation = directory.join(format!("operation-{:016x}", config.random.next_u64()));
        let reader = collection.reader(ReaderRole::Maintenance)?;
        let suggested_name = if let Some((id, _)) = deck {
            let snapshot = reader.snapshot()?;
            let name = snapshot.deck_name(meguri_model::ids::DeckId(id))?.ok_or_else(|| EngineError::NotFound {
                table: meguri_storage::Table::Decks, key: id.to_string(),
            })?;
            meguri_package::DeckExportPlan::suggested_name_for_deck(&name)
        } else {
            "Meguri-Collection.colpkg".into()
        };
        let path = operation.join(&suggested_name);
        let is_cancelled = || cancel.is_cancelled();
        let context = CollectionPackageWriteContext {
            collection_directory: &config.collection_directory,
            purpose: Purpose::Export,
            finalisation: Finalisation::ExclusiveCreate,
            clock: config.clock.as_ref(),
            random: config.random.as_ref(),
            space: config.disk_space_provider.as_ref(),
            is_cancelled: &is_cancelled,
        };
        struct Unfinished<'a>(&'a Path, &'a [PathBuf], bool);
        impl Drop for Unfinished<'_> {
            fn drop(&mut self) {
                if !self.2 {
                    let _ = remove_collection_export(self.0, self.1);
                    let _ = fs::remove_dir(self.0); // empty, unmarked setup failure only
                }
            }
        }
        // The actual DB+WAL P10 budget precedes every output mutation, including recovery.
        enum Plan {
            Collection(meguri_package::MediaCollectionExportPlan),
            Deck(meguri_package::DeckExportPlan),
        }
        let mut plan = match deck {
            Some((id, options)) => Plan::Deck(meguri_package::preflight_deck_export_from_live(&reader, &path, &context, id, options).map_err(package_error)?),
            None => Plan::Collection(meguri_package::preflight_media_collection_export_from_live(&reader, &path, &context).map_err(package_error)?),
        };
        if let Ok((retained, _, filename)) = ownership(&directory) {
            let paths = [
                directory.join(MARKER),
                directory.join(&retained).join(filename),
            ];
            match &mut plan {
                Plan::Collection(plan) => plan.protect_retained_outputs(&paths, &is_cancelled),
                Plan::Deck(plan) => plan.protect_retained_outputs(&paths, &is_cancelled),
            }.map_err(package_error)?;
        }
        cancel.check()?;
        if directory.symlink_metadata().is_ok() {
            // Only a durable never-handed-off/completed marker permits recovery.
            // Unknown/old/provider-owned slots remain fenced, with a truthful refusal.
            if ownership(&directory).is_ok_and(|(_, state, _)| state == "safe") {
                remove_collection_export(&directory, &protected)?;
            } else {
                return Err(EngineError::Busy { running_op: "a retained Files export; finish its provider operation before retrying (ownership cannot be proven after restart)".into() });
            }
        }
        fs::create_dir_all(&root).map_err(io)?;
        fs::create_dir(&directory).map_err(io)?;
        let mut unfinished = Unfinished(&directory, &protected, false);
        write_ownership(&directory, operation.file_name().unwrap().to_str().unwrap(), "safe", &suggested_name)?;
        fs::create_dir(&operation).map_err(io)?;
        meguri_storage::atomic_file::sync_directory(&directory)?;
        meguri_storage::atomic_file::sync_directory(&root)?;
        meguri_storage::atomic_file::sync_directory(parent)?;
        let completed = match plan {
            Plan::Collection(plan) => meguri_package::write_media_collection_export_from_live(&reader, &path, &context, plan),
            Plan::Deck(plan) => meguri_package::write_deck_export_from_live(&reader, &path, &context, plan),
        }.map_err(package_error)?;
        drop(reader);
        cancel.check()?;
        if !collection.is_current() {
            return Err(EngineError::StaleGeneration {
                planned: collection.generation(),
                current: collection.current_generation(),
            });
        }
        unfinished.2 = true;
        drop(unfinished);
        Ok(WrittenCollectionExport {
            directory,
            path,
            written: completed.written,
            deck_counts: completed.deck_counts,
            suggested_name,
            source_validation: completed.source_validation,
            protected_directories: protected,
            space: config.disk_space_provider.clone(),
        })
    }
}
fn io(error: std::io::Error) -> EngineError {
    meguri_storage::StorageError::from(error).into()
}

fn package_error(error: meguri_package::CollectionPackageWriteError) -> EngineError {
    match error {
        meguri_package::CollectionPackageWriteError::DeckNotFound { id } => EngineError::NotFound {
            table: meguri_storage::Table::Decks, key: id.to_string(),
        },
        meguri_package::CollectionPackageWriteError::ExportInvalidIds => EngineError::ExportInvalidIds,
        meguri_package::CollectionPackageWriteError::ExportNeedsCheckDatabase => EngineError::ExportNeedsCheckDatabase,
        meguri_package::CollectionPackageWriteError::Cancelled => EngineError::Cancelled,
        meguri_package::CollectionPackageWriteError::ExportSourceChanged => EngineError::ExportSourceChanged,
        meguri_package::CollectionPackageWriteError::ExportSourceInvalid { .. } => EngineError::ExportSourceInvalid,
        meguri_package::CollectionPackageWriteError::ExportFormatLimit => EngineError::ExportFormatLimit,
        other => EngineError::Internal {
            message: format!("collection export: {other}"),
        },
    }
}
fn invalid() -> EngineError {
    EngineError::InvalidArgument {
        message: "export output overlaps protected paths or has unrecognized ownership".into(),
    }
}
/// Resolve existing symlinks component by component, including before `..`;
/// normalize absent suffixes too. Failure is a refusal, never lexical fallback.
fn normalized(path: &Path) -> Result<PathBuf, EngineError> {
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_err(io)?.join(path)
    };
    let mut result = PathBuf::new();
    for part in absolute.components() {
        match part {
            Component::RootDir => result.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(name) => {
                result.push(name);
                match result.symlink_metadata() {
                    Ok(_) => {
                        result = result.canonicalize().map_err(io)?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(io(e)),
                }
            }
            Component::Prefix(_) => return Err(invalid()),
        }
    }
    Ok(result)
}
fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
fn check_layout(root: &Path, protected: &[PathBuf]) -> Result<(), EngineError> {
    let root = normalized(root)?;
    let paths = protected.iter().map(|p| normalized(p)).collect::<Result<Vec<_>, _>>()?;
    for (i, a) in paths.iter().enumerate() {
        if overlaps(&root, a) || paths[..i].iter().any(|b| overlaps(a, b)) {
            return Err(invalid());
        }
    }
    Ok(())
}
const MARKER: &str = "ownership";
fn ownership(directory: &Path) -> Result<(String, String, String), EngineError> {
    // Reject redirected slots and markers; never recurse through aliases.
    if !directory.symlink_metadata().map_err(io)?.file_type().is_dir() {
        return Err(invalid());
    }
    let marker = directory.join(MARKER);
    let meta = marker.symlink_metadata().map_err(io)?;
    if !meta.file_type().is_file() || meta.len() > 4096 {
        return Err(invalid());
    }
    let content = fs::read_to_string(marker).map_err(io)?;
    let lines: Vec<_> = content.lines().collect();
    if !matches!(lines.first(), Some(&"meguri-export-v1") | Some(&"meguri-export-v2"))
        || (lines[0] == "meguri-export-v1" && lines.len() != 3)
        || (lines[0] == "meguri-export-v2" && lines.len() != 4)
        || !["safe", "provider"].contains(&lines[1])
        || lines[2].len() != 26
        || !lines[2].starts_with("operation-")
        || !lines[2][10..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid());
    }
    let filename = if lines[0] == "meguri-export-v1" { "Meguri-Collection.colpkg" } else { lines[3] };
    if filename.is_empty()
        || filename.len() > 255
        || Path::new(filename).file_name().and_then(|name| name.to_str()) != Some(filename)
        || (filename != "Meguri-Collection.colpkg" && !filename.ends_with(".apkg"))
        || filename.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok((lines[2].into(), lines[1].into(), filename.into()))
}
fn write_ownership(directory: &Path, operation: &str, state: &str, filename: &str) -> Result<(), EngineError> {
    let mut file = meguri_storage::atomic_file::AtomicFile::new(&directory.join(MARKER), Purpose::Export)?;
    let marker = if filename == "Meguri-Collection.colpkg" {
        format!("meguri-export-v1\n{state}\n{operation}\n")
    } else {
        format!("meguri-export-v2\n{state}\n{operation}\n{filename}\n")
    };
    file.file_mut()
        .write_all(marker.as_bytes())
        .map_err(io)?;
    file.publish(|_| Ok(())).map_err(Into::into)
}
/// Worker-only: durably fence recovery BEFORE exposing any native source URL.
pub fn collection_export_provider_state(directory: &Path, active: bool) -> Result<(), EngineError> {
    let (operation, _, filename) = ownership(directory)?;
    write_ownership(directory, &operation, if active { "provider" } else { "safe" }, &filename)
}
/// Worker-only cleanup with an in-memory proof retained across removal failures.
/// A fresh remover must first recognize and validate the complete owned tree.
#[derive(Default)]
pub struct CollectionExportRemoval {
    proof: Option<RemovalProof>,
}
struct RemovalProof {
    directory: PathBuf,
    root: PathBuf,
    directory_identity: (u64, u64),
    root_identity: (u64, u64),
    // Keep the proven inodes allocated until this remover is released.
    _directory_handle: fs::File,
    _root_handle: fs::File,
    operation: String,
    filename: String,
    phase: RemovalPhase,
}
#[derive(PartialEq, Eq)]
enum RemovalPhase {
    Marker,
    Directory,
    RootSync,
    Done,
}
fn atomic_export_temporary(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(".meguri-tmp-export-"))
        .is_some_and(|suffix| suffix.len() == 6 && suffix.bytes().all(|b| b.is_ascii_alphanumeric()))
}
fn directory_identity(path: &Path) -> Result<(u64, u64), EngineError> {
    let meta = path.symlink_metadata().map_err(io)?;
    if !meta.file_type().is_dir() {
        return Err(invalid());
    }
    Ok((meta.dev(), meta.ino()))
}
impl CollectionExportRemoval {
    pub fn remove(&mut self, directory: &Path, protected: &[PathBuf]) -> Result<(), EngineError> {
        let root = directory.parent().ok_or_else(invalid)?;
        check_layout(root, protected)?;
        if self.proof.is_none() {
            let (operation, state, filename) = ownership(directory)?;
            let directory_id = directory_identity(directory)?;
            let root_id = directory_identity(root)?;
            let directory_handle = fs::File::open(directory).map_err(io)?;
            let root_handle = fs::File::open(root).map_err(io)?;
            let opened_directory = directory_handle.metadata().map_err(io)?;
            let opened_root = root_handle.metadata().map_err(io)?;
            if (opened_directory.dev(), opened_directory.ino()) != directory_id
                || (opened_root.dev(), opened_root.ino()) != root_id
            {
                return Err(invalid());
            }
            for entry in fs::read_dir(directory).map_err(io)? {
                let entry = entry.map_err(io)?;
                if entry.file_name() != MARKER && entry.file_name() != operation.as_str() {
                    return Err(invalid());
                }
            }
            let output = directory.join(&operation);
            match output.symlink_metadata() {
                Ok(meta) => {
                    if !meta.file_type().is_dir() {
                        return Err(invalid());
                    }
                    // Validate the entire tree before unlinking any entry. A
                    // process killed during compression leaves only AtomicFile's
                    // reserved regular file, and only safe ownership permits it.
                    let mut files = Vec::new();
                    for entry in fs::read_dir(&output).map_err(io)? {
                        let entry = entry.map_err(io)?;
                        let name = entry.file_name();
                        if !entry.file_type().map_err(io)?.is_file()
                            || (name != filename.as_str()
                                && !(state == "safe" && atomic_export_temporary(&name)))
                        {
                            return Err(invalid());
                        }
                        files.push(entry.path());
                    }
                    for file in files {
                        fs::remove_file(file).map_err(io)?;
                    }
                    fs::remove_dir(output).map_err(io)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(e)),
            }
            self.proof = Some(RemovalProof {
                directory: directory.to_owned(),
                root: root.to_owned(),
                directory_identity: directory_id,
                root_identity: root_id,
                _directory_handle: directory_handle,
                _root_handle: root_handle,
                operation,
                filename,
                phase: RemovalPhase::Marker,
            });
        }
        let proof = self.proof.as_mut().unwrap();
        if proof.directory != directory || proof.root != root || directory_identity(root)? != proof.root_identity {
            return Err(invalid());
        }
        if proof.phase == RemovalPhase::Done {
            return Ok(());
        }
        if matches!(proof.phase, RemovalPhase::Marker | RemovalPhase::Directory) {
            if directory_identity(directory)? != proof.directory_identity {
                return Err(invalid());
            }
            for entry in fs::read_dir(directory).map_err(io)? {
                let entry = entry.map_err(io)?;
                if proof.phase != RemovalPhase::Marker || entry.file_name() != MARKER {
                    return Err(invalid());
                }
            }
        }
        if proof.phase == RemovalPhase::Marker {
            let marker = directory.join(MARKER);
            match marker.symlink_metadata() {
                Ok(_) => {
                    let (operation, _, filename) = ownership(directory)?;
                    if operation != proof.operation || filename != proof.filename {
                        return Err(invalid());
                    }
                    fs::remove_file(marker).map_err(io)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(e)),
            }
            proof.phase = RemovalPhase::Directory;
            meguri_model::faultpoint!("host.collection_export.after_marker_unlink").map_err(io)?;
        }
        if proof.phase == RemovalPhase::Directory {
            fs::remove_dir(directory).map_err(io)?;
            proof.phase = RemovalPhase::RootSync;
        }
        // Absence is accepted only by this remover's earlier ownership proof.
        match directory.symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(invalid()),
        }
        meguri_model::faultpoint!("host.collection_export.before_removal_root_sync").map_err(io)?;
        meguri_storage::atomic_file::sync_directory(root)?;
        proof.phase = RemovalPhase::Done;
        Ok(())
    }
}
/// One-shot cleanup for paths whose ownership marker is still present.
pub fn remove_collection_export(directory: &Path, protected: &[PathBuf]) -> Result<(), EngineError> {
    CollectionExportRemoval::default().remove(directory, protected)
}

#[cfg(test)]
mod export_refusal_tests {
    use super::*;
    use meguri_package::CollectionPackageWriteError as PackageError;
    #[test]
    fn deck_names_and_owned_markers_remain_narrow_and_collection_compatible() {
        let suggested = meguri_package::DeckExportPlan::suggested_name_for_deck;
        assert_eq!(suggested("Alpha::Beta"), "Alpha__Beta.apkg");
        assert_eq!(suggested("Alpha\u{1f}Beta/?"), "Alpha__Beta__.apkg");
        assert_eq!(suggested("Alpha\0\r\n\tBeta"), "AlphaBeta.apkg");
        assert_eq!(suggested(" . /\\:?<>*|\"^\n . "), "__________.apkg");
        assert_eq!(suggested(" . \0\r\n\t . "), "Meguri-Deck.apkg");
        assert_eq!(suggested(&"a".repeat(300)), format!("{}.apkg", "a".repeat(250)));
        assert_eq!(
            suggested(&format!("{}é", "a".repeat(249))),
            format!("{}.apkg", "a".repeat(249))
        );
        assert_eq!(suggested(&"🎴".repeat(100)), format!("{}.apkg", "🎴".repeat(62)));
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("owned");
        fs::create_dir(&directory).unwrap();
        let operation = "operation-0000000000000001";
        let filenames = [
            "Meguri-Collection.colpkg".into(),
            "Alpha__Beta.apkg".into(),
            suggested(&"é".repeat(200)),
            suggested(&"🎴".repeat(100)),
            suggested("Hello\nworld\0"),
        ];
        for filename in &filenames {
            write_ownership(&directory, operation, "safe", filename).unwrap();
            assert_eq!(
                ownership(&directory).unwrap(),
                (operation.into(), "safe".into(), filename.clone())
            );
            let marker = fs::read_to_string(directory.join(MARKER)).unwrap();
            assert!(marker.starts_with(if filename == "Meguri-Collection.colpkg" {
                "meguri-export-v1\n"
            } else {
                "meguri-export-v2\n"
            }));
            let output = directory.join(operation);
            fs::create_dir(&output).unwrap();
            fs::write(output.join(filename), b"synthetic package filename witness").unwrap();
            collection_export_provider_state(&directory, true).unwrap();
            assert_eq!(ownership(&directory).unwrap().1, "provider");
            collection_export_provider_state(&directory, false).unwrap();
            assert_eq!(&ownership(&directory).unwrap().2, filename);
            remove_collection_export(&directory, &[]).unwrap();
            assert!(!directory.exists());
            fs::create_dir(&directory).unwrap();
        }
        for filename in [
            "../keep.apkg".into(),
            "/keep.apkg".into(),
            "keep.colpkg".into(),
            "child/keep.apkg".into(),
            "keep\nname.apkg".into(),
            "keep\0name.apkg".into(),
            format!("{}.apkg", "a".repeat(251)),
        ] {
            fs::write(directory.join(MARKER), format!("meguri-export-v2\nsafe\n{operation}\n{filename}\n")).unwrap();
            assert!(ownership(&directory).is_err());
        }
    }
    #[test]
    fn package_refusals_preserve_only_known_source_reasons() {
        assert_eq!(
            package_error(PackageError::ExportSourceChanged),
            EngineError::ExportSourceChanged
        );
        assert_eq!(
            package_error(PackageError::ExportSourceInvalid {
                detail: "never displayed".into()
            }),
            EngineError::ExportSourceInvalid
        );
        assert_eq!(
            package_error(PackageError::ExportFormatLimit),
            EngineError::ExportFormatLimit
        );
        assert_eq!(package_error(PackageError::Cancelled), EngineError::Cancelled);
        assert_eq!(package_error(PackageError::DeckNotFound { id: 42 }), EngineError::NotFound {
            table: meguri_storage::Table::Decks, key: "42".into(),
        });
        assert_eq!(package_error(PackageError::ExportInvalidIds), EngineError::ExportInvalidIds);
        assert_eq!(package_error(PackageError::ExportNeedsCheckDatabase), EngineError::ExportNeedsCheckDatabase);
        for error in [
            PackageError::DiskFull,
            PackageError::InsufficientSpace {
                needed: 10,
                available: 1,
            },
            PackageError::DestinationExists,
            PackageError::Corrupt {
                detail: "generation failure".into(),
            },
            PackageError::Io {
                detail: "unknown I/O".into(),
            },
        ] {
            assert!(matches!(package_error(error), EngineError::Internal { .. }));
        }
    }
}
