//! R6 publication identities and replacement journal. No network or decision resolver.
use meguri_model::rng::RandomSource;
use meguri_storage::{StorageError, sidecar};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum AccountBinding {
    LocalOnly,
    Connected {
        account: String,
        configuration: String,
        environment: String,
    },
    /// A caller with uncertain account state cannot grant publication or recovery.
    Unknown,
}
impl AccountBinding {
    fn certain(&self) -> bool {
        match self {
            Self::LocalOnly => true,
            Self::Connected {
                account,
                configuration,
                environment,
            } => !account.is_empty() && !configuration.is_empty() && !environment.is_empty(),
            Self::Unknown => false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicationId([u64; 2]);
impl PublicationId {
    fn fresh(random: &dyn RandomSource) -> Self {
        Self([random.next_u64(), random.next_u64()])
    }
    fn successor(random: &dyn RandomSource, prior: Self) -> Self {
        // Include the persistent predecessor so restarting a deterministic
        // test random source does not repeat a stored publication identifier.
        let mut hash = Sha256::new();
        hash.update(b"meguri-publication-successor-v1");
        for bits in [prior.0[0], prior.0[1], random.next_u64(), random.next_u64()] {
            hash.update(bits.to_le_bytes());
        }
        let bytes: [u8; 32] = hash.finalize().into();
        Self([
            u64::from_le_bytes(bytes[..8].try_into().unwrap()),
            u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
        ])
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicationDecision {
    Clear,
    Pending,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Publication {
    pub identity: PublicationId,
    pub binding: AccountBinding,
    pub decision: PublicationDecision,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicationToken {
    identity: PublicationId,
    binding: AccountBinding,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIdentity {
    pub length: u64,
    pub sha256: [u8; 32],
}
pub(crate) fn fingerprint(path: &Path) -> Result<Option<FileIdentity>, StorageError> {
    fingerprint_native(path).map(|identity| identity.map(|(bytes, _)| bytes))
}
fn fingerprint_native(path: &Path) -> Result<Option<(FileIdentity, NativeIdentity)>, StorageError> {
    fingerprint_native_checked(path, &|| false, false)
}
fn fingerprint_native_checked(
    path: &Path,
    cancel: &dyn Fn() -> bool,
    final_pass: bool,
) -> Result<Option<(FileIdentity, NativeIdentity)>, StorageError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let before = file.metadata()?;
    if !before.is_file() || before.nlink() != 1 {
        return Err(failure("database identity is not an independent regular file"));
    }
    let mut digest = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        if final_pass {
            meguri_model::faultpoint!("replace.media.final_database_hash")?;
        }
        if cancel() {
            return Err(failure("database identity scan cancelled"));
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        length = length
            .checked_add(read as u64)
            .ok_or_else(|| failure("database identity length overflow"))?;
        digest.update(&buffer[..read]);
    }
    let after = file.metadata()?;
    let bound = std::fs::symlink_metadata(path)?;
    if before.dev() != bound.dev()
        || before.ino() != bound.ino()
        || before.len() != length
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(failure("database identity changed while reading"));
    }
    Ok(Some((
        FileIdentity {
            length,
            sha256: digest.finalize().into(),
        },
        NativeIdentity {
            device: before.dev(),
            inode: before.ino(),
        },
    )))
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Phase {
    Intent,
    Activated,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Intent {
    operation: PublicationId,
    reason: crate::replace::ReplaceReason,
    prior: Publication,
    candidate: Publication,
    old_file: Option<FileIdentity>,
    candidate_file: FileIdentity,
    phase: Phase,
    #[serde(default)]
    old_native: Option<NativeIdentity>,
    #[serde(default)]
    candidate_native: Option<NativeIdentity>,
    #[serde(default)]
    media: Option<meguri_storage::media::CollectionMediaIntent>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct NativeIdentity {
    device: u64,
    inode: u64,
}
fn native(path: &Path) -> Result<Option<NativeIdentity>, StorageError> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let m = file.metadata()?;
    if !m.is_file() || m.nlink() != 1 {
        return Err(failure("nonregular or linked database identity"));
    }
    Ok(Some(NativeIdentity {
        device: m.dev(),
        inode: m.ino(),
    }))
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct State {
    active: Publication,
    intent: Option<Intent>,
    /// R6 activation chain survives policy write failure and further replacements.
    #[serde(default)]
    replacements: Vec<ReplacementEvidence>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}
fn failure(message: impl ToString) -> StorageError {
    StorageError::Io {
        detail: format!("publication recovery blocked: {}", message.to_string()),
    }
}
fn decode(doc: Option<&Map<String, Value>>) -> Result<Option<State>, StorageError> {
    doc.and_then(|doc| doc.get("deviceState"))
        .map(|value| serde_json::from_value(value.clone()).map_err(|_| failure("unreadable device state")))
        .transpose()
}
fn put(doc: &mut Map<String, Value>, state: &State) -> Result<(), StorageError> {
    doc.insert("deviceState".into(), serde_json::to_value(state).map_err(failure)?);
    Ok(())
}
fn bound(state: &State, binding: &AccountBinding) -> Result<(), StorageError> {
    if !binding.certain() || &state.active.binding != binding {
        return Err(failure("publication binding is unavailable or does not match"));
    }
    if let Some(intent) = &state.intent {
        if &intent.prior.binding != binding || &intent.candidate.binding != binding {
            return Err(failure("intent account binding differs"));
        }
        let expected = if intent.reason.requires_publication_decision() {
            PublicationDecision::Pending
        } else {
            intent.prior.decision
        };
        if intent.candidate.decision != expected
            || intent.prior.identity == intent.candidate.identity
            || (intent.phase == Phase::Intent && state.active != intent.prior)
            || (intent.phase == Phase::Activated && state.active != intent.candidate)
        {
            return Err(failure("inconsistent publication journal"));
        }
    }
    Ok(())
}
fn sidecar_path(live: &Path) -> PathBuf {
    live.parent().unwrap_or(Path::new(".")).join("meguri-sidecar.json")
}

/// Durable local-activation evidence, retained after R6 intent cleanup. This
/// proves a local replacement only, never remote upload acceptance/equality.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacementEvidence {
    pub reason: crate::replace::ReplaceReason,
    pub prior: Publication,
    pub candidate: Publication,
    pub candidate_file: FileIdentity,
    pub old_file: Option<FileIdentity>,
}

/// Constructible only by checking the durable R6 chain against its active record.
/// Consumers must hold the collection's lifecycle slot until their CAS completes.
#[derive(Clone, Debug)]
pub struct LocalReplacementSuccessor {
    before: PublicationId,
    after: PublicationId,
}
impl LocalReplacementSuccessor {
    pub fn before(&self) -> PublicationId {
        self.before
    }
    pub fn after(&self) -> PublicationId {
        self.after
    }
}

/// Read access to the device record. Collection ownership must cover these
/// calls, including queued-work token validation immediately before sending.
#[derive(Clone, Debug)]
pub struct DeviceState {
    path: PathBuf,
    live: PathBuf,
}
impl DeviceState {
    pub fn for_collection(live: &Path) -> Self {
        Self {
            path: sidecar_path(live),
            live: live.to_owned(),
        }
    }
    pub fn replacement_evidence(&self) -> Result<Option<ReplacementEvidence>, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            decode(doc)?
                .and_then(|s| s.extra.get("lastReplacement").cloned())
                .map(|v| serde_json::from_value(v).map_err(|_| failure("unreadable activation evidence")))
                .transpose()
        })
    }
    /// Offline proof, never inferred from process generation or collection absence.
    /// Legacy lastReplacement is the first retained link on the next activation.
    pub fn local_replacement_successor(
        &self,
        before: PublicationId,
    ) -> Result<Option<LocalReplacementSuccessor>, StorageError> {
        if !std::fs::symlink_metadata(&self.live)?.file_type().is_file() {
            return Ok(None);
        }
        sidecar::read_document(&self.path, |doc| {
            let Some(state) = decode(doc)? else { return Ok(None) };
            bound(&state, &state.active.binding)?;
            if state.intent.is_some() || state.active.identity == before {
                return Ok(None);
            }
            let chain = retained_replacements(&state)?;
            let Some(start) = chain.iter().position(|e| e.prior.identity == before) else {
                return Ok(None);
            };
            if !matches!(
                chain[start].reason,
                crate::replace::ReplaceReason::ColpkgImport | crate::replace::ReplaceReason::Restore
            ) {
                return Ok(None);
            }
            let mut successor = &chain[start].prior;
            for evidence in &chain[start..] {
                if evidence.prior.identity != successor.identity
                    || evidence.prior.binding != successor.binding
                    || evidence.prior.identity == evidence.candidate.identity
                    || evidence.prior.binding != evidence.candidate.binding
                {
                    return Ok(None);
                }
                successor = &evidence.candidate;
            }
            // Decisions may be resolved independently; persistent identity and
            // binding still have to name the active verified successor.
            if successor.identity != state.active.identity || successor.binding != state.active.binding {
                return Ok(None);
            }
            Ok(Some(LocalReplacementSuccessor {
                before,
                after: successor.identity,
            }))
        })
    }
    pub fn publication(&self) -> Result<Option<Publication>, StorageError> {
        sidecar::read_document(&self.path, |doc| Ok(decode(doc)?.map(|state| state.active)))
    }
    /// Local-only never grants a sending token. Pending and unfinished intents
    /// block both identities. There is deliberately no public clear setter.
    pub fn publication_token(&self, binding: &AccountBinding) -> Result<PublicationToken, StorageError> {
        sidecar::read_document(&self.path, |doc| {
            let state = decode(doc)?.ok_or_else(|| failure("missing identity"))?;
            bound(&state, binding)?;
            if !std::fs::symlink_metadata(&self.live)?.file_type().is_file() {
                return Err(failure("no regular live collection"));
            }
            if state.intent.is_some()
                || state.active.decision != PublicationDecision::Clear
                || !matches!(binding, AccountBinding::Connected { .. })
            {
                return Err(failure("explicit publication decision required"));
            }
            Ok(PublicationToken {
                identity: state.active.identity,
                binding: binding.clone(),
            })
        })
    }
    pub fn validate_token(&self, token: &PublicationToken, binding: &AccountBinding) -> Result<(), StorageError> {
        if &token.binding != binding || self.publication_token(binding)? != *token {
            return Err(failure("stale publication token"));
        }
        Ok(())
    }
    /// O8 bookkeeping cannot clear or rewrite the cloud publication decision.
    pub fn record_manual_transfer_success(
        &self,
        optimized_at: Option<meguri_model::time::EpochMillis>,
    ) -> Result<(), StorageError> {
        sidecar::update_document(&self.path, |doc| {
            let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing identity"))?;
            state.extra.insert("oneWayChoicePending".into(), false.into());
            state.extra.remove("lastFailedManualSanityCounts");
            if let Some(at) = optimized_at {
                state.extra.insert("lastOptimize".into(), at.0.into());
            }
            put(doc, &state)
        })
    }
    /// Legacy manual state is independent; clearing it cannot edit R6's decision.
    pub fn set_manual_transfer_pending(&self, pending: bool) -> Result<(), StorageError> {
        sidecar::update_document(&self.path, |doc| {
            let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing identity"))?;
            state.extra.insert("oneWayChoicePending".into(), pending.into());
            put(doc, &state)
        })
    }
}

pub(crate) fn ensure(live: &Path, binding: &AccountBinding, random: &dyn RandomSource) -> Result<(), StorageError> {
    let path = sidecar_path(live);
    if sidecar::read_document(&path, |doc| {
        if let Some(state) = decode(doc)? {
            bound(&state, binding)?;
            Ok(true)
        } else {
            Ok(false)
        }
    })? {
        return Ok(());
    }
    if !binding.certain() {
        return Err(failure("uncertain binding"));
    }
    sidecar::update_document(&path, |doc| {
        if let Some(state) = decode(Some(doc))? {
            return bound(&state, binding);
        }
        put(
            doc,
            &State {
                active: Publication {
                    identity: PublicationId::fresh(random),
                    binding: binding.clone(),
                    decision: PublicationDecision::Clear,
                },
                intent: None,
                replacements: Vec::new(),
                extra: BTreeMap::new(),
            },
        )
    })
}

/// Called under the collection lock, before sweep/open/day writes. Activated
/// records deliberately do not compare hashes: local review may have committed.
pub(crate) fn reconcile(
    live: &Path,
    binding: &AccountBinding,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
) -> Result<(), StorageError> {
    reconcile_with_media(live, binding, clock, None)
}
pub(crate) fn reconcile_with_media(
    live: &Path,
    binding: &AccountBinding,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    shared: Option<&meguri_storage::media::MediaStore>,
) -> Result<(), StorageError> {
    let path = sidecar_path(live);
    let state = sidecar::read_document(&path, decode)?;
    let Some(state) = state else {
        return if binding.certain() {
            Ok(())
        } else {
            Err(failure("uncertain binding"))
        };
    };
    bound(&state, binding)?;
    if state.intent.is_none() {
        return cleanup_completed_media(live, binding);
    }
    let intent = state.intent.unwrap();
    let decision = match intent.phase {
        Phase::Activated => {
            let expected = if intent.reason.requires_publication_decision() {
                PublicationDecision::Pending
            } else {
                intent.prior.decision
            };
            if state.active != intent.candidate || state.active.decision != expected {
                return Err(failure("invalid activated record"));
            }
            intent.candidate.clone()
        }
        Phase::Intent => {
            // No live writer is permitted in this phase. A surviving WAL or
            // journal makes main-file-only proof insufficient.
            for suffix in ["-wal", "-journal"] {
                let mut name = live.as_os_str().to_owned();
                name.push(suffix);
                match std::fs::metadata(PathBuf::from(name)) {
                    Ok(m) if m.len() != 0 => return Err(failure("unfinished intent has live journal bytes")),
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
            let file = fingerprint(live)?;
            let current_native = native(live)?;
            let old = file == intent.old_file && (intent.old_native.is_none() || current_native == intent.old_native);
            let candidate = file.as_ref() == Some(&intent.candidate_file)
                && (intent.candidate_native.is_none() || current_native == intent.candidate_native);
            match (old, candidate) {
                (true, false) => intent.prior.clone(),
                (false, true) => {
                    // A crash after rename may have preceded directory sync.
                    meguri_storage::atomic_file::sync_directory(live.parent().unwrap_or(Path::new(".")))?;
                    finish_media(live, &intent, true, clock.clone(), shared)?;
                    activate(live, binding)?;
                    return cleanup_completed_media(live, binding);
                }
                _ => {
                    return Err(failure(
                        "both or neither closed file identities match; journal retained",
                    ));
                }
            }
        }
    };
    finish_media(
        live,
        &intent,
        decision.identity == intent.candidate.identity,
        clock,
        shared,
    )?;
    // Keep decision and intent together in the first durable write. Clearing
    // the journal is a second write, never the source of the decision.
    sidecar::update_document(&path, |doc| {
        let mut current = decode(Some(doc))?.ok_or_else(|| failure("missing journal"))?;
        bound(&current, binding)?;
        current.active = decision;
        put(doc, &current)
    })?;
    complete(live, binding)?;
    cleanup_completed_media(live, binding)
}

pub(crate) fn begin(
    paths: (&Path, &Path),
    reason: crate::replace::ReplaceReason,
    binding: &AccountBinding,
    random: &dyn RandomSource,
    media: Option<meguri_storage::media::CollectionMediaIntent>,
    cancel: &dyn Fn() -> bool,
    before_record: impl FnOnce(),
) -> Result<(), StorageError> {
    let (live, candidate) = paths;
    ensure(live, binding, random)?;
    let old_identity = fingerprint_native_checked(live, cancel, false)?;
    let (candidate_file, candidate_native) =
        fingerprint_native_checked(candidate, cancel, false)?.ok_or_else(|| failure("candidate absent"))?;
    let (old_file, old_native) = match old_identity {
        Some((b, n)) => (Some(b), Some(n)),
        None => (None, None),
    };
    let candidate_native = Some(candidate_native);
    before_record();
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing identity"))?;
        bound(&state, binding)?;
        if state.intent.is_some() {
            return Err(failure("unfinished replacement"));
        }
        let candidate = Publication {
            identity: PublicationId::successor(random, state.active.identity),
            binding: binding.clone(),
            decision: if reason.requires_publication_decision() {
                PublicationDecision::Pending
            } else {
                state.active.decision
            },
        };
        if candidate.identity == state.active.identity {
            return Err(failure("random source reused publication identity"));
        }
        state.intent = Some(Intent {
            operation: PublicationId::successor(random, candidate.identity),
            reason,
            prior: state.active.clone(),
            candidate,
            old_file,
            candidate_file,
            phase: Phase::Intent,
            old_native,
            candidate_native,
            media,
        });
        put(doc, &state)
    })
}
fn record_media(live: &Path, media: &meguri_storage::media::CollectionMediaIntent) -> Result<(), StorageError> {
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing media intent"))?;
        let intent = state.intent.as_mut().ok_or_else(|| failure("missing media intent"))?;
        if intent.media.as_ref().map(|m| m.recovery_reference()) != Some(media.recovery_reference()) {
            return Err(failure("media operation changed"));
        }
        intent.media = Some(media.clone());
        put(doc, &state)
    })
}
pub(crate) fn verify_swap_files(live: &Path, candidate: &Path, cancel: &dyn Fn() -> bool) -> Result<(), StorageError> {
    let state =
        sidecar::read_document(&sidecar_path(live), decode)?.ok_or_else(|| failure("missing final swap evidence"))?;
    let intent = state.intent.ok_or_else(|| failure("missing final swap intent"))?;
    let old = fingerprint_native_checked(live, cancel, true)?;
    let incoming =
        fingerprint_native_checked(candidate, cancel, true)?.ok_or_else(|| failure("final candidate missing"))?;
    if old.as_ref().map(|(b, _)| b) != intent.old_file.as_ref()
        || old.as_ref().map(|(_, n)| n) != intent.old_native.as_ref()
        || incoming.0 != intent.candidate_file
        || Some(&incoming.1) != intent.candidate_native.as_ref()
    {
        return Err(failure("closed swap file identities changed"));
    }
    Ok(())
}
pub(crate) fn owns_media_intent(live: &Path, reference: &str) -> Result<bool, StorageError> {
    sidecar::read_document(&sidecar_path(live), |doc| {
        Ok(decode(doc)?
            .and_then(|state| state.intent)
            .and_then(|intent| intent.media)
            .is_some_and(|media| media.recovery_reference() == reference))
    })
}
pub(crate) fn media_activation(live: &Path, reference: &str) -> Result<Option<bool>, StorageError> {
    sidecar::read_document(&sidecar_path(live), |doc| {
        let Some(state) = decode(doc)? else { return Ok(None) };
        let Some(list) = state.extra.get("collectionMediaReplacements").and_then(Value::as_array) else {
            return Ok(None);
        };
        list.iter()
            .rev()
            .find(|v| v.get("root").and_then(Value::as_str) == Some(reference))
            .map(|v| {
                v.get("candidateActive")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| failure("invalid completed media evidence"))
            })
            .transpose()
    })
}
pub(crate) fn publish_media(
    live: &Path,
    media: &mut meguri_storage::media::CollectionMediaIntent,
    store: &meguri_storage::media::MediaStore,
    cancel: &dyn Fn() -> bool,
) -> Result<(), meguri_storage::media::MediaError> {
    media.publish(
        store,
        |m| {
            record_media(live, m).map_err(|_| {
                meguri_storage::media::MediaError::InvalidManifest("media progress could not be recorded".into())
            })
        },
        cancel,
    )
}
fn finish_media(
    live: &Path,
    intent: &Intent,
    candidate: bool,
    clock: std::sync::Arc<dyn meguri_model::clock::Clock>,
    shared: Option<&meguri_storage::media::MediaStore>,
) -> Result<(), StorageError> {
    let Some(mut media) = intent.media.clone() else {
        return Ok(());
    };
    media
        .validate_initial_admission(live)
        .map_err(|e| failure(e.to_string()))?;
    let opened;
    let store = match shared {
        Some(store) => store,
        None => {
            opened = media
                .open_recovery_store(live, clock)
                .map_err(|e| failure(e.to_string()))?;
            &opened
        }
    };
    if candidate {
        publish_media(live, &mut media, store, &|| false).map_err(|e| failure(e.to_string()))?;
    } else {
        media.rollback(store).map_err(|e| failure(e.to_string()))?;
    }
    // Evidence survives delivery failure and future replacements. In particular
    // a backup's old-name/trash mapping is independent of backup retention.
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing media evidence"))?;
        let key = "collectionMediaReplacements";
        let mut value = serde_json::to_value(&media).map_err(failure)?;
        let record = value.as_object_mut().ok_or_else(|| failure("invalid media evidence"))?;
        record.insert(
            "operation".into(),
            serde_json::to_value(intent.operation).map_err(failure)?,
        );
        record.insert("candidateActive".into(), candidate.into());
        record.insert(
            "priorPublication".into(),
            serde_json::to_value(&intent.prior).map_err(failure)?,
        );
        record.insert(
            "candidatePublication".into(),
            serde_json::to_value(&intent.candidate).map_err(failure)?,
        );
        let list = state
            .extra
            .entry(key.into())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| failure("invalid media evidence list"))?;
        if let Some(prior) = list.iter_mut().find(|v| v.get("operation") == value.get("operation")) {
            *prior = value;
        } else {
            list.push(value);
        }
        put(doc, &state)
    })
}
fn cleanup_completed_media(live: &Path, binding: &AccountBinding) -> Result<(), StorageError> {
    let state = sidecar::read_document(&sidecar_path(live), decode)?
        .ok_or_else(|| failure("missing media completion state"))?;
    bound(&state, binding)?;
    if state.intent.is_some() {
        return Err(failure("media cleanup still has an active replacement"));
    }
    let Some(records) = state.extra.get("collectionMediaReplacements") else {
        return Ok(());
    };
    for record in records
        .as_array()
        .ok_or_else(|| failure("invalid media evidence list"))?
    {
        if record.get("completed").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let operation: PublicationId = serde_json::from_value(
            record
                .get("operation")
                .cloned()
                .ok_or_else(|| failure("missing media operation"))?,
        )
        .map_err(failure)?;
        let prior: Publication = serde_json::from_value(
            record
                .get("priorPublication")
                .cloned()
                .ok_or_else(|| failure("missing prior media publication"))?,
        )
        .map_err(failure)?;
        let candidate: Publication = serde_json::from_value(
            record
                .get("candidatePublication")
                .cloned()
                .ok_or_else(|| failure("missing candidate media publication"))?,
        )
        .map_err(failure)?;
        if !prior.binding.certain()
            || prior.binding != candidate.binding
            || prior.identity == candidate.identity
            || operation == prior.identity
            || operation == candidate.identity
            || record.get("candidateActive").and_then(Value::as_bool).is_none()
        {
            return Err(failure("invalid media completion witness"));
        }
        let media: meguri_storage::media::CollectionMediaIntent =
            serde_json::from_value(record.clone()).map_err(failure)?;
        media
            .cleanup_initial_admission(live)
            .map_err(|e| failure(e.to_string()))?;
    }
    Ok(())
}
fn retained_replacements(state: &State) -> Result<Vec<ReplacementEvidence>, StorageError> {
    if !state.replacements.is_empty() {
        return Ok(state.replacements.clone());
    }
    state
        .extra
        .get("lastReplacement")
        .cloned()
        .map(|value| {
            serde_json::from_value(value)
                .map(|e| vec![e])
                .map_err(|_| failure("unreadable activation evidence"))
        })
        .transpose()
        .map(Option::unwrap_or_default)
}

