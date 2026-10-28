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
            self.rig.get_mut().driver.call::<()>(Input::StartSDK)?;
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
            active_epoch,
            record,
            kind,
            lost_zone,
        })
    }
    pub fn deliver_read(&mut self, read: &ReadReceipt) -> Result<()> {
        if read.kind != ReadKind::Epoch {
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
