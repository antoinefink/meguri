//! C12.5's whole-file exception is reachable only from the real opening step.
use super::*;
use crate::{Collection, LifecycleState, WeakEngine, executor::exclusive::Privilege};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{MetadataExt, OpenOptionsExt},
};

struct Opening {
    engine: WeakEngine,
    executor: std::sync::Weak<crate::executor::ExecShared>,
    privilege: Privilege,
    generation: Generation,
    native: (u64, u64),
    persistent: PersistentIdentity,
    canonical: Vec<u8>,
}
impl Opening {
    fn require(&self) -> Result<(), EngineError> {
        let engine = self.engine.upgrade().ok_or_else(identity_mismatch)?;
        let exec = self.executor.upgrade().ok_or_else(identity_mismatch)?;
        if engine.state() != LifecycleState::Opening || exec.generation() != self.generation {
            return Err(identity_mismatch());
        }
        exec.require_admission(Some(self.privilege))?;
        let binding = exec
            .control(crate::suspend::Control::ReviewIdentity)
            .wait()?
            .review_identity
            .ok_or_else(identity_mismatch)?;
        if binding.0 != self.native || binding.1 != self.persistent {
            return Err(identity_mismatch());
        }
        require_live(&exec.path, self.native, &self.canonical)?;
        Ok(())
    }
}

// Hold the directory descriptor alongside the journal descriptors; do not reopen SQLite.
struct Directory {
    path: PathBuf,
    file: File,
}
impl Directory {
    fn read(path: &Path) -> Result<Self, EngineError> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(path)
            .map_err(error)?;
        let value = Self {
            path: path.to_owned(),
            file,
        };
        value.require()?;
        Ok(value)
    }
    fn require(&self) -> Result<(), EngineError> {
        let held = self.file.metadata().map_err(error)?;
        let named = std::fs::metadata(&self.path).map_err(error)?;
        if !held.is_dir() || !named.is_dir() || (held.dev(), held.ino()) != (named.dev(), named.ino()) {
            return Err(identity_mismatch());
        }
        Ok(())
    }
    fn require_file(&self, path: &Path) -> Result<(), EngineError> {
        self.require()?;
        recovery_file(path)?;
        Ok(())
    }
}

fn first_header(bytes: &[u8]) -> Option<Header> {
    let end = bytes.iter().position(|b| *b == b'\n')?;
    let frame: Frame = serde_json::from_slice(&bytes[..end]).ok()?;
    if frame.number != 1 || frame.checksum != checksum(1, frame.payload.as_bytes()) {
        return None;
    }
    match serde_json::from_str(&frame.payload).ok()? {
        Record::Header(h) => Some(h),
        _ => None,
    }
}
fn pristine(bytes: &[u8]) -> Option<Header> {
    // Prefix recovery deliberately is not used as proof of exact EOF.
    if bytes.last() != Some(&b'\n') || bytes.iter().filter(|b| **b == b'\n').count() != 1 {
        return None;
    }
    let h = first_header(bytes)?;
    if h.version != 2
        || h.next_event != 1
        || h.notice.is_some()
        || !matches!(h.native, Native::Persistent(identity) if identity.valid_for_platform())
    {
        return None;
    }
    Some(h)
}
fn same_darwin_file(header: &Header, persistent: PersistentIdentity, canonical: &[u8]) -> bool {
    header.version == 2
        && header.path != canonical
        && Path::new(std::ffi::OsStr::from_bytes(&header.path)).is_absolute()
        && matches!(header.native, Native::Persistent(identity @ PersistentIdentity::Darwin { .. })
            if identity.valid_for_platform() && identity == persistent)
}
fn archive_path(collection: &Path, hash: &[u8; 32]) -> PathBuf {
    let name: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    collection.with_file_name(format!("meguri-pending-answers.{name}.identity.jsonl"))
}
fn retained_bytes(retained: &RecoveryBinding) -> Result<(), EngineError> {
    use std::os::unix::fs::FileExt;
    let mut observed = vec![0; retained.bytes.len()];
    retained.file.read_exact_at(&mut observed, 0).map_err(error)?;
    let length = retained.file.metadata().map_err(error)?.len();
    if length != observed.len() as u64 || observed != retained.bytes {
        return Err(identity_mismatch());
    }
    Ok(())
}
fn verify_migration_archive(
    collection: &Path,
    current: &Header,
    directory: &Directory,
) -> Result<Option<RecoveryBinding>, EngineError> {
    let Some(notice) = current
        .notice
        .as_ref()
        .filter(|notice| notice.reason == MigrationReason::PristineV2Rebound)
    else {
        return Ok(None);
    };
    let location = archive_path(collection, &notice.archive_sha256);
    directory.require_file(&location)?;
    let retained = RecoveryBinding::read(&location)?;
    let previous = pristine(&retained.bytes).ok_or_else(identity_mismatch)?;
    let digest: [u8; 32] = Sha256::digest(&retained.bytes).into();
    if digest != notice.archive_sha256
        || previous.incarnation == current.incarnation
        || (previous.path == current.path && previous.native == current.native)
    {
        return Err(identity_mismatch());
    }
    retained.sync(&location)?;
    directory.require_file(&location)?;
    Ok(Some(retained))
}

