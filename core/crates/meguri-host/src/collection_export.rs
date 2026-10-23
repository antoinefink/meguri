//! Host inputs for the collection-only export worker. This never gates on study admission.
use crate::{CoreHost, HostConfig};
use meguri_engine::{CancelToken, Collection, EngineError};
use meguri_package::{CollectionPackageWriteContext, Finalisation, LiveCollectionPackageWritten};
use meguri_storage::{ReaderRole, atomic_file::Purpose};
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
        self.review().flush_for_background()?;
        progress.set_phase(meguri_engine::ProgressPhase::Named("Capturing and writing collection"));
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
        let operation = directory.join(format!("operation-{:016x}", config.random.next_u64()));
        let path = operation.join("Meguri-Collection.colpkg");
        let reader = collection.reader(ReaderRole::Maintenance)?;
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
        // The actual DB+WAL P10 budget precedes every output mutation, including recovery.
        meguri_package::preflight_collection_package_from_live(&reader, &path, &context).map_err(package_error)?;
        cancel.check()?;
        if directory.symlink_metadata().is_ok() {
            // Only a durable never-handed-off/completed marker permits recovery.
            // Unknown/old/provider-owned slots remain fenced, with a truthful refusal.
            if ownership(&directory).is_ok_and(|(_, state)| state == "safe") {
                remove_collection_export(&directory, &protected)?;
            } else {
                return Err(EngineError::Busy { running_op: "a retained Files export; finish its provider operation before retrying (ownership cannot be proven after restart)".into() });
            }
        }
        fs::create_dir_all(&root).map_err(io)?;
        fs::create_dir(&directory).map_err(io)?;
        struct Unfinished<'a>(&'a Path, &'a [PathBuf], bool);
        impl Drop for Unfinished<'_> {
            fn drop(&mut self) {
                if !self.2 {
                    let _ = remove_collection_export(self.0, self.1);
                    let _ = fs::remove_dir(self.0); // empty, unmarked setup failure only
                }
            }
        }
        let mut unfinished = Unfinished(&directory, &protected, false);
        write_ownership(&directory, operation.file_name().unwrap().to_str().unwrap(), "safe")?;
        fs::create_dir(&operation).map_err(io)?;
        meguri_storage::atomic_file::sync_directory(&directory)?;
        meguri_storage::atomic_file::sync_directory(&root)?;
        meguri_storage::atomic_file::sync_directory(parent)?;
        let written =
            meguri_package::write_collection_package_from_live(&reader, &path, &context).map_err(package_error)?;
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
            written,
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
        meguri_package::CollectionPackageWriteError::Cancelled => EngineError::Cancelled,
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
fn ownership(directory: &Path) -> Result<(String, String), EngineError> {
    // Reject redirected slots and markers; never recurse through aliases.
    if !directory.symlink_metadata().map_err(io)?.file_type().is_dir() {
        return Err(invalid());
    }
    let marker = directory.join(MARKER);
    let meta = marker.symlink_metadata().map_err(io)?;
    if !meta.file_type().is_file() || meta.len() > 128 {
        return Err(invalid());
    }
    let content = fs::read_to_string(marker).map_err(io)?;
    let lines: Vec<_> = content.lines().collect();
    if lines.len() != 3
        || lines[0] != "meguri-export-v1"
        || !["safe", "provider"].contains(&lines[1])
        || lines[2].len() != 26
        || !lines[2].starts_with("operation-")
        || !lines[2][10..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid());
    }
    Ok((lines[2].into(), lines[1].into()))
}
fn write_ownership(directory: &Path, operation: &str, state: &str) -> Result<(), EngineError> {
    let mut file = meguri_storage::atomic_file::AtomicFile::new(&directory.join(MARKER), Purpose::Export)?;
    file.file_mut()
        .write_all(format!("meguri-export-v1\n{state}\n{operation}\n").as_bytes())
        .map_err(io)?;
    file.publish(|_| Ok(())).map_err(Into::into)
}
/// Worker-only: durably fence recovery BEFORE exposing any native source URL.
pub fn collection_export_provider_state(directory: &Path, active: bool) -> Result<(), EngineError> {
    let (operation, _) = ownership(directory)?;
    write_ownership(directory, &operation, if active { "provider" } else { "safe" })
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
    phase: RemovalPhase,
}
#[derive(PartialEq, Eq)]
enum RemovalPhase {
    Marker,
    Directory,
    RootSync,
    Done,
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
            let (operation, _) = ownership(directory)?;
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
                    for entry in fs::read_dir(&output).map_err(io)? {
                        let entry = entry.map_err(io)?;
                        if entry.file_name() != "Meguri-Collection.colpkg" || !entry.file_type().map_err(io)?.is_file()
                        {
                            return Err(invalid());
                        }
                    }
                    let package = output.join("Meguri-Collection.colpkg");
                    if package.symlink_metadata().is_ok() {
                        fs::remove_file(package).map_err(io)?;
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
                    if ownership(directory)?.0 != proof.operation {
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
