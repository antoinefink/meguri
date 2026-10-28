//! Closed synthetic baselines and read-only durable observations.
use super::*;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::{RandomSource, SeededRandom},
};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
use meguri_sync::cloud::{
    self as production, entity_codec as codec,
    row_payload::{self, View},
    runtime::{self, FileIdentityProvider},
    workflow::EntityFacts,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub(crate) fn failure() -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(std::io::Error::other("cloud device operation failed"))
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum IdentityReads {
    Available(String),
    FailAllSession,
}
pub use runtime::DirtySource;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DirtyMark {
    pub binding: production::BindingId,
    pub source: DirtySource,
    pub change_seq: u64,
    pub payload_digest: [u8; 32],
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DeviceFacts {
    pub note: codec::Note,
    pub local_rev: u64,
    pub change_counter: u64,
    pub last_observed_digest: Option<[u8; 32]>,
    pub mark: Option<DirtyMark>,
    pub pending_names: BTreeSet<String>,
    pub binding_state: production::BindingState,
    pub scan_drained: bool,
    pub sidecar_clean: bool,
}
#[derive(Clone)]
pub struct PreboundDevice {
    pub directory: PathBuf,
    pub scope: Scope,
    pub binding: production::Binding,
    pub collection_zone: String,
    pub entity_name: String,
    pub note: codec::Note,
    pub(crate) seed: u64,
    pub(crate) notes: Vec<codec::Note>,
}
impl std::fmt::Debug for PreboundDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreboundDevice")
    }
}
pub(crate) fn build_scope(s: &Scope) -> Result<production::BuildScope> {
    Ok(production::BuildScope {
        container: production::ContainerId::new(s.container.clone())?,
        environment: match s.environment.as_str() {
            "development" => production::Environment::Development,
            "production" => production::Environment::Production,
            _ => return Err(failure()),
        },
    })
}
pub struct NativeFiles;
impl FileIdentityProvider for NativeFiles {
    fn identity(&self, path: &Path) -> std::result::Result<runtime::NativeFileIdentity, runtime::RuntimeError> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::symlink_metadata(path).map_err(|_| runtime::RuntimeError::FileIdentityUnavailable)?;
        if !m.file_type().is_file() {
            return Err(runtime::RuntimeError::FileIdentityUnavailable);
        }
        let created = m
            .created()
            .map_err(|_| runtime::RuntimeError::FileIdentityUnavailable)?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| runtime::RuntimeError::FileIdentityUnavailable)?;
        Ok(runtime::NativeFileIdentity {
            device: m.dev(),
            inode: m.ino(),
            created_seconds: i64::try_from(created.as_secs())
                .map_err(|_| runtime::RuntimeError::FileIdentityUnavailable)?,
            created_nanos: created.subsec_nanos(),
        })
    }
}
pub fn options(dir: &Path, now: i64, seed: u64) -> meguri_host::HostConfig {
    let mut backup_name = dir.file_name().expect("owned collection directory").to_os_string();
    backup_name.push("-backups");
    let backups = dir.with_file_name(backup_name);
    meguri_host::HostConfig::new(
        dir.to_owned(),
        backups,
        dir.join("widget"),
        Arc::new(FixedClock::new(now, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(seed)),
        Arc::new(meguri_storage::space::StatvfsSpace),
    )
}
pub(crate) fn collect<T: ReadRows + ?Sized>(
    rows: &T,
    key: &[u8; 32],
    layouts: &BTreeMap<i64, codec::Notetype>,
) -> Result<BTreeMap<String, codec::Payload>> {
    let v = row_payload::ReadView(rows);
    let mut out = BTreeMap::new();
    let mut total = 0;
    for kind in row_payload::KINDS {
        let Some(t) = row_payload::table(kind) else { continue };
        let mut after = None;
        loop {
            let page = v.page(t, after.as_ref())?;
            if page.is_empty() {
                break;
            }
            for r in page {
                after = Some(RowKey::from_stored(t, &r)?);
                let k = row_payload::key(kind, &r)?;
                let p = row_payload::project(&v, kind, &r, |id| {
                    layouts.get(&id).cloned().ok_or(production::Error::InvalidData)
                })?;
                total += p.encode()?.len();
                if out.len() >= 2048 || total > 8 * 1024 * 1024 {
                    return Err(failure());
                }
                out.insert(codec::entity_name(key, kind, &k)?, p);
            }
        }
    }
    Ok(out)
}
pub(crate) fn layouts<T: ReadRows + ?Sized>(rows: &T) -> Result<BTreeMap<i64, codec::Notetype>> {
    let v = row_payload::ReadView(rows);
    let mut out = BTreeMap::new();
    let mut after = None;
    loop {
        let page = v.page(Table::Notetypes, after.as_ref())?;
        if page.is_empty() {
            break;
        }
        for r in page {
            let id = row_payload::int(&r, 0)?;
            after = Some(RowKey::Id(id));
            out.insert(id, row_payload::notetype(&v, &r, 1)?);
        }
    }
    Ok(out)
}
pub(crate) fn open_store(files: &PreboundDevice) -> Result<production::TrackingStore> {
    match production::TrackingStore::open(&files.directory, files.binding.scope.clone())? {
        production::OpenOutcome::Opened(s) => Ok(s),
        _ => Err(failure()),
    }
}
impl PreboundDevice {
    pub fn arrange(directory: &Path, service: &mut Service<SeededRandom>, seed: u64) -> Result<Self> {
        const NOW: i64 = 1_735_689_600_000;
        let host = meguri_host::CoreHost::open(options(directory, NOW, seed))?;
        let c = host.collection()?;
        let media = host.media()?;
        media.store().register_full(|_, _| true).map_err(|_| failure())?;
        let feed = media.store().media_feed_snapshot().map_err(|_| failure())?;
        if !feed.status.complete || !feed.items.is_empty() {
            return Err(failure());
        }
        let (crt, payloads, baseline_cursor) = {
            let lease = c.reader(ReaderRole::Query)?;
            let snap = lease.snapshot()?;
            let l = layouts(&*snap)?;
            let crt = snap.col()?.created.0;
            let random = SeededRandom::new(seed);
            let mut key = [0; 32];
            for part in key.chunks_mut(8) {
                part.copy_from_slice(&random.next_u64().to_le_bytes())
            }
            let payloads = collect(&*snap, &key, &l)?;
            let file = NativeFiles.identity(c.path())?;
            let receipt =
                match c.wait_wal_covered(snap.wal_identity(), snap.seq(), std::time::Duration::from_secs(30))? {
                    meguri_engine::WalCoverage::Covered(r)
                        if r.identity() == snap.wal_identity() && r.through() >= snap.seq() =>
                    {
                        r
                    }
                    _ => return Err(failure()),
                };
            if receipt.identity().native_file() != (file.device, file.inode) {
                return Err(failure());
            }
            let col = snap.col()?;
            (
                crt,
                (key, payloads),
                runtime::NoteScanCursor {
                    seq: snap.seq().0,
                    generation: snap.generation().0,
                    covered_through: receipt.through().0,
                    native_file: file,
                    modified: col.modified.0,
                    schema_modified: col.schema_modified.0,
                },
            )
        };
        host.close()?;
        drop(host);
        drop(c);
        let (record_key, payloads) = payloads;
        let scope = Scope {
            container: "test.meguri.cloud".into(),
            environment: "development".into(),
            account: "synthetic-account-a".into(),
        };
        let rng = SeededRandom::new(seed ^ 0x411);
        let mut epoch = [0; 16];
        epoch[..8].copy_from_slice(&rng.next_u64().to_le_bytes());
        epoch[8..].copy_from_slice(&rng.next_u64().to_le_bytes());
        let collection_zone = format!("c-{}", epoch.iter().map(|b| format!("{b:02x}")).collect::<String>());
        let publication = meguri_engine::device_state::DeviceState::for_collection(&directory.join("collection.anki2"))
            .publication()?
            .ok_or_else(failure)?
            .identity;
        let mut store = runtime::with_tracking_access(|| -> Result<_> {
            Ok(production::TrackingStore::open(directory, build_scope(&scope)?)?)
        })?;
        let store = match &mut store {
            production::OpenOutcome::Missing(s) | production::OpenOutcome::Opened(s) => s,
            _ => return Err(failure()),
        };
        let binding = store.create_binding(
            production::BindingFacts {
                account: production::OpaqueBytes(scope.account.as_bytes().to_vec()),
                epoch: production::EpochId(epoch),
                record_key: production::RecordKey(record_key),
                publication,
                state: production::BindingState::Active,
            },
            &FixedClock::new(NOW, UtcOffset::UTC),
            &rng,
        )?;
        for zone in ["meguri-root", "meguri-media", &collection_zone] {
            service
                .request(&scope, Caller::default(), Operation::CreateZone { zone: zone.into() })?
                .result
                .map_err(|_| failure())?;
        }
        let counts = row_payload::KINDS
            .iter()
            .map(|k| (*k, payloads.values().filter(|p| p.kind() == *k).count() as u64))
            .collect::<Vec<_>>();
        for (zone, name, ty, e) in [
            (
                "meguri-root",
                "active",
                "MGActive",
                production::control_codec::Active {
                    epoch,
                    activated_at_ms: NOW,
                    previous: None,
                    replaced_through_ms: None,
                }
                .encode()?,
            ),
            (
                collection_zone.as_str(),
                "epoch",
                "MGEpoch",
                production::control_codec::epoch(epoch, record_key, crt, NOW, &counts)?,
            ),
            (
                "meguri-media",
                "root",
                "MGMediaRoot",
                production::control_codec::media_root(
                    {
                        let mut key = [0; 32];
                        for chunk in key.chunks_mut(8) {
                            chunk.copy_from_slice(&rng.next_u64().to_le_bytes());
                        }
                        key
                    },
                    NOW,
                )?,
            ),
        ] {
            service
                .request(
                    &scope,
                    Caller::default(),
                    Operation::Modify {
                        zone: zone.into(),
                        atomic: true,
                        changes: vec![Mutation::Save {
                            record: RecordInput::encrypted(name, ty, e),
                            if_unchanged: None,
                        }],
                    },
                )?
                .result
                .map_err(|_| failure())?;
        }
        let mut notes = Vec::new();
        for (name, p) in &payloads {
            if let codec::Payload::Note(n) = p {
                notes.push(n.clone())
            }
            let key = payload_key(p)?;
            let mut origin = [0; 16];
            origin[..8].copy_from_slice(&rng.next_u64().to_le_bytes());
            origin[8..].copy_from_slice(&rng.next_u64().to_le_bytes());
            let envelope = codec::Entity::build(
                codec::EntityFields {
                    kind: p.kind(),
                    key,
                    origin,
                    erev: 1,
                    generation: None,
                    tombstone: false,
                    deleted_at_ms: 0,
                    writer: binding.id.0,
                    base_sha256: None,
                },
                &p.encode()?,
            )?
            .encode()?;
            let response = service.request(
                &scope,
                Caller::default(),
                Operation::Modify {
                    zone: collection_zone.clone(),
                    atomic: true,
                    changes: vec![Mutation::Save {
                        record: RecordInput::encrypted(name, "MGEntity", envelope.clone()),
                        if_unchanged: None,
                    }],
                },
            )?;
            let Ok(Reply::Modified(items)) = response.result else {
                return Err(failure());
            };
            let Some(MutationResult {
                result: Ok(MutationSuccess::Saved(saved)),
                ..
            }) = items.into_iter().next()
            else {
                return Err(failure());
            };
            store.prepare_entity_baseline_for_testing(
                binding.id,
                name,
                &envelope,
                production::OpaqueBytes(saved.system_fields.bytes().to_vec()),
            )?;
        }
        notes.sort_by_key(|n| n.id);
        let note = notes.first().cloned().ok_or_else(failure)?;
        let entity_name = codec::entity_name(&record_key, codec::Kind::Note, &codec::id_key(note.id))?;
        store.prepare_drained_baseline_for_testing(
            binding.id,
            NativeFiles.identity(&directory.join("collection.anki2"))?,
            baseline_cursor,
            feed.status.feed_id.0,
            feed.status.counter,
        )?;
        Ok(Self {
            directory: directory.into(),
            scope,
            binding,
            collection_zone,
            entity_name,
            note,
            seed,
            notes,
        })
    }
    pub fn notes(&self) -> &[codec::Note] {
        &self.notes
    }
    pub fn entity_names(&self, kind: codec::Kind) -> BTreeSet<String> {
        runtime::with_tracking_access(|| -> Result<_> {
            let s = open_store(self)?;
            let mut out = BTreeSet::new();
            let mut after = None;
            loop {
                let page = s.entity_page(self.binding.id, kind, after.as_deref())?;
                if page.is_empty() {
                    break;
                }
                for e in page {
                    after = Some(e.key);
                    out.insert(e.name);
                }
            }
            Ok(out)
        })
        .expect("baseline entity names")
    }
    pub fn read_entity_durable(&self, name: &str) -> Result<EntityFacts> {
        runtime::with_tracking_access(|| {
            open_store(self)?
                .entity_facts(self.binding.id, name)?
                .ok_or_else(failure)
        })
    }
    pub fn read_collection_payloads(&self) -> Result<BTreeMap<String, codec::Payload>> {
        let ack = runtime::with_tracking_access(|| -> Result<_> {
            let s = open_store(self)?;
            let mut map = BTreeMap::new();
            for e in s.entity_page(self.binding.id, codec::Kind::Notetype, None)? {
                if let Some(codec::Payload::Notetype(n)) = e.acked_payload {
                    map.insert(n.id, n);
                }
            }
            Ok(map)
        })?;
        let reader = meguri_storage::ReaderFactory::recovery(
            &self.directory.join("collection.anki2"),
            &meguri_storage::Initializers::default(),
        )
        .open(ReaderRole::Query)?;
        let snapshot = reader.begin_snapshot()?;
        collect(&snapshot, &self.binding.facts.record_key.0, &ack)
    }
    pub fn read_durable(&self) -> Result<DeviceFacts> {
        let payloads = self.read_collection_payloads()?;
        let Some(codec::Payload::Note(note)) = payloads.get(&self.entity_name) else {
            return Err(failure());
        };
        runtime::with_tracking_access(|| {
            let s = open_store(self)?;
            let e = s
                .entity_facts(self.binding.id, &self.entity_name)?
                .ok_or_else(failure)?;
            let state = s.stored_runtime_state(self.binding.id)?.ok_or_else(failure)?;
            let binding = s.binding(self.binding.id)?.ok_or_else(failure)?;
            let mut pending_names = BTreeSet::new();
            for kind in row_payload::KINDS {
                let mut after = None;
                loop {
                    let page = s.entity_page(self.binding.id, kind, after.as_deref())?;
                    if page.is_empty() {
                        break;
                    }
                    for e in page {
                        after = Some(e.key);
                        if e.mark.is_some() {
                            pending_names.insert(e.name);
                        }
                    }
                }
            }
            let sidecar_clean =
                meguri_storage::sidecar::read_document(&self.directory.join("meguri-sidecar.json"), |doc| {
                    Ok(doc.and_then(|d| d.get("clean_shutdown")).and_then(|v| v.as_bool()) == Some(true))
                })?;
            Ok(DeviceFacts {
                note: note.clone(),
                local_rev: e.local_rev,
                change_counter: state.change_counter,
                last_observed_digest: Some(e.payload.row_digest()?),
                mark: e.mark.map(|m| DirtyMark {
                    binding: self.binding.id,
                    source: m.source,
                    change_seq: m.change_seq,
                    payload_digest: m.payload_digest,
                }),
                pending_names,
                binding_state: binding.facts.state,
                scan_drained: state.drained,
                sidecar_clean,
            })
        })
    }
}
pub(crate) fn payload_key(p: &codec::Payload) -> Result<Vec<u8>> {
    Ok(row_payload::payload_key(p)?)
}