// Keep this evidence through Migrate, including recovery and compaction.
// Drop it before Ready so later reviews and resumes cannot reuse the exception.
pub(super) struct Evidence {
    proof: Opening,
    directory: Directory,
    original: Option<RecoveryBinding>,
    archive: Option<(RecoveryBinding, PathBuf)>,
    active: RecoveryBinding,
    path: PathBuf,
}
impl Evidence {
    pub(super) fn require(&self) -> Result<(), EngineError> {
        self.proof.require()?;
        self.require_files()
    }
    fn require_files(&self) -> Result<(), EngineError> {
        let exec = self.proof.executor.upgrade().ok_or_else(identity_mismatch)?;
        require_live(&exec.path, self.proof.native, &self.proof.canonical)?;
        self.directory.require_file(&self.path)?;
        if let Some((archive, path)) = &self.archive {
            self.directory.require_file(path)?;
            archive.require(path)?;
        }
        if let Some(original) = &self.original {
            retained_bytes(original)?;
            let metadata = original.file.metadata().map_err(error)?;
            if metadata.nlink() != 0 {
                return Err(identity_mismatch());
            }
        }
        self.active.require(&self.path)?;
        Ok(())
    }
    pub(super) fn require_append(&self, file: &File) -> Result<(), EngineError> {
        // The writer can be awaiting this replay receipt. Avoid a control call
        // here; initialization, compaction and Migrate check the SQLite binding.
        self.require_files()?;
        same_file(file, &self.active.file)
    }
    pub(super) fn truncated(&mut self, complete: usize) {
        self.active.bytes.truncate(complete);
    }
    pub(super) fn appended(&mut self, bytes: &[u8]) {
        self.active.bytes.extend(bytes);
    }
    fn reset_retired(&mut self, header: &Header) -> Result<RecoveryBinding, EngineError> {
        self.require()?;
        let encoded = encode(1, &Record::Header(header.clone()))?;
        let mut staged = AtomicFile::new(&self.path, Purpose::Sidecar)?;
        staged.file_mut().write_all(&encoded).map_err(error)?;
        let published = RecoveryBinding {
            file: staged.file_mut().try_clone().map_err(error)?,
            bytes: encoded,
        };
        staged.publish(|_| {
            self.require().map_err(|_| meguri_storage::StorageError::Io {
                detail: "pending_answer_identity_mismatch".into(),
            })
        })?;
        // Keep the qualified retired descriptor through the final barrier;
        // the replacement is the staged descriptor, never a newly chosen file.
        self.original = Some(std::mem::replace(&mut self.active, published));
        self.active.sync(&self.path)?;
        self.require()?;
        Ok(RecoveryBinding {
            file: self.active.file.try_clone().map_err(error)?,
            bytes: self.active.bytes.clone(),
        })
    }
    pub(super) fn compact(&mut self, header: &Header) -> Result<(), EngineError> {
        self.require()?;
        let encoded = encode(1, &Record::Header(header.clone()))?;
        let mut staged = AtomicFile::new(&self.path, Purpose::Sidecar)?;
        staged.file_mut().write_all(&encoded).map_err(error)?;
        let published = RecoveryBinding {
            file: staged.file_mut().try_clone().map_err(error)?,
            bytes: encoded,
        };
        staged.publish(|_| {
            self.require().map_err(|_| meguri_storage::StorageError::Io {
                detail: "pending_answer_identity_mismatch".into(),
            })
        })?;
        published.sync(&self.path)?;
        self.active = published;
        self.require()
    }
}