pub(crate) fn activate(live: &Path, binding: &AccountBinding) -> Result<(), StorageError> {
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing activation journal"))?;
        bound(&state, binding)?;
        let mut retained = retained_replacements(&state)?;
        let intent = state.intent.as_mut().ok_or_else(|| failure("missing intent"))?;
        state.active = intent.candidate.clone();
        intent.phase = Phase::Activated;
        let evidence = ReplacementEvidence {
            reason: intent.reason,
            prior: intent.prior.clone(),
            candidate: intent.candidate.clone(),
            candidate_file: intent.candidate_file.clone(),
            old_file: intent.old_file.clone(),
        };
        if retained.last() != Some(&evidence) {
            retained.push(evidence.clone());
        }
        state.replacements = retained;
        state.extra.insert(
            "lastReplacement".into(),
            serde_json::to_value(evidence).map_err(failure)?,
        );
        put(doc, &state)
    })?;
    meguri_model::faultpoint!("replace.after_durable_activation")?;
    complete(live, binding)
}
fn complete(live: &Path, binding: &AccountBinding) -> Result<(), StorageError> {
    meguri_model::faultpoint!("replace.media.before_completion")?;
    sidecar::update_document(&sidecar_path(live), |doc| {
        let mut state = decode(Some(doc))?.ok_or_else(|| failure("missing completed journal"))?;
        bound(&state, binding)?;
        if let Some(intent) = state.intent.take()
            && let Some(media) = intent.media
        {
            let operation = serde_json::to_value(intent.operation).map_err(failure)?;
            let record = state
                .extra
                .get_mut("collectionMediaReplacements")
                .and_then(Value::as_array_mut)
                .and_then(|records| {
                    records
                        .iter_mut()
                        .find(|record| record.get("operation") == Some(&operation))
                })
                .ok_or_else(|| failure("missing media completion evidence"))?;
            let candidate = record
                .get("candidateActive")
                .and_then(Value::as_bool)
                .ok_or_else(|| failure("missing media completion decision"))?;
            let expected = if candidate { &intent.candidate } else { &intent.prior };
            if &state.active != expected
                || record.get("root").and_then(Value::as_str) != Some(media.recovery_reference())
                || record.get("priorPublication") != Some(&serde_json::to_value(&intent.prior).map_err(failure)?)
                || record.get("candidatePublication")
                    != Some(&serde_json::to_value(&intent.candidate).map_err(failure)?)
            {
                return Err(failure("media completion publication differs"));
            }
            record
                .as_object_mut()
                .ok_or_else(|| failure("invalid media completion record"))?
                .insert("completed".into(), true.into());
        }
        put(doc, &state)
    })?;
    meguri_model::faultpoint!("replace.media.after_completion")?;
    Ok(())
}
