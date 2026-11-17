//! P4 driver over the shared SDK stand-in and actual production child.
pub use super::child_driver::{CaptureProof, Edit, Killed, Receipt, ScanProof, ScanState};
use super::{
    adapter::{BatchAttempt, SDKRig},
    child_driver::Input,
    devices::{DeviceFacts, IdentityReads, PreboundDevice, Result, failure},
    *,
};
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{self as production, control_codec};
pub use production::workflow::TraceEvent;
use std::cell::RefCell;

// Establish an older last-applied fact through actual root-page intake,
// independently of the confirmation branches under regression.
pub fn store_older_active(
    service: &mut Service<SeededRandom>,
    files: &PreboundDevice,
) -> Result<production::workflow::Saved> {
    use super::devices::{NativeFiles, options};
    use meguri_sync::cloud::{
        OpaqueBytes,
        runtime::{CloudInputs, IdentityProvider, IdentityStatus},
    };
    use std::sync::Arc;
    struct Account(String);
    impl IdentityProvider for Account {
        fn read_identity(&self, _: &meguri_sync::cloud::BuildScope) -> IdentityStatus {
            IdentityStatus::Available(OpaqueBytes(self.0.as_bytes().to_vec()))
        }
    }
    let mut config = options(&files.directory, 1_735_689_600_000, files.seed);
    config.manual_transfer_directory = files.directory.join("synthetic-provider-installation");
    meguri_host::CoreHost::migrate_provider_configuration(&config.manual_transfer_directory, meguri_sync::provider::LegacyInput::Settings(
        meguri_sync::provider::LegacySettings { enabled: true, paused: false, automatic: false, media: true }
    ))?;
    config.cloud = Some(CloudInputs {
        scope: files.binding.scope.clone(),
        identity: Arc::new(Account(files.scope.account.clone())),
        files: Arc::new(NativeFiles),
    });
    let host = meguri_host::CoreHost::open(config)?;
    let runtime = host.cloud_runtime().ok_or_else(failure)?;
    runtime.refresh_identity().map_err(|_| failure())?;
    let collection = host.collection()?;
    let media = host.media()?;
    media.store().register_full(|_, _| true).map_err(|_| failure())?;
    runtime
        .reconcile_binding(&collection, &media, std::time::Duration::from_secs(30))
        .map_err(|_| failure())?;
    host.refresh_provider_admission();
    let (sdk, _, _) = runtime.register_sdk(false).map_err(|_| failure())?;
    let response = service.request(
        &files.scope,
        Caller {
            instance: Some(sdk.clone()),
            ..Caller::default()
        },
        Operation::FetchZone {
            zone: "meguri-root".into(),
            token: None,
        },
    )?;
    let Ok(Reply::ZoneChanges(page)) = response.result else {
        panic!("expected the actual root fetch page");
    };
    assert!(!page.more);
    let records = page
        .changes
        .iter()
        .map(|change| {
            let RecordChange::Saved(record) = change else {
                panic!("expected the actual root Active record");
            };
            assert_eq!(record.name, "active");
            assert_eq!(record.record_type, "MGActive");
            let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
                panic!("expected the actual Active envelope");
            };
            production::workflow::Saved {
                name: record.name.clone(),
                envelope: envelope.clone(),
                system_fields: record.system_fields.bytes().to_vec(),
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    runtime
        .adapter_fetched(&sdk, "meguri-root", &records, &[])
        .map_err(|_| failure())?;
    assert!(!runtime.sdk_root_confirmed());
    host.close()?;
    Ok(records.into_iter().next().unwrap())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Barrier {
    IdentityBlocked,
    ScanCommitted,
    BeforeRootConfirmation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadKind {
    Epoch,
    ActiveNoState,
    ActiveAfterLostZone,
}
#[derive(Clone, Debug)]
pub struct ReadReceipt {
    pub instance: String,
    pub request_id: u64,
    pub scope: Scope,
    pub zone: String,
    pub active_epoch: Option<production::EpochId>,
    pub record: Record,
    pub kind: ReadKind,
    pub lost_zone: bool,
}
#[derive(Clone, Debug)]
pub struct RootFetch {
    pub instance: String,
    pub responses: Vec<Response>,
    active: Option<production::workflow::Saved>,
}
pub struct Coordinator {
    rig: RefCell<SDKRig>,
    trace: Vec<TraceEvent>,
    identity_checked: bool,
    scanned: bool,
    started: bool,
}
impl Coordinator {
    pub fn open(files: &PreboundDevice, identity: IdentityReads) -> Result<Self> {
        Ok(Self {
            rig: RefCell::new(SDKRig::open_with_identity(
                files,
                identity,
                false,
                1_735_689_600_000,
                files.seed,
            )?),
            trace: Vec::new(),
            identity_checked: false,
            scanned: false,
            started: false,
        })
    }
    fn refresh(&mut self) -> Result<()> {
        self.trace = self.rig.get_mut().driver.call(Input::Trace)?;
        self.rig.get_mut().refresh()
    }
    pub fn instance(&self) -> String {
        self.rig.borrow().instance()
    }
    pub fn trace(&self) -> &[TraceEvent] {
        &self.trace
    }
    pub fn reach(&mut self, barrier: Barrier) -> Result<()> {
        if !self.identity_checked {
            self.rig.get_mut().driver.call::<()>(Input::Identity(None))?;
            self.identity_checked = true;
        }
        if barrier != Barrier::IdentityBlocked && !self.scanned {
            self.rig.get_mut().driver.call::<()>(Input::Reconcile)?;
            self.scanned = true;
        }
        if barrier == Barrier::BeforeRootConfirmation && !self.started {
            let instance = self.rig.get_mut().instance();
            self.rig.get_mut().driver.call::<()>(Input::StartSDK(instance))?;
            self.started = true;
        }
        self.refresh()
    }
    pub fn verify_identity(&mut self, account: String) -> Result<()> {
        self.rig.get_mut().driver.call::<()>(Input::Identity(Some(account)))?;
        self.refresh()
    }
    pub fn facts(&self) -> Result<DeviceFacts> {
        self.rig.borrow_mut().driver.call(Input::Facts)
    }
    pub fn last_applied_active(&self) -> Result<Option<production::workflow::Saved>> {
        self.rig.borrow_mut().driver.call(Input::LastAppliedActive)
    }
    /// Read the actual durable attention mark and Runtime projection independently.
    pub fn epoch_readability(&self) -> Result<(Option<production::OpaqueBytes>, u64, bool, bool)> {
        self.rig.borrow_mut().driver.call(Input::EpochReadability)
    }
    pub fn scan_state(&self) -> Result<ScanState> {
        self.rig.borrow_mut().driver.call(Input::ScanState)
    }
    pub fn scan_proof(&self) -> Result<ScanProof> {
        self.rig.borrow_mut().driver.call(Input::ScanProof)
    }
    pub fn capture_proof(&self, edit: &Edit) -> Result<CaptureProof> {
        self.rig
            .borrow_mut()
            .driver
            .call(Input::Capture(edit.seq, edit.generation))
    }
    pub fn wal_proof(&self, edit: &Edit) -> Result<Receipt> {
        self.rig.borrow_mut().driver.call(Input::Wal(edit.seq))
    }
    pub fn arm_capture_loss_for_test(&mut self, id: i64) -> Result<()> {
        self.rig.get_mut().driver.call(Input::ArmLoss(id))
    }
    pub fn edit_note_fields(&mut self, id: i64, fields: &[&str]) -> Result<Edit> {
        let edit = self.rig.get_mut().driver.edit_note_fields(id, fields)?;
        self.refresh()?;
        Ok(edit)
    }
    pub fn request_collection_batch(&mut self, service: &mut Service<SeededRandom>) -> Result<BatchAttempt> {
        let a = self.rig.get_mut().request_collection_batch(service)?;
        self.refresh()?;
        Ok(a)
    }
    pub fn read(&mut self, service: &mut Service<SeededRandom>, kind: ReadKind) -> Result<ReadReceipt> {
        let rig = self.rig.get_mut();
        let mut lost_zone = false;
        if kind != ReadKind::Epoch {
            rig.qualify_no_state(service, 10_000)?;
        }
        if kind == ReadKind::ActiveAfterLostZone {
            let r = service.request(
                &rig.files.scope,
                rig.caller(),
                Operation::FetchZone {
                    zone: rig.files.collection_zone.clone(),
                    token: None,
                },
            )?;
            lost_zone = matches!(
                r.result,
                Err(ServiceError {
                    code: ErrorCode::ZoneNotFound,
                    ..
                })
            );
            if !lost_zone {
                return Err(failure());
            }
        }
        let response = service.request(
            &rig.files.scope,
            rig.caller(),
            Operation::Read {
                zone: if kind == ReadKind::Epoch {
                    rig.files.collection_zone.clone()
                } else {
                    "meguri-root".into()
                },
                name: if kind == ReadKind::Epoch {
                    "epoch".into()
                } else {
                    "active".into()
                },
                fields: RequestedFields::All,
            },
        )?;
        let Ok(Reply::Record(record)) = response.result else {
            return Err(failure());
        };
        let active_epoch = if kind == ReadKind::Epoch {
            None
        } else {
            let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
                return Err(failure());
            };
            Some(production::EpochId(control_codec::Active::decode(e)?.epoch))
        };
        Ok(ReadReceipt {
            instance: rig.instance(),
            request_id: response.request_id,
            scope: rig.files.scope.clone(),
            zone: if kind == ReadKind::Epoch {
                rig.files.collection_zone.clone()
            } else {
                "meguri-root".into()
            },
            active_epoch,
            record,
            kind,
            lost_zone,
        })
    }
    pub fn deliver_read(&mut self, read: &ReadReceipt) -> Result<()> {
        if read.kind == ReadKind::Epoch {
            self.rig.get_mut().driver.call::<()>(Input::ServiceEpoch(
                read.instance.clone(),
                read.request_id,
                read.scope.clone(),
                read.zone.clone(),
                read.record.clone(),
            ))?;
        } else {
            let Some(Field::EncryptedBytes(e)) = read.record.fields.get("e") else {
                return Err(failure());
            };
            self.rig.get_mut().driver.call::<bool>(Input::RootRead(
                read.instance.clone(),
                read.request_id,
                e.clone(),
                read.record.system_fields.bytes().to_vec(),
                read.lost_zone,
            ))?;
        }
        self.refresh()
    }
    /// Exercise legacy fetched-page transport with the original Service read.
    /// This grants no native SDK/FetchRead authority or cursor-resume proof.
    pub fn deliver_epoch_page(&mut self, read: &ReadReceipt) -> Result<()> {
        if read.kind != ReadKind::Epoch { return Err(failure()); }
        let rig = self.rig.get_mut();
        let callback = rig.driver.stage_fetched(
            read.instance.clone(), vec![read.record.clone()], read.scope.clone(),
            read.zone.clone(), Vec::new(), None,
        )?;
        rig.driver.call::<()>(Input::Fetched(callback))?;
        self.refresh()
    }
    pub fn fetch_root(&mut self, service: &mut Service<SeededRandom>) -> Result<RootFetch> {
        let rig = self.rig.get_mut();
        let mut token = None;
        let mut responses = Vec::new();
        let mut active = None;
        for _ in 0..10_000 {
            let response = service.request(
                &rig.files.scope,
                rig.caller(),
                Operation::FetchZone {
                    zone: "meguri-root".into(),
                    token,
                },
            )?;
            let Ok(Reply::ZoneChanges(page)) = &response.result else {
                return Err(failure());
            };
            for change in &page.changes {
                match change {
                    RecordChange::Saved(r) if r.name == "active" && r.record_type == "MGActive" => {
                        if let Some(Field::EncryptedBytes(e)) = r.fields.get("e") {
                            control_codec::Active::decode(e)?;
                            active = Some(production::workflow::Saved {
                                name: r.name.clone(),
                                envelope: e.clone(),
                                system_fields: r.system_fields.bytes().to_vec(),
                            });
                        }
                    }
                    RecordChange::Deleted { name, .. } if name == "active" => active = None,
                    _ => {}
                }
            }
            let more = page.more;
            token = Some(page.token.clone());
            responses.push(response);
            if !more {
                return Ok(RootFetch {
                    instance: rig.instance(),
                    responses,
                    active,
                });
            }
        }
        Err(failure())
    }
    pub fn deliver_root_fetch(&mut self, fetch: &RootFetch) -> Result<()> {
        let completed = matches!(fetch.responses.last().map(|r|&r.result),Some(Ok(Reply::ZoneChanges(p))) if !p.more);
        self.rig.get_mut().driver.call::<bool>(Input::RootFetch(
            fetch.instance.clone(),
            fetch.active.clone(),
            completed,
            fetch.responses.last().map_or(0, |r| r.request_id),
        ))?;
        self.refresh()
    }
    pub fn close_clean(&mut self) -> Result<()> {
        self.refresh()?;
        self.rig.get_mut().close_clean()
    }
    pub fn kill_sigkill(&mut self) -> Result<Killed> {
        self.refresh()?;
        self.rig.get_mut().driver.kill_sigkill()
    }
}

/// Transport one Runtime-owned request. The three zone saves are one own
/// modify-zones operation attribution, and never SDK database pending changes.
/// Returned responses are the actual Service replies, including save conflicts.
pub fn execute_publication_request(
    service: &mut Service<SeededRandom>,
    scope: &Scope,
    request: &production::publication::OwnRequest,
    runtime: &production::runtime::Runtime,
    collection: &meguri_engine::Collection,
) -> Result<(production::publication::OwnResult, Vec<Response>)> {
    struct Direct<'a> {
        service: &'a mut Service<SeededRandom>,
        scope: &'a Scope,
    }
    impl PublicationTransport for Direct<'_> {
        type Enqueued = Response;
        fn enqueue(&mut self, caller: Caller, operation: Operation) -> Result<Response> {
            Ok(self.service.request(self.scope, caller, operation)?)
        }
        fn reply(&mut self, response: Response) -> Result<Response> {
            Ok(response)
        }
    }
    execute_publication_transport(request, runtime, collection, &mut Direct { service, scope })
}
pub(crate) trait PublicationTransport {
    type Enqueued;
    fn enqueue(&mut self, caller: Caller, operation: Operation) -> Result<Self::Enqueued>;
    fn reply(&mut self, enqueued: Self::Enqueued) -> Result<Response>;
}
/// Only synchronous enqueue retains collection authority. Reply waits and nested
/// lifecycle commands run after the guard has released all leases and locks.
pub(crate) fn execute_publication_transport(
    request: &production::publication::OwnRequest,
    runtime: &production::runtime::Runtime,
    collection: &meguri_engine::Collection,
    transport: &mut impl PublicationTransport,
) -> Result<(production::publication::OwnResult, Vec<Response>)> {
    use production::publication::{OwnOperation, OwnResult, RecordRead};
    let caller = Caller {
        own_operation: Some(format!("publication-{:02x?}-{}", request.op, request.attempt)),
        ..Caller::default()
    };
    let mut replies = Vec::new();
    let mut submit = |caller, operation| -> Result<Response> {
        let enqueued = runtime
            .submit_publication_request(request, collection, || transport.enqueue(caller, operation))
            .map_err(|_| failure())??;
        transport.reply(enqueued)
    };
    let result = match &request.operation {
        OwnOperation::CreateZones(zones) => {
            let mut succeeded = true;
            for zone in zones {
                let response = submit(caller.clone(), Operation::CreateZone { zone: zone.clone() })?;
                succeeded &= response.result.is_ok();
                replies.push(response);
            }
            if succeeded {
                OwnResult::ZonesCreated
            } else {
                OwnResult::Failed { retry_after_ms: None }
            }
        }
        OwnOperation::Read { zone, name } => {
            let response = submit(
                caller,
                Operation::Read {
                    zone: zone.clone(),
                    name: name.clone(),
                    fields: RequestedFields::All,
                },
            )?;
            let read = match &response.result {
                Ok(Reply::Record(record)) => RecordRead::Found(publication_control_record(record)?),
                Err(error) if error.code == ErrorCode::UnknownItem => RecordRead::Missing,
                Err(error) if error.code == ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
                _ => RecordRead::Failed,
            };
            replies.push(response);
            OwnResult::Read(read)
        }
        OwnOperation::Save {
            zone,
            name,
            record_type,
            encrypted,
            if_unchanged,
        } => {
            let response = submit(
                caller,
                Operation::Modify {
                    zone: zone.clone(),
                    atomic: false,
                    changes: vec![Mutation::Save {
                        record: RecordInput::encrypted(name, record_type, encrypted.clone()),
                        if_unchanged: if_unchanged.as_ref().map(|s| SystemFields::from_bytes(s.0.clone())),
                    }],
                },
            )?;
            let result = match &response.result {
                Ok(Reply::Modified(items)) if items.len() == 1 => match &items[0].result {
                    Ok(MutationSuccess::Saved(record)) => OwnResult::Saved(publication_control_record(record)?),
                    Err(error) if error.code == ErrorCode::ServerRecordChanged => match &error.server_record {
                        Some(record) => OwnResult::Conflict(publication_control_record(record)?),
                        None => OwnResult::Failed { retry_after_ms: None },
                    },
                    _ => OwnResult::Failed { retry_after_ms: None },
                },
                _ => OwnResult::Failed { retry_after_ms: None },
            };
            replies.push(response);
            result
        }
    };
    Ok((result, replies))
}
/// Preserve the platform's absent encrypted field independently of missing rows.
pub fn publication_control_record(record: &Record) -> Result<production::publication::ControlRecord> {
    Ok(production::publication::ControlRecord {
        encrypted: match record.fields.get("e") {
            Some(Field::EncryptedBytes(bytes)) => Some(bytes.clone()),
            None => None,
            _ => return Err(failure()),
        },
        system_fields: production::OpaqueBytes(record.system_fields.bytes().to_vec()),
    })
}

#[path = "publication_rig.rs"]
mod publication_rig;
pub use publication_rig::*;