pub(in crate::session) fn open(
    c: &Collection,
    engine: &WeakEngine,
    threads: &ThreadRegistry,
) -> Result<Arc<Journal>, EngineError> {
    let (native, persistent, incarnation) = super::super::service::native_binding(c)?;
    let canonical = c.path().canonicalize().map_err(error)?.as_os_str().as_bytes().to_vec();
    let path = c.path().with_file_name("meguri-pending-answers.jsonl");
    // Use the established recovery path except for a qualified C12.5 mismatch.
    let recover_existing = || {
        Journal::open(
            c.path(),
            native,
            persistent,
            incarnation,
            c.generation(),
            threads,
            || super::super::service::native_binding(c).map(|b| b.2),
        )
    };
    if engine.upgrade().is_none_or(|e| e.state() != LifecycleState::Opening) {
        return recover_existing();
    }
    let original = match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return recover_existing(),
        Err(e) => return Err(error(e)),
        Ok(_) => RecoveryBinding::read(&path)?,
    };
    let eligible_header = pristine(&original.bytes);
    if eligible_header
        .as_ref()
        .is_some_and(|header| header.path == canonical && header.native == Native::Persistent(persistent))
    {
        return recover_existing();
    }
    let retained_header = first_header(&original.bytes);
    let migration_header = retained_header.as_ref().filter(|header| {
        header
            .notice
            .as_ref()
            .is_some_and(|notice| notice.reason == MigrationReason::PristineV2Rebound)
    });
    let same_file_path = retained_header
        .as_ref()
        .is_some_and(|header| same_darwin_file(header, persistent, &canonical));
    if eligible_header.is_none() && migration_header.is_none() && !same_file_path {
        return recover_existing();
    }
    let proof = Opening {
        engine: engine.clone(),
        executor: Arc::downgrade(&c.core.exec),
        privilege: c.privilege.ok_or_else(identity_mismatch)?,
        generation: c.generation(),
        native,
        persistent,
        canonical: canonical.clone(),
    };
    proof.require()?;
    let directory = Directory::read(path.parent().ok_or_else(identity_mismatch)?)?;
    directory.require_file(&path)?;
    original.require(&path)?;
    // An exact unused v2 source always takes the existing fresh-binding path.
    if eligible_header.is_none() {
        let h = retained_header.ok_or_else(identity_mismatch)?;
        if h.native != Native::Persistent(persistent) || (h.path != canonical && !same_file_path) {
            return Err(identity_mismatch());
        }
        // Historical-path decoding is confined to the exact Darwin authority
        // above. The full ordinary codec still validates every complete frame.
        let (mut loaded, mut number, mut complete) = read_bytes_bound(&original.bytes, &h, native)?;
        if loaded.retired && h.path != canonical {
            return Err(identity_mismatch());
        }
        let archive = match &h.notice {
            Some(notice) if notice.reason == MigrationReason::PristineV2Rebound => Some((
                verify_migration_archive(c.path(), &h, &directory)?.ok_or_else(identity_mismatch)?,
                archive_path(c.path(), &notice.archive_sha256),
            )),
            _ => None,
        };
        original.require(&path)?;
        proof.require()?;
        let active = RecoveryBinding {
            file: original.file.try_clone().map_err(error)?,
            bytes: original.bytes.clone(),
        };
        let mut evidence = Evidence {
            proof,
            directory,
            original: None,
            archive,
            active,
            path,
        };
        let original = if loaded.retired {
            let next_incarnation = if incarnation == h.incarnation {
                super::super::service::native_binding(c)?.2
            } else {
                incarnation
            };
            if next_incarnation == h.incarnation {
                return Err(identity_mismatch());
            }
            let next = Header {
                version: 2,
                path: canonical.clone(),
                native: Native::Persistent(persistent),
                incarnation: next_incarnation,
                next_event: 1,
                notice: None,
            };
            let reset = evidence.reset_retired(&next)?;
            (loaded, number, complete) = read_bytes_bound(&reset.bytes, &next, native)?;
            reset
        } else {
            original
        };
        return Journal::initialize_retained(
            CurrentBinding {
                collection: c.path(),
                native,
                canonical: &canonical,
            },
            c.generation(),
            threads,
            (original, loaded, number, complete),
            None,
            Some(evidence),
        );
    }
    let old = eligible_header.ok_or_else(identity_mismatch)?;
    let next_incarnation = if incarnation == old.incarnation {
        super::super::service::native_binding(c)?.2
    } else {
        incarnation
    };
    if next_incarnation == old.incarnation {
        return Err(identity_mismatch());
    }
    let next = Header {
        version: 2,
        path: canonical,
        native: Native::Persistent(persistent),
        incarnation: next_incarnation,
        next_event: 1,
        notice: None,
    };
    let loaded = read_bytes_bound(&original.bytes, &old, native)?.0;
    let migration = Migration {
        collection: c.path().to_owned(),
        live: native,
        current_bytes: original.bytes.clone(),
        original,
        next,
        reason: MigrationReason::PristineV2Rebound,
    };
    let state = Arc::new(Mutex::new(loaded));
    let shared = state.clone();
    let (tx, rx) = mpsc::channel();
    let worker_path = path.clone();
    // Bootstrap on the existing registered file owner, before its append loop.
    // A Barrier uses that same queue to acknowledge initialization; no journal
    // handle is published to recovery/admission until it succeeds.
    let worker = threads
        .spawn_owned(
            "meguri-review-file",
            Qos::UserInitiated,
            "review-session",
            move |activity| {
                let initialized = (|| {
                    let snapshot = shared.lock().unwrap().clone();
                    let archive = migration.archive_path();
                    let archive_binding = std::cell::RefCell::new(None::<RecoveryBinding>);
                    let check = || {
                        proof.require()?;
                        directory.require()?;
                        retained_bytes(&migration.original)?;
                        match std::fs::symlink_metadata(&archive) {
                            Err(e)
                                if e.kind() == std::io::ErrorKind::NotFound && archive_binding.borrow().is_none() => {}
                            Err(e) => return Err(error(e)),
                            Ok(_) => {
                                directory.require_file(&archive)?;
                                let mut binding = archive_binding.borrow_mut();
                                if binding.is_none() {
                                    let archived = RecoveryBinding::read(&archive)?;
                                    if archived.bytes != migration.original.bytes {
                                        return Err(identity_mismatch());
                                    }
                                    *binding = Some(archived);
                                }
                                binding.as_ref().unwrap().require(&archive)?;
                            }
                        }
                        Ok(())
                    };
                    let (h, landed) = migration.publish_checked(&worker_path, &snapshot, || {}, check)?;
                    proof.require()?;
                    directory.require_file(&worker_path)?;
                    let archived = archive_binding.into_inner().ok_or_else(identity_mismatch)?;
                    archived.require(&archive)?;
                    directory.require_file(&archive)?;
                    retained_bytes(&migration.original)?;
                    if migration.original.file.metadata().map_err(error)?.nlink() != 0 {
                        return Err(identity_mismatch());
                    }
                    landed.require(&worker_path)?;
                    archived.require(&archive)?;
                    let append = append_file(&worker_path)?;
                    same_file(&append, &landed.file)?;
                    landed.require(&worker_path)?;
                    proof.require()?;
                    let loaded = read_bytes_bound(&landed.bytes, &h, native)?.0;
                    *shared.lock().unwrap() = loaded;
                    let evidence = Evidence {
                        proof,
                        directory,
                        original: Some(migration.original),
                        archive: Some((archived, archive)),
                        active: landed,
                        path: worker_path.clone(),
                    };
                    Ok::<_, EngineError>((append, evidence))
                })();
                match initialized {
                    Ok((file, evidence)) => {
                        file_loop(rx, file, worker_path, shared, 1, (None, Some(evidence)), activity)
                    }
                    Err(failure) => {
                        shared.lock().unwrap().failure = Some(failure);
                    }
                }
            },
        )
        .map_err(error)?;
    let journal = Arc::new(Journal {
        tx,
        state,
        worker: Mutex::new(Some(worker)),
        generation: c.generation(),
        live_native: native,
    });
    if let Err(failure) = journal.barrier() {
        let saved = journal.state.lock().unwrap().failure.clone().unwrap_or(failure);
        return Err(saved);
    }
    journal.check(c.generation())?;
    Ok(journal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            version: 2,
            path: b"/synthetic/old.anki2".to_vec(),
            native: Native::Persistent(identity(9)),
            incarnation: [1, 2],
            next_event: 1,
            notice: None,
        }
    }
    fn identity(inode: u64) -> PersistentIdentity {
        #[cfg(target_vendor = "apple")]
        {
            PersistentIdentity::Darwin {
                volume_uuid: [3; 16],
                inode,
            }
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            PersistentIdentity::NonDarwin { device: 7, inode }
        }
    }
    fn payload(value: serde_json::Value) -> Vec<u8> {
        let text = serde_json::to_string(&value).unwrap();
        let mut bytes = serde_json::to_vec(&Frame {
            number: 1,
            checksum: checksum(1, text.as_bytes()),
            payload: text,
        })
        .unwrap();
        bytes.push(b'\n');
        bytes
    }
    #[test]
    fn c12_pristine_qualifier_rejects_history_notice_legacy_foreign_and_invalid_headers() {
        let h = header();
        assert_eq!(
            pristine(&encode(1, &Record::Header(h.clone())).unwrap()),
            Some(h.clone())
        );
        for case in [
            "zero",
            "highwater",
            "notice",
            "null_notice",
            "v1",
            "future",
            "foreign",
            "unknown_header",
            "unknown_native",
            "unknown_variant",
            "short_incarnation",
            "negative_counter",
        ] {
            let mut value = serde_json::to_value(Record::Header(h.clone())).unwrap();
            let v = &mut value["Header"];
            match case {
                "zero" => v["next_event"] = 0.into(),
                "highwater" => v["next_event"] = 2.into(),
                "notice" => {
                    v["notice"] = serde_json::to_value(MigrationNotice {
                        reason: MigrationReason::PristineV2Rebound,
                        archive_sha256: [5; 32],
                    })
                    .unwrap()
                }
                "null_notice" => v["notice"] = serde_json::Value::Null,
                "v1" => {
                    v["version"] = 1.into();
                    v["native"] = serde_json::json!([7, 9]);
                }
                "future" => v["version"] = 99.into(),
                "foreign" => {
                    v["native"] = if cfg!(target_vendor = "apple") {
                        serde_json::json!({"platform":"non_darwin","device":7,"inode":9})
                    } else {
                        serde_json::json!({"platform":"darwin","volume_uuid":[3,3,3,3,3,3,3,3,3,3,3,3,3,3,3,3],"inode":9})
                    }
                }
                "unknown_header" => v["extra"] = true.into(),
                "unknown_native" => v["native"]["extra"] = true.into(),
                "unknown_variant" => v["native"]["platform"] = "future".into(),
                "short_incarnation" => v["incarnation"] = serde_json::json!([1]),
                "negative_counter" => v["next_event"] = (-1).into(),
                _ => unreachable!(),
            }
            assert!(pristine(&payload(value)).is_none(), "{case}");
        }
    }
    #[test]
    fn c12_pristine_qualifier_requires_exact_complete_single_frame_and_checksum() {
        let bytes = encode(1, &Record::Header(header())).unwrap();
        for tail in [b"torn".as_slice(), b"\n", b" \n", b" ", b"{}\n"] {
            let mut bad = bytes.clone();
            bad.extend(tail);
            assert!(pristine(&bad).is_none());
        }
        assert!(pristine(&bytes[..bytes.len() - 1]).is_none());
        assert!(pristine(b"").is_none());
        assert!(pristine(&encode(2, &Record::Header(header())).unwrap()).is_none());
        let mut frame: Frame = serde_json::from_slice(&bytes[..bytes.len() - 1]).unwrap();
        frame.checksum[0] ^= 1;
        let mut bad = serde_json::to_vec(&frame).unwrap();
        bad.push(b'\n');
        assert!(pristine(&bad).is_none());
        frame.checksum = checksum(frame.number, frame.payload.as_bytes());
        let mut extra = serde_json::to_value(&frame).unwrap();
        extra["extra"] = true.into();
        let mut bad = serde_json::to_vec(&extra).unwrap();
        bad.push(b'\n');
        assert!(pristine(&bad).is_none());
        for record in [
            Record::Intent {
                event: 1,
                answer: super::super::super::frozen::test_answer(),
            },
            Record::Prepared {
                event: 1,
                receipt: Receipt {
                    row: [0; 9],
                    base: [0; 32],
                    post: [0; 32],
                },
            },
            Record::Terminal {
                event: 1,
                status: Terminal::Done,
            },
            Record::Terminal {
                event: 1,
                status: Terminal::Cancelled,
            },
            Record::Terminal {
                event: 1,
                status: Terminal::Dropped,
            },
            Record::Retired,
            Record::Header(header()),
        ] {
            let mut bad = bytes.clone();
            bad.extend(encode(2, &record).unwrap());
            assert!(pristine(&bad).is_none());
        }
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, Migration, State) {
        let sandbox = tempfile::tempdir().unwrap();
        let collection = sandbox.path().join("collection.anki2");
        std::fs::write(&collection, b"journal publication fixture").unwrap();
        let database_stat = std::fs::metadata(&collection).unwrap();
        let live = (database_stat.dev(), database_stat.ino());
        let mut next = header();
        next.path = collection.canonicalize().unwrap().as_os_str().as_bytes().to_vec();
        next.native = Native::Persistent(identity(live.1));
        #[cfg(not(target_vendor = "apple"))]
        {
            next.native = Native::Persistent(PersistentIdentity::NonDarwin {
                device: live.0,
                inode: live.1,
            });
        }
        next.incarnation = [31, 32];
        let previous_header = header();
        let initial_frame = encode(1, &Record::Header(previous_header.clone())).unwrap();
        let journal_path = collection.with_file_name("meguri-pending-answers.jsonl");
        std::fs::write(&journal_path, &initial_frame).unwrap();
        let decoded = read_bytes_bound(&initial_frame, &previous_header, live).unwrap().0;
        let publication = Migration {
            collection,
            live,
            original: RecoveryBinding::read(&journal_path).unwrap(),
            current_bytes: initial_frame,
            next,
            reason: MigrationReason::PristineV2Rebound,
        };
        (sandbox, journal_path, publication, decoded)
    }

    #[test]
    fn c12_unused_v2_archive_proof_survives_restart_and_compaction() {
        let (_dir, path, migration, state) = fixture();
        let initial_frame = migration.original.bytes.clone();
        let (published_header, landed) = migration.publish(&path, &state).unwrap();
        assert_eq!(published_header.next_event, 1);
        assert_ne!(published_header.incarnation, state.header.incarnation);
        assert_eq!(
            published_header.notice.as_ref().unwrap().reason,
            MigrationReason::PristineV2Rebound
        );
        let directory = Directory::read(path.parent().unwrap()).unwrap();
        let archive_file = verify_migration_archive(&migration.collection, &published_header, &directory)
            .unwrap()
            .unwrap();
        assert_eq!(archive_file.bytes, initial_frame);
        assert_eq!(
            read_bytes_bound(&landed.bytes, &published_header, migration.live)
                .unwrap()
                .0
                .events
                .len(),
            0
        );
        create(&path, &published_header, true).unwrap(); // Compaction writes this same header and notice.
        let landed = RecoveryBinding::read(&path).unwrap();
        assert_eq!(first_header(&landed.bytes).unwrap(), published_header);
        assert!(
            verify_migration_archive(&migration.collection, &published_header, &directory)
                .unwrap()
                .is_some()
        );
        std::fs::write(migration.archive_path(), b"mutated evidence").unwrap();
        let untouched = std::fs::read(&path).unwrap();
        assert!(verify_migration_archive(&migration.collection, &published_header, &directory).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), untouched);
    }

    #[test]
    fn c12_unused_v2_rejects_changed_files_links_or_collection_binding() {
        for case in [
            "source_replace",
            "source_symlink",
            "source_hardlink",
            "archive_collision",
            "archive_symlink",
            "archive_hardlink",
            "main_replace",
            "main_path",
            "source_bytes",
        ] {
            let (dir, path, migration, state) = fixture();
            let initial_frame = migration.original.bytes.clone();
            match case {
                "source_replace" | "source_symlink" => {
                    let saved = dir.path().join("saved-source");
                    std::fs::rename(&path, &saved).unwrap();
                    if case == "source_replace" {
                        std::fs::write(&path, &initial_frame).unwrap();
                    } else {
                        std::os::unix::fs::symlink(saved, &path).unwrap();
                    }
                }
                "source_hardlink" => std::fs::hard_link(&path, dir.path().join("source-alias")).unwrap(),
                "source_bytes" => std::fs::write(&path, b"changed").unwrap(),
                "archive_collision" => std::fs::write(migration.archive_path(), b"unrelated").unwrap(),
                "archive_symlink" => std::os::unix::fs::symlink(&path, migration.archive_path()).unwrap(),
                "archive_hardlink" => std::fs::hard_link(&path, migration.archive_path()).unwrap(),
                "main_replace" => {
                    std::fs::rename(&migration.collection, dir.path().join("saved-main")).unwrap();
                    std::fs::write(&migration.collection, b"replacement").unwrap();
                }
                "main_path" => {
                    std::fs::rename(&migration.collection, dir.path().join("moved-main")).unwrap();
                }
                _ => unreachable!(),
            }
            let untouched = std::fs::read(&path).unwrap();
            let archive_before = std::fs::read(migration.archive_path()).ok();
            assert!(migration.publish(&path, &state).is_err(), "{case}");
            assert_eq!(std::fs::read(&path).unwrap(), untouched, "{case}");
            if let Some(bytes) = archive_before {
                assert_eq!(std::fs::read(migration.archive_path()).unwrap(), bytes);
            }
        }
    }

    #[test]
    fn c12_unused_v2_checks_each_publication_boundary_and_keeps_evidence() {
        for reject_visit in 1..=4 {
            let (_dir, path, migration, state) = fixture();
            let initial_frame = migration.original.bytes.clone();
            let visits = std::cell::Cell::new(0);
            let result = migration.publish_checked(
                &path,
                &state,
                || {},
                || {
                    visits.set(visits.get() + 1);
                    if visits.get() == reject_visit {
                        Err(identity_mismatch())
                    } else {
                        Ok(())
                    }
                },
            );
            assert!(result.is_err());
            let observed = std::fs::read(&path).unwrap();
            if observed != initial_frame {
                assert_eq!(std::fs::read(migration.archive_path()).unwrap(), initial_frame);
                assert!(first_header(&observed).unwrap().notice.is_some());
            } else {
                assert_eq!(observed, initial_frame);
            }
        }
    }

    #[cfg(feature = "test-hooks")]
    #[test]
    fn c12_unused_v2_publication_faults_keep_restart_evidence() {
        use meguri_model::faultpoint::{self, Fault};
        for active in [false, true] {
            for point in [
                "storage.atomic.after_write",
                "storage.atomic.after_sync",
                "storage.atomic.after_verify",
                "storage.atomic.before_rename",
                "storage.atomic.after_rename",
            ] {
                let (_dir, path, migration, state) = fixture();
                let initial_frame = migration.original.bytes.clone();
                if !active {
                    faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
                }
                let result = migration.publish_with(&path, &state, || {
                    if active {
                        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
                    }
                });
                faultpoint::disarm(point);
                assert!(result.is_err(), "{active} {point}");
                let observed = std::fs::read(&path).unwrap();
                if observed == initial_frame {
                    migration.publish(&path, &state).unwrap();
                } else {
                    let published_header = first_header(&observed).unwrap();
                    assert_eq!(
                        published_header.notice.as_ref().unwrap().reason,
                        MigrationReason::PristineV2Rebound
                    );
                    let directory = Directory::read(path.parent().unwrap()).unwrap();
                    assert!(
                        verify_migration_archive(&migration.collection, &published_header, &directory)
                            .unwrap()
                            .is_some()
                    );
                    RecoveryBinding::read(&path).unwrap().sync(&path).unwrap();
                }
                assert_eq!(std::fs::read(migration.archive_path()).unwrap(), initial_frame);
            }
        }
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::{Engine, EngineConfig, OpenConfig, Ports, sched::queue::QueueProvider, session::ReviewService};
    use meguri_model::{
        clock::{StepClock, UtcOffset},
        rng::SeededRandom,
    };

    fn open_review(path: &Path) -> (Engine, Arc<ReviewService>, Result<Collection, EngineError>) {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let queues = Arc::new(QueueProvider::default());
        let review = ReviewService::new(&engine, queues.clone()).unwrap();
        let ports = Ports::new()
            .open_step(review.clone())
            .observer(review.clone())
            .generation_listener(review.clone())
            .suspend_participant(review.clone())
            .observer(queues.clone())
            .generation_listener(queues);
        let config = OpenConfig::new(
            Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC")),
            Arc::new(SeededRandom::new(43)),
        )
        .ports(ports);
        let result = engine.open_raw(path, config);
        (engine, review, result)
    }
    fn fresh() -> (tempfile::TempDir, PathBuf, PathBuf, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let clock = StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC");
        crate::bootstrap::create(&path, &clock, &SeededRandom::new(43)).unwrap();
        let (engine, review, result) = open_review(&path);
        let collection = result.unwrap();
        engine.close().unwrap();
        drop(collection);
        drop(review);
        drop(engine);
        let journal = path.with_file_name("meguri-pending-answers.jsonl");
        let bytes = std::fs::read(&journal).unwrap();
        (dir, path, journal, bytes)
    }
    fn finish(engine: Engine, review: Arc<ReviewService>, result: Result<Collection, EngineError>) -> bool {
        let ready = matches!(engine.state(), LifecycleState::Ready { .. }) && result.is_ok();
        let closed = engine.close();
        drop(result);
        drop(review);
        drop(engine);
        closed.unwrap();
        ready
    }
    #[test]
    fn c12_managed_matching_pristine_uses_ordinary_open_without_archive_or_incarnation_churn() {
        let (dir, path, journal, original) = fresh();
        let (engine, review, result) = open_review(&path);
        let ready = finish(engine, review, result);
        assert!(ready);
        assert_eq!(std::fs::read(journal).unwrap(), original);
        assert!(
            std::fs::read_dir(dir.path()).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".identity.jsonl"))
        );
    }
    #[test]
    fn c12_managed_ineligible_mismatch_keeps_journal_and_refuses_ready() {
        for case in [
            "zero",
            "highwater",
            "notice",
            "legacy",
            "foreign",
            "unknown",
            "torn",
            "newline",
            "extra_frame",
            "retired",
            "intent",
            "prepared",
            "terminal",
            "checksum",
            "empty",
        ] {
            let (_dir, path, journal, original) = fresh();
            let mut candidate_header = first_header(&original).unwrap();
            candidate_header.path.push(b'x');
            if matches!(case, "highwater" | "torn" | "intent") {
                candidate_header.native = match candidate_header.native {
                    Native::Persistent(PersistentIdentity::Darwin { volume_uuid, inode }) => {
                        Native::Persistent(PersistentIdentity::Darwin {
                            volume_uuid,
                            inode: inode ^ 1,
                        })
                    }
                    Native::Persistent(PersistentIdentity::NonDarwin { device, inode }) => {
                        Native::Persistent(PersistentIdentity::NonDarwin {
                            device,
                            inode: inode ^ 1,
                        })
                    }
                    _ => unreachable!(),
                };
            }
            match case {
                "zero" => candidate_header.next_event = 0,
                "highwater" => candidate_header.next_event = 11,
                "notice" => {
                    candidate_header.notice = Some(MigrationNotice {
                        reason: MigrationReason::PristineV2Rebound,
                        archive_sha256: [5; 32],
                    })
                }
                "legacy" => {
                    candidate_header.version = 1;
                    candidate_header.native = Native::Legacy((7, 9));
                }
                "foreign" => {
                    candidate_header.native = Native::Persistent(if cfg!(target_vendor = "apple") {
                        PersistentIdentity::NonDarwin { device: 7, inode: 9 }
                    } else {
                        PersistentIdentity::Darwin {
                            volume_uuid: [3; 16],
                            inode: 9,
                        }
                    })
                }
                _ => {}
            }
            let mut candidate_frames = encode(1, &Record::Header(candidate_header.clone())).unwrap();
            match case {
                "unknown" => {
                    let mut value = serde_json::to_value(Record::Header(candidate_header)).unwrap();
                    value["Header"]["extra"] = true.into();
                    let json_payload = serde_json::to_string(&value).unwrap();
                    candidate_frames = serde_json::to_vec(&Frame {
                        number: 1,
                        checksum: checksum(1, json_payload.as_bytes()),
                        payload: json_payload,
                    })
                    .unwrap();
                    candidate_frames.push(b'\n');
                }
                "torn" => candidate_frames.extend(b"torn"),
                "newline" => {
                    candidate_frames.pop();
                }
                "extra_frame" => candidate_frames.extend(encode(2, &Record::Header(candidate_header)).unwrap()),
                "retired" => candidate_frames.extend(encode(2, &Record::Retired).unwrap()),
                "intent" => candidate_frames.extend(
                    encode(
                        2,
                        &Record::Intent {
                            event: 1,
                            answer: super::super::super::frozen::test_answer(),
                        },
                    )
                    .unwrap(),
                ),
                "prepared" => candidate_frames.extend(
                    encode(
                        2,
                        &Record::Prepared {
                            event: 1,
                            receipt: Receipt {
                                row: [0; 9],
                                base: [0; 32],
                                post: [0; 32],
                            },
                        },
                    )
                    .unwrap(),
                ),
                "terminal" => candidate_frames.extend(
                    encode(
                        2,
                        &Record::Terminal {
                            event: 1,
                            status: Terminal::Cancelled,
                        },
                    )
                    .unwrap(),
                ),
                "checksum" => {
                    let mut frame: Frame =
                        serde_json::from_slice(&candidate_frames[..candidate_frames.len() - 1]).unwrap();
                    frame.checksum[0] ^= 1;
                    candidate_frames = serde_json::to_vec(&frame).unwrap();
                    candidate_frames.push(b'\n');
                }
                "empty" => candidate_frames.clear(),
                _ => {}
            }
            std::fs::write(&journal, &candidate_frames).unwrap();
            let (engine, review, result) = open_review(&path);
            let ready = finish(engine, review, result);
            assert!(!ready, "{case}");
            assert_eq!(std::fs::read(&journal).unwrap(), candidate_frames, "{case}");
        }
    }
    #[test]
    fn c12_managed_rebind_accepts_native_change_once_then_rejects_replacement() {
        let (_dir, path, journal, original) = fresh();
        let mut old = first_header(&original).unwrap();
        old.native = match old.native {
            Native::Persistent(PersistentIdentity::Darwin { volume_uuid, inode }) => {
                Native::Persistent(PersistentIdentity::Darwin {
                    volume_uuid,
                    inode: inode ^ 1,
                })
            }
            Native::Persistent(PersistentIdentity::NonDarwin { device, inode }) => {
                Native::Persistent(PersistentIdentity::NonDarwin {
                    device,
                    inode: inode ^ 1,
                })
            }
            _ => unreachable!(),
        };
        let bytes = encode(1, &Record::Header(old.clone())).unwrap();
        std::fs::write(&journal, &bytes).unwrap();
        let archive = archive_path(&path, &Sha256::digest(&bytes).into());
        let (engine, review, result) = open_review(&path);
        let ready = finish(engine, review, result);
        assert!(ready);
        let landed = std::fs::read(&journal).unwrap();
        let h = first_header(&landed).unwrap();
        assert_ne!(h.incarnation, old.incarnation);
        assert_eq!(h.next_event, 1);
        assert_eq!(std::fs::read(&archive).unwrap(), bytes);
        let (engine, review, result) = open_review(&path);
        assert!(finish(engine, review, result));
        assert_eq!(std::fs::read(&journal).unwrap(), landed);
        let moved = path.with_file_name("moved.anki2");
        std::fs::copy(&path, &moved).unwrap();
        assert_ne!(
            std::fs::metadata(&path).unwrap().ino(),
            std::fs::metadata(&moved).unwrap().ino()
        );
        let (engine, review, result) = open_review(&moved);
        assert!(!finish(engine, review, result));
        assert_eq!(std::fs::read(&journal).unwrap(), landed);
        assert_eq!(std::fs::read(&archive).unwrap(), bytes);
    }
}

