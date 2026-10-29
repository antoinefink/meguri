//! Unbound child commands. Collection reads never consult tracking payloads.
use super::super::coordinator::{BackupResult, CollectionHeader, RecoveryState, RestoredWire};
use super::*;
use meguri_sync::cloud::row_payload::{self, View};
use std::path::{Component, PathBuf};

impl Device {
    pub(super) fn collection_rows(&self) -> Result<Vec<codec::Payload>> {
        let collection = self.host.collection()?;
        let lease = collection.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        project_rows(&*snapshot)
    }
    fn backup_path(&self, name: &str) -> Result<PathBuf> {
        let mut components = std::path::Path::new(name).components();
        if !matches!(components.next(), Some(Component::Normal(_)))
            || components.next().is_some()
            || meguri_package::backup::naming::parse_name(name).is_none()
        {
            return Err(failure());
        }
        Ok(self.options.backups_directory.join(name))
    }
    fn backup_rows(&self, name: &str) -> Result<Vec<codec::Payload>> {
        let path = self.backup_path(name)?;
        let inspected = meguri_package::inspect_collection_package(
            &path,
            &meguri_package::CollectionPackageInspectContext {
                collection_directory: &self.options.collection_directory,
                live_collection_size: 0,
                reserved_space_bytes: 0,
                clock: self.options.clock.as_ref(),
                random: self.options.random.as_ref(),
                space: self.options.disk_space_provider.as_ref(),
                is_cancelled: &|| false,
            },
        )?;
        let reader = meguri_storage::connections::ReaderFactory::recovery(
            &inspected.collection_path(),
            &meguri_storage::conn_init::Initializers::new(),
        )
        .open(ReaderRole::Query)?;
        project_rows(&reader)
    }
}
fn project_rows(rows: &impl ReadRows) -> Result<Vec<codec::Payload>> {
    let layouts = devices::layouts(rows)?;
    let view = row_payload::ReadView(rows);
    let mut result = Vec::new();
    let mut bytes = 0;
    for kind in row_payload::KINDS {
        let Some(table) = row_payload::table(kind) else {
            continue;
        };
        let mut after = None;
        loop {
            let page = view.page(table, after.as_ref())?;
            if page.is_empty() {
                break;
            }
            for row in page {
                after = Some(RowKey::from_stored(table, &row)?);
                let payload = row_payload::project(&view, kind, &row, |id| {
                    layouts.get(&id).cloned().ok_or(core::Error::InvalidData)
                })?;
                bytes += payload.encode()?.len();
                if result.len() >= 2048 || bytes > 8 * 1024 * 1024 {
                    return Err(failure());
                }
                result.push(payload);
            }
        }
    }
    Ok(result)
}
pub(super) fn handle(device: &mut Device, input: Input) -> Result<serde_json::Value> {
    fn value<T: Serialize>(value: T) -> Result<serde_json::Value> {
        Ok(serde_json::to_value(value)?)
    }
    let collection = device.host.collection()?;
    match input {
        Input::BackupPath(name) => value(device.backup_path(&name)?),
        Input::BackupNotices => {
            use super::super::coordinator::BackupNotice;
            let facts = device.runtime()?.publication_facts(&collection)?;
            let mut notices = Vec::new();
            if let Some(binding) = facts.current {
                match binding.facts.state {
                    core::BindingState::Isolated(core::IsolationReason::FileMismatch) => {
                        notices.push(BackupNotice::OutsideReplacementNoBackup)
                    }
                    core::BindingState::Isolated(core::IsolationReason::ReplacedLocally) => {
                        let evidence = meguri_engine::device_state::DeviceState::for_collection(collection.path())
                            .replacement_evidence_for(binding.facts.publication)?;
                        if let Some(backup) = evidence.and_then(|e| e.safety_backup) {
                            let path = device.backup_path(&backup.name)?;
                            notices.push(match std::fs::symlink_metadata(path) {
                                Ok(metadata) if metadata.file_type().is_file() => {
                                    BackupNotice::ReplacementBackupRetained { name: backup.name }
                                }
                                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                    BackupNotice::ReplacementBackupExpired { name: backup.name }
                                }
                                _ => return Err(failure()),
                            });
                        } else {
                            notices.push(BackupNotice::ReplacementBackupUnknown);
                        }
                    }
                    _ => {}
                }
            }
            value(notices)
        }
        Input::RootConfirmed => value(device.runtime()?.sdk_root_confirmed()),
        Input::RegisterSDK(old) => {
            if let Some(old) = old {
                device.runtime()?.retire_sdk(&old, false)?;
                device.runtime()?.release_sdk(&old)?;
            }
            let registration = device.runtime()?.register_sdk(device.automatic)?;
            device.runtime()?.start_sdk_instance(&registration.0)?;
            value(registration)
        }
        Input::PreviewBegin => value(
            device
                .runtime()?
                .begin_connection_preview(&collection, &device.host.media()?)?,
        ),
        Input::PreviewActive(id, read) => value(device.runtime()?.connection_active_read(id, read)?),
        Input::PreviewEpoch(id, read) => value(device.runtime()?.connection_epoch_read(id, read)?),
        Input::PreviewCancel => {
            device.runtime()?.cancel_connection_preview();
            value(())
        }
        Input::PublicationConfirm(preview) => value(
            device
                .runtime()?
                .confirm_connection_publication(&preview, &collection)?,
        ),
        Input::AdoptionConfirm(preview) => value(device.host.confirm_cloud_adoption(&preview)?),
        Input::AdoptionReply(work, reply) => {
            device.host.cloud_adoption_reply(&work, reply)?;
            value(())
        }
        Input::AdoptionLocal(work) => {
            device.host.perform_cloud_adoption_local(&work)?;
            value(())
        }
        Input::PublicationNext => value(device.runtime()?.next_publication_work(&collection)?),
        Input::PublicationLocal(id, work) => {
            device.runtime()?.perform_publication_local(id, work, &collection)?;
            value(())
        }
        Input::PublicationCancel(id) => {
            if device.runtime()?.adoption_op()?.is_some_and(|op| op.id == id) {
                device.runtime()?.cancel_cloud_adoption(id, &collection)?;
            } else {
                device.runtime()?.cancel_publication(id, &collection)?;
            }
            value(())
        }
        Input::PublicationTrigger => {
            device.runtime()?.publication_trigger();
            value(())
        }
        Input::PublicationFacts => value(device.runtime()?.publication_facts(&collection)?),
        Input::CollectionRows => value(device.collection_rows()?),
        Input::CollectionHeader => {
            let lease = collection.reader(ReaderRole::Query)?;
            let col = lease.col()?;
            value(CollectionHeader {
                crt: col.created.0,
                ver: col.version,
            })
        }
        Input::BackupNow => {
            let backup = device.host.engine().back_up_now(&meguri_engine::CancelToken::new())?;
            value(BackupResult {
                name: backup.receipt.name,
                deleted: backup.deleted_names,
            })
        }
        Input::RestoreBackup(name) => {
            let restored = device.host.restore_backup(
                &name,
                &meguri_engine::CancelToken::new(),
                &meguri_engine::ProgressSink::default(),
            )?;
            value(RestoredWire::from(restored))
        }
        Input::BackupRows(name) => value(device.backup_rows(&name)?),
        Input::BackupRetention(daily, weekly, monthly) => {
            let mut limits = {
                let lease = collection.reader(ReaderRole::Query)?;
                meguri_engine::backup_limits::read(&*lease)?
            };
            limits.daily = daily;
            limits.weekly = weekly;
            limits.monthly = monthly;
            meguri_engine::backup_limits::save(&collection, limits)?;
            value(())
        }
        Input::PutMedia(name, bytes) => value(device.host.media()?.write_exact(&name, &bytes)?),
        Input::MediaFiles => {
            let media = device.host.media()?;
            let snapshot = media.tracked_names()?;
            let mut result = BTreeMap::new();
            let mut total = 0;
            for name in snapshot.names {
                if let Some(mut file) = media.store().open_file(&name.name).map_err(|_| failure())? {
                    let mut bytes = Vec::new();
                    Read::by_ref(&mut file)
                        .take(8 * 1024 * 1024 + 1)
                        .read_to_end(&mut bytes)?;
                    total += bytes.len();
                    if total > 8 * 1024 * 1024 {
                        return Err(failure());
                    }
                    result.insert(name.name, bytes);
                }
            }
            value(result)
        }
        Input::RecoveryState => {
            let decision = meguri_engine::device_state::DeviceState::for_collection(collection.path())
                .publication_decision()?
                .ok_or_else(failure)?;
            value(RecoveryState {
                publication_fence_decision: (decision.publication.decision
                    == meguri_engine::device_state::PublicationDecision::Pending)
                    .then_some(decision),
            })
        }
        Input::ZonesSeen(zones) => {
            device
                .runtime()?
                .publication_zones_seen(&device.runtime()?.instance()?, &zones)?;
            value(())
        }
        Input::CollectionDrained => {
            device
                .runtime()?
                .publication_collection_drained(&device.runtime()?.instance()?)?;
            value(())
        }
        Input::DeletionGuard => {
            device.deletion = Some(
                device
                    .runtime()?
                    .begin_zone_deletion_read(&device.runtime()?.instance()?, &collection)?,
            );
            value(())
        }
        Input::DeletionCheck(read) => value(device.runtime()?.zone_deletion_check(
            device.deletion.as_ref().ok_or_else(failure)?,
            read,
            &collection,
        )?),
        _ => Err(failure()),
    }
}
