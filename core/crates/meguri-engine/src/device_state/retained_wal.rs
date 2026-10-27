//! Restore-only proof for bytes which cannot pass through a live checkpoint.
//! The R6 intent owns every transition; these files are retained recovery data.
use super::*;
use meguri_storage::atomic_file;
use std::fs;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BoundFile {
    path: PathBuf,
    bytes: FileIdentity,
    native: NativeIdentity,
}
impl BoundFile {
    fn capture(path: &Path) -> Result<Self, StorageError> {
        let (bytes, native) = fingerprint_native(path)?.ok_or_else(|| failure("raw evidence absent"))?;
        Ok(Self {
            path: path.to_owned(),
            bytes,
            native,
        })
    }
    fn check(&self) -> Result<(), StorageError> {
        if fingerprint_native(&self.path)? != Some((self.bytes.clone(), self.native.clone())) {
            return Err(failure("raw evidence binding or bytes changed"));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct RootBinding {
    path: PathBuf,
    native: NativeIdentity,
}
fn roots(path: &Path) -> Result<Vec<RootBinding>, StorageError> {
    use std::os::unix::fs::MetadataExt;
    if !path.is_absolute() || path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err(failure("raw evidence requires an absolute root"));
    }
    let mut result = Vec::new();
    for ancestor in path.ancestors() {
        let m = fs::symlink_metadata(ancestor)?;
        if !m.is_dir() {
            return Err(failure("raw evidence root is not a no-follow directory"));
        }
        result.push(RootBinding {
            path: ancestor.to_owned(),
            native: NativeIdentity {
                device: m.dev(),
                inode: m.ino(),
            },
        });
    }
    Ok(result)
}

/// Constructed only by verifying synchronized independent T7 copies against
/// their still-owned sources. The initial Restore consumes this value once.
#[derive(Clone, Debug)]
pub struct RawRestoreAuthority(RawPair);
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RawPair {
    source_main: BoundFile,
    source_wal: BoundFile,
    copy_main: BoundFile,
    copy_wal: BoundFile,
    roots: Vec<RootBinding>,
}
impl RawRestoreAuthority {
    pub fn verify(live: &Path, copy_main: &Path, copy_wal: &Path) -> Result<Self, StorageError> {
        let mut bindings = roots(live.parent().ok_or_else(|| failure("raw source root absent"))?)?;
        let backup_root = copy_main.parent().ok_or_else(|| failure("raw copy root absent"))?;
        if copy_wal.parent() != Some(backup_root) {
            return Err(failure("raw copies have different roots"));
        }
        bindings.extend(roots(backup_root)?);
        let authority = RawPair {
            source_main: BoundFile::capture(live)?,
            source_wal: BoundFile::capture(&live.with_file_name("collection.anki2-wal"))?,
            copy_main: BoundFile::capture(copy_main)?,
            copy_wal: BoundFile::capture(copy_wal)?,
            roots: bindings,
        };
        if authority.source_wal.bytes.length == 0 {
            return Err(failure("raw WAL authority requires retained bytes"));
        }
        let files = [
            &authority.source_main,
            &authority.source_wal,
            &authority.copy_main,
            &authority.copy_wal,
        ];
        for (i, file) in files.iter().enumerate() {
            if files[..i].iter().any(|prior| prior.native == file.native) {
                return Err(failure("raw evidence aliases another file"));
            }
            atomic_file::full_sync(&fs::File::open(&file.path)?)?;
        }
        atomic_file::sync_directory(backup_root)?;
        atomic_file::sync_directory(live.parent().ok_or_else(|| failure("raw source root absent"))?)?;
        authority.check_copies()?;
        authority.source_main.check()?;
        authority.source_wal.check()?;
        Ok(Self(authority))
    }
}
impl RawPair {
    fn check_copies(&self) -> Result<(), StorageError> {
        let source_root = self
            .source_main
            .path
            .parent()
            .ok_or_else(|| failure("missing raw source root"))?;
        let copy_root = self
            .copy_main
            .path
            .parent()
            .ok_or_else(|| failure("missing raw copy root"))?;
        if self.source_wal.path != self.source_main.path.with_file_name("collection.anki2-wal")
            || self.copy_wal.path.parent() != Some(copy_root)
            || source_root.starts_with(copy_root)
            || copy_root.starts_with(source_root)
        {
            return Err(failure("raw source and copy namespaces are not disjoint"));
        }
        let mut current = roots(source_root)?;
        current.extend(roots(copy_root)?);
        if current != self.roots {
            return Err(failure("raw evidence directory binding changed"));
        }
        let files = [&self.source_main, &self.source_wal, &self.copy_main, &self.copy_wal];
        for (i, file) in files.iter().enumerate() {
            if files[..i].iter().any(|prior| prior.native == file.native) {
                return Err(failure("raw evidence aliases another file"));
            }
        }
        if self.source_main.bytes != self.copy_main.bytes || self.source_wal.bytes != self.copy_wal.bytes {
            return Err(failure("raw copies do not match the recorded source pair"));
        }
        self.copy_main.check()?;
        self.copy_wal.check()?;
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum WalDisposition {
    Retiring,
    Retired,
    Restoring,
    ResolvedOld,
    ResolvedCandidate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct WalRecovery {
    operation: PublicationId,
    authority: RawPair,
    retired: PathBuf,
    disposition: WalDisposition,
}
impl WalRecovery {
    pub(super) fn prepare(
        live: &Path,
        authority: RawRestoreAuthority,
        operation: PublicationId,
        old: Option<&FileIdentity>,
        candidate: &FileIdentity,
    ) -> Result<Self, StorageError> {
        let authority = authority.0;
        authority.check_copies()?;
        authority.source_main.check()?;
        authority.source_wal.check()?;
        if authority.source_main.path != live || old != Some(&authority.source_main.bytes) || old == Some(candidate) {
            return Err(failure("raw authority or distinct main branch cannot be proven"));
        }
        let retired = live.with_file_name(format!(
            ".meguri-restore-wal-{:016x}{:016x}.recovery",
            operation.0[0], operation.0[1]
        ));
        match fs::symlink_metadata(&retired) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
            Ok(_) => return Err(failure("retained WAL destination exists")),
        }
        Ok(Self {
            operation,
            authority,
            retired,
            disposition: WalDisposition::Retiring,
        })
    }
    fn locations(&self, live: &Path, intent: &Intent) -> Result<bool, StorageError> {
        if self.operation != intent.operation
            || intent.reason != crate::replace::ReplaceReason::Restore
            || self.authority.source_main.path != live
            || intent.old_file.as_ref() != Some(&self.authority.source_main.bytes)
            || intent.old_native.as_ref() != Some(&self.authority.source_main.native)
            || self.authority.source_wal.path != live.with_file_name("collection.anki2-wal")
            || self.retired
                != live.with_file_name(format!(
                    ".meguri-restore-wal-{:016x}{:016x}.recovery",
                    self.operation.0[0], self.operation.0[1]
                ))
        {
            return Err(failure("WAL authority belongs to another operation"));
        }
        self.authority.check_copies()?;
        let expected = Some((
            self.authority.source_wal.bytes.clone(),
            self.authority.source_wal.native.clone(),
        ));
        let original = fingerprint_native(&self.authority.source_wal.path)?;
        let retired = fingerprint_native(&self.retired)?;
        match (original, retired) {
            (Some(pair), None) if Some(pair) == expected => Ok(false),
            (None, Some(pair)) if Some(pair) == expected => Ok(true),
            _ => Err(failure("WAL locations are missing, substituted or ambiguous")),
        }
    }
}
fn branch(live: &Path, intent: &Intent) -> Result<bool, StorageError> {
    // Main proof precedes all WAL location decisions. Equal bytes cannot select
    // the old view, even when the inode differs.
    if intent.old_file.as_ref() == Some(&intent.candidate_file) {
        return Err(failure("equal main bytes leave WAL recovery ambiguous"));
    }
    let current = fingerprint_native(live)?;
    let old = current.as_ref().map(|(b, _)| b) == intent.old_file.as_ref()
        && current.as_ref().map(|(_, n)| n) == intent.old_native.as_ref();
    let candidate = current.as_ref().map(|(b, _)| b) == Some(&intent.candidate_file)
        && current.as_ref().map(|(_, n)| n) == intent.candidate_native.as_ref();
    if old == candidate || (intent.phase == Phase::WalActivated && !candidate) {
        return Err(failure("main branch or WAL phase is ambiguous"));
    }
    Ok(candidate)
}
fn saved(live: &Path) -> Result<(State, Intent, WalRecovery), StorageError> {
    let state = sidecar::read_document(&sidecar_path(live), decode)?.ok_or_else(|| failure("missing WAL state"))?;
    let intent = state.intent.clone().ok_or_else(|| failure("missing WAL intent"))?;
    let wal = intent.wal.clone().ok_or_else(|| failure("missing WAL authority"))?;
    Ok((state, intent, wal))
}
fn record(live: &Path, wal: &WalRecovery) -> Result<(), StorageError> {
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing WAL state"))?;
        let intent = state.intent.as_mut().ok_or_else(|| failure("missing WAL intent"))?;
        if intent.operation != wal.operation || intent.wal.is_none() {
            return Err(failure("WAL operation changed"));
        }
        intent.wal = Some(wal.clone());
        put(doc, &state)
    })
}
fn move_owned(live: &Path, wal: &WalRecovery, source: &Path, destination: &Path) -> Result<(), StorageError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let root = live.parent().ok_or_else(|| failure("missing WAL root"))?;
    let expected = wal
        .authority
        .roots
        .iter()
        .find(|r| r.path == root)
        .ok_or_else(|| failure("missing WAL root binding"))?;
    let (state, intent, _) = saved(live)?;
    bound(&state, &state.active.binding)?;
    if branch(live, &intent)? {
        return Err(failure("a candidate cannot receive or retire the old WAL"));
    }
    wal.locations(live, &intent)?;
    if fingerprint_native(source)?
        != Some((
            wal.authority.source_wal.bytes.clone(),
            wal.authority.source_wal.native.clone(),
        ))
        || fingerprint_native(destination)?.is_some()
    {
        return Err(failure("WAL move source or destination changed"));
    }
    wal.authority.check_copies()?;
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(root)?;
    let m = directory.metadata()?;
    if expected.native
        != (NativeIdentity {
            device: m.dev(),
            inode: m.ino(),
        })
        || source.parent() != Some(root)
        || destination.parent() != Some(root)
    {
        return Err(failure("WAL recovery root changed"));
    }
    atomic_file::move_new_in(
        &directory,
        source.file_name().ok_or_else(|| failure("missing WAL source name"))?,
        destination
            .file_name()
            .ok_or_else(|| failure("missing WAL destination name"))?,
    )?;
    wal.authority.check_copies()
}
/// Intent was durably published with Retiring before this first move.
pub(super) fn retire(live: &Path) -> Result<(), StorageError> {
    let (state, intent, mut wal) = saved(live)?;
    bound(&state, &state.active.binding)?;
    if branch(live, &intent)? || intent.phase != Phase::WalIntent {
        return Err(failure("moving the old WAL needs a confirmed original database"));
    }
    if wal.disposition != WalDisposition::Retiring || wal.locations(live, &intent)? {
        return Err(failure("the saved old WAL state does not permit a move"));
    }
    move_owned(live, &wal, &wal.authority.source_wal.path, &wal.retired)?;
    meguri_model::faultpoint!("replace.wal.after_retire")?;
    if !wal.locations(live, &intent)? {
        return Err(failure("the old WAL was not retained at its recovery path"));
    }
    wal.disposition = WalDisposition::Retired;
    record(live, &wal)?;
    meguri_model::faultpoint!("replace.wal.after_retired_record")?;
    Ok(())
}
pub(super) fn verify_retired(live: &Path, intent: &Intent) -> Result<(), StorageError> {
    if let Some(wal) = &intent.wal {
        if wal.disposition != WalDisposition::Retired || !wal.locations(live, intent)? {
            return Err(failure("the retained old WAL no longer matches the saved swap state"));
        }
    }
    Ok(())
}
pub(super) fn reconcile_wal(live: &Path, state: State, binding: &AccountBinding) -> Result<(), StorageError> {
    let intent = state.intent.as_ref().ok_or_else(|| failure("missing WAL intent"))?;
    let mut wal = intent.wal.clone().ok_or_else(|| failure("missing WAL authority"))?;
    let candidate = branch(live, intent)?;
    let journal = live.with_file_name("collection.anki2-journal");
    match fs::symlink_metadata(journal) {
        Ok(m) if !m.is_file() || m.len() != 0 => return Err(failure("unexpected rollback journal")),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let moved = wal.locations(live, intent)?;
    if candidate {
        if !moved
            || !matches!(
                wal.disposition,
                WalDisposition::Retired | WalDisposition::ResolvedCandidate
            )
        {
            return Err(failure("candidate has inconsistent WAL disposition"));
        }
        atomic_file::sync_directory(live.parent().ok_or_else(|| failure("missing collection root"))?)?;
        wal.disposition = WalDisposition::ResolvedCandidate;
        record(live, &wal)?;
        meguri_model::faultpoint!("replace.wal.after_resolution")?;
        activate(live, binding)
    } else {
        match wal.disposition {
            WalDisposition::Retiring => {}
            WalDisposition::Retired if moved => {}
            WalDisposition::Restoring => {}
            WalDisposition::ResolvedOld if !moved => {}
            _ => return Err(failure("old branch has inconsistent WAL disposition")),
        }
        wal.disposition = WalDisposition::Restoring;
        record(live, &wal)?;
        meguri_model::faultpoint!("replace.wal.before_restore_move")?;
        if moved {
            move_owned(live, &wal, &wal.retired, &wal.authority.source_wal.path)?;
            meguri_model::faultpoint!("replace.wal.after_restore_move")?;
        }
        if wal.locations(live, intent)? {
            return Err(failure("old WAL was not restored"));
        }
        atomic_file::sync_directory(live.parent().ok_or_else(|| failure("missing collection root"))?)?;
        wal.disposition = WalDisposition::ResolvedOld;
        record(live, &wal)?;
        meguri_model::faultpoint!("replace.wal.after_resolution")?;
        complete(live, binding)
    }
}
pub(super) fn complete_wal(live: &Path, state: &mut State) -> Result<(), StorageError> {
    let Some(intent) = state.intent.as_ref() else {
        return Ok(());
    };
    let Some(wal) = intent.wal.as_ref() else { return Ok(()) };
    let expected = match wal.disposition {
        WalDisposition::ResolvedOld => &intent.prior,
        WalDisposition::ResolvedCandidate => &intent.candidate,
        _ => return Err(failure("WAL recovery has not completed")),
    };
    if &state.active != expected {
        return Err(failure("WAL decision differs from publication state"));
    }
    let candidate = wal.disposition == WalDisposition::ResolvedCandidate;
    if branch(live, intent)? != candidate || wal.locations(live, intent)? != candidate {
        return Err(failure("WAL completion no longer proves its selected branch"));
    }
    atomic_file::sync_directory(live.parent().ok_or_else(|| failure("missing WAL root"))?)?;
    let evidence = serde_json::to_value(intent).map_err(failure)?;
    state
        .extra
        .entry("retainedWalReplacements".into())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| failure("invalid WAL completion evidence"))?
        .push(evidence);
    Ok(())
}