#[cfg(test)]
mod retained_directory_tests {
    use super::*;

    #[test]
    fn c12_managed_directory_replacement_invalidates_retained_descriptor() {
        let sandbox = tempfile::tempdir().unwrap();
        let named_directory = sandbox.path().join("journal-home");
        std::fs::create_dir(&named_directory).unwrap();
        let retained = Directory::read(&named_directory).unwrap();
        retained.require().unwrap();
        let old = sandbox.path().join("retained-origin");
        std::fs::rename(&named_directory, &old).unwrap();
        std::fs::create_dir(&named_directory).unwrap();
        assert!(retained.require().is_err());
        assert!(old.is_dir());
        let held = retained.file.metadata().unwrap();
        let moved = std::fs::metadata(&old).unwrap();
        let replacement = std::fs::metadata(&named_directory).unwrap();
        assert_eq!((held.dev(), held.ino()), (moved.dev(), moved.ino()));
        assert_ne!((held.dev(), held.ino()), (replacement.dev(), replacement.ino()));
    }
}

#[cfg(all(test, feature = "test-hooks"))]
mod opening_fault_tests {
    use super::*;
    use crate::{Engine, EngineConfig, OpenConfig, Ports, sched::queue::QueueProvider, session::ReviewService};
    use meguri_model::{
        clock::{StepClock, UtcOffset},
        faultpoint::{self, Fault},
        rng::SeededRandom,
    };

    fn attempt(path: &Path) -> bool {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let queues = Arc::new(QueueProvider::default());
        let review = ReviewService::new(&engine, queues.clone()).unwrap();
        let ports = Ports::new()
            .open_step(review.clone())
            .observer(review.clone())
            .generation_listener(review.clone())
            .suspend_participant(review.clone())
            .observer(queues.clone())
            .generation_listener(queues);
        let result = engine.open_raw(
            path,
            OpenConfig::new(
                Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC")),
                Arc::new(SeededRandom::new(43)),
            )
            .ports(ports),
        );
        let ready = matches!(engine.state(), LifecycleState::Ready { .. }) && result.is_ok();
        let closed = engine.close();
        drop(result);
        drop(review);
        drop(engine);
        closed.unwrap();
        ready
    }
    #[test]
    fn c12_managed_archive_fault_closes_workers_refuses_ready_and_requires_explicit_restart() {
        for point in [
            "storage.atomic.after_write",
            "storage.atomic.after_sync",
            "storage.atomic.after_rename",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("collection.anki2");
            crate::bootstrap::create(
                &path,
                &StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"),
                &SeededRandom::new(43),
            )
            .unwrap();
            assert!(attempt(&path));
            let journal = path.with_file_name("meguri-pending-answers.jsonl");
            let mut old = first_header(&std::fs::read(&journal).unwrap()).unwrap();
            old.path.push(b'x');
            let original = encode(1, &Record::Header(old)).unwrap();
            std::fs::write(&journal, &original).unwrap();
            let archive = archive_path(&path, &Sha256::digest(&original).into());
            faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
            let ready = attempt(&path);
            faultpoint::disarm(point);
            assert!(!ready, "{point}");
            assert_eq!(std::fs::read(&journal).unwrap(), original);
            if archive.exists() {
                assert_eq!(std::fs::read(&archive).unwrap(), original);
            }
            assert!(attempt(&path));
            assert_eq!(std::fs::read(&archive).unwrap(), original);
            assert_eq!(first_header(&std::fs::read(&journal).unwrap()).unwrap().next_event, 1);
        }
    }
}
