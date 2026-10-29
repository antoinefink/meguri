//! One bounded IPC driver shared by the cloud integration targets.
use super::{
    Scope,
    devices::{self, IdentityReads, PreboundDevice, Result, failure},
};
use meguri_engine::{Generation, Seq, SessionId, WalCoverage};
use meguri_model::{
    changes::OpSource,
    clock::{Clock, StepClock, UtcOffset},
    ids::{CardId, DeckId, NoteId},
};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
use meguri_sync::cloud::{
    self as core, entity_codec as codec,
    runtime::{self, IdentityProvider, IdentityStatus},
    workflow::*,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{
        net::UnixStream,
        process::{CommandExt, ExitStatusExt},
    },
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};
const FRAME: usize = 16 * 1024 * 1024;
const IPC_TIMEOUT: Duration = Duration::from_secs(30);
pub use core::incoming::{ApplyIntent, ApplyKeyRole};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingInboxEntry {
    pub id: i64,
    pub record_name: String,
    /// Zero only for a deletion or an envelope with no readable revision.
    pub erev: u64,
    pub reason: core::InboxReason,
    pub retry_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingRefusedEntry {
    pub record_name: String,
    pub erev: u64,
    pub version_known: bool,
    pub origin: Option<[u8; 16]>,
    pub reason: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub note: codec::Note,
    pub seq: u64,
    pub generation: u64,
    pub native_file: runtime::NativeFileIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub through: u64,
    pub generation: u64,
    pub native_file: runtime::NativeFileIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanProof {
    pub snapshot_seq: u64,
    pub snapshot_generation: u64,
    pub snapshot_native_file: runtime::NativeFileIdentity,
    pub receipt: Receipt,
    pub recovered_names: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureProof {
    pub observed_seq: u64,
    pub generation: u64,
    pub attempted: bool,
    pub persisted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanState {
    pub scan_required: bool,
    pub scan_running: bool,
    pub scan_completed_this_open: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewAnswer {
    pub revlog_id: i64,
    pub op_id: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewHold {
    pub op_ids: BTreeSet<u64>,
    pub entity_names: BTreeSet<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdoptionCompletion {
    pub identity_changes: usize,
    pub slot_available: bool,
    pub stored_usable: bool,
    pub identity_verified: bool,
    pub proof_matches: bool,
}
#[derive(Clone, Debug)]
pub struct Killed {
    pub signal: Option<i32>,
    pub clean_close: bool,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Boot {
    directory: std::path::PathBuf,
    scope: Scope,
    stable: Option<(i64, core::BindingId)>,
    identity: IdentityReads,
    automatic: bool,
    now: i64,
    seed: u64,
}
#[derive(Serialize, Deserialize)]
pub(crate) enum Input {
    Boot(Boot),
    Instance,
    Validate(String),
    SignOut,
    Reopen,
    BodyReads,
    StoredState,
    Identity(Option<String>),
    Reconcile,
    StartSDK(String),
    Facts,
    ScanState,
    ScanProof,
    Capture(u64, u64),
    Wal(u64),
    ArmLoss(i64),
    ArmAdoptionActivationCrash,
    ArmAdoptionAccountChange(Option<String>),
    AdoptionCompletion(core::adoption::AdoptOp),
    Edit(i64, Vec<String>),
    Payloads,
    Entity(String),
    AllEntities,
    Poll(String),
    Trace,
    Observations,
    Emitted,
    Updates(String),
    Transition(String, UpdateKind, PendingSnapshot),
    Sendables,
    Build(String, BTreeSet<String>),
    EnterHandler(String, u64),
    ReturnHandler(String),
    Sent(String, u64, Vec<Saved>, Vec<(String, SendError)>, usize),
    AppliedUpdate(String, u64, Vec<String>),
    SavedRemoved(String, u64, Vec<String>),
    Qualify(String),
    RootRead(String, u64, Vec<u8>, Vec<u8>, bool),
    RootFetch(String, Option<Saved>, bool, u64),
    ReadDue,
    RootEpoch,
    SyncNow(String),
    FetchDue,
    FetchCompleted(String, FetchPhase, BTreeSet<String>),
    Fetched(String, Vec<Saved>),
    IncomingIntents(core::BindingId),
    IncomingInbox(core::BindingId),
    IncomingRefused(core::BindingId),
    BindingEntity(core::BindingId, String),
    DeliverCollection(Vec<Saved>),
    DrainIncoming(usize),
    PauseAutomaticWAL(bool),
    CloudHold(u64),
    SendsFinished(String),
    WantsSend,
    StartCall(String),
    RecordCall(String, u64, u64),
    FinishCall(String, u64),
    Calls,
    Exchange(u64),
    Begin(i64),
    Start(u64),
    Show(u64, i64),
    Good(u64, i64),
    End(u64),
    Hold(u64),
    Card(i64),
    Revlog(i64),
    Advance(i64),
    Close,
    BackupPath(String),
    BackupNotices,
    RegisterSDK(Option<String>),
    RootConfirmed,
    PreviewBegin,
    PreviewActive(u64, core::publication::RecordRead),
    PreviewEpoch(u64, core::publication::RecordRead),
    PreviewCancel,
    PublicationConfirm(core::publication::Preview),
    AdoptionConfirm(core::publication::Preview),
    AdoptionReply(core::adoption::AdoptionWork, core::adoption::AdoptionReply),
    AdoptionLocal(core::adoption::AdoptionWork),
    PublicationNext,
    PublicationLocal([u8; 16], core::publication::LocalWork),
    PublicationExecute(core::publication::OwnRequest),
    PublicationCancel([u8; 16]),
    PublicationTrigger,
    PublicationFacts,
    CollectionRows,
    CollectionHeader,
    BackupNow,
    RestoreBackup(String),
    BackupRows(String),
    BackupRetention(u32, u32, u32),
    PutMedia(String, Vec<u8>),
    MediaFiles,
    RecoveryState,
    ZonesSeen(Vec<String>),
    CollectionDrained,
    DeletionGuard,
    DeletionCheck(core::publication::RecordRead),
    DeleteZone(String),
    KeyResult(std::result::Result<[u8; 32], core::runtime::KeySourceError>),
    TransportAccepted,
    TransportResult(super::Response),
}
#[derive(Serialize, Deserialize)]
enum ChildMessage {
    Reply(std::result::Result<serde_json::Value, String>),
    DrawKey,
    Submit(super::Caller, super::Operation),
}

fn write_frame<T: Serialize>(s: &mut UnixStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > FRAME {
        return Err(failure());
    }
    s.write_all(&u32::try_from(bytes.len())?.to_be_bytes())?;
    s.write_all(&bytes)?;
    Ok(())
}
fn read_frame<T: DeserializeOwned>(s: &mut UnixStream) -> Result<T> {
    let mut header = [0; 4];
    s.read_exact(&mut header)?;
    let n = u32::from_be_bytes(header) as usize;
    if n > FRAME {
        return Err(failure());
    }
    let mut bytes = vec![0; n];
    s.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub struct ChildDriver {
    child: Option<Child>,
    stream: UnixStream,
    keys: Option<Arc<dyn runtime::KeyByteSource>>,
    queued: Option<super::coordinator::HeldRequest>,
    queued_response: Option<super::Response>,
}
impl ChildDriver {
    pub fn apply_intents(&mut self, binding: core::BindingId) -> Result<Vec<ApplyIntent>> {
        self.call(Input::IncomingIntents(binding))
    }
    pub fn inbox_entries(&mut self, binding: core::BindingId) -> Result<Vec<IncomingInboxEntry>> {
        self.call(Input::IncomingInbox(binding))
    }
    pub fn refused_entries(&mut self, binding: core::BindingId) -> Result<Vec<IncomingRefusedEntry>> {
        self.call(Input::IncomingRefused(binding))
    }
    pub fn binding_entity(&mut self, binding: core::BindingId, name: &str) -> Result<Option<EntityFacts>> {
        self.call(Input::BindingEntity(binding, name.into()))
    }
    pub fn deliver_collection_records(&mut self, records: &[super::Record]) -> Result<()> {
        let records = records
            .iter()
            .map(|r| {
                let Some(super::Field::EncryptedBytes(bytes)) = r.fields.get("e") else {
                    return Err(failure());
                };
                Ok(Saved {
                    name: r.name.clone(),
                    envelope: bytes.clone(),
                    system_fields: r.system_fields.bytes().to_vec(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.call(Input::DeliverCollection(records))
    }
    pub fn drain_incoming(&mut self, steps: usize) -> Result<usize> {
        self.call(Input::DrainIncoming(steps))
    }
    pub fn pause_automatic_wal_flush_for_testing(&mut self, paused: bool) -> Result<()> {
        self.call(Input::PauseAutomaticWAL(paused))
    }
    pub fn read_cloud_review_hold(&mut self, session: u64) -> Result<ReviewHold> {
        self.call(Input::CloudHold(session))
    }
    pub(crate) fn spawn(
        files: &PreboundDevice,
        identity: IdentityReads,
        automatic: bool,
        now: i64,
        seed: u64,
    ) -> Result<Self> {
        Self::spawn_boot(
            Boot {
                directory: files.directory.clone(),
                scope: files.scope.clone(),
                stable: Some((files.note.id, files.binding.id)),
                identity,
                automatic,
                now,
                seed,
            },
            None,
        )
    }
    pub(crate) fn spawn_unbound(
        directory: &std::path::Path,
        scope: &Scope,
        identity: IdentityReads,
        automatic: bool,
        now: i64,
        seed: u64,
        keys: Arc<dyn runtime::KeyByteSource>,
    ) -> Result<Self> {
        Self::spawn_boot(
            Boot {
                directory: directory.to_owned(),
                scope: scope.clone(),
                stable: None,
                identity,
                automatic,
                now,
                seed,
            },
            Some(keys),
        )
    }
    fn spawn_boot(boot: Boot, keys: Option<Arc<dyn runtime::KeyByteSource>>) -> Result<Self> {
        let (parent, child_socket) = UnixStream::pair()?;
        parent.set_read_timeout(Some(IPC_TIMEOUT))?;
        parent.set_write_timeout(Some(IPC_TIMEOUT))?;
        let fd = child_socket.as_raw_fd();
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--exact", "cloud_child_entry", "--nocapture"])
            .env("MEGURI_CLOUD_CHILD_FD", fd.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Only the selected socket survives exec; all other descriptors retain CLOEXEC.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(child_socket);
        let mut driver = Self {
            child: Some(child),
            stream: parent,
            keys,
            queued: None,
            queued_response: None,
        };
        driver.call::<()>(Input::Boot(boot))?;
        Ok(driver)
    }
    fn next_message(&mut self) -> Result<ChildMessage> {
        loop {
            match read_frame(&mut self.stream)? {
                ChildMessage::DrawKey => {
                    let draw = self.keys.as_ref().ok_or_else(failure)?.draw_key();
                    write_frame(&mut self.stream, &Input::KeyResult(draw))?;
                }
                message => return Ok(message),
            }
        }
    }
    fn reply<T: DeserializeOwned>(&mut self) -> Result<T> {
        let ChildMessage::Reply(result) = self.next_message()? else {
            return Err(failure());
        };
        Ok(serde_json::from_value(result.map_err(|_| failure())?)?)
    }
    pub(crate) fn call<T: DeserializeOwned>(&mut self, input: Input) -> Result<T> {
        write_frame(&mut self.stream, &input)?;
        self.reply()
    }
    /// Enter the real submission guard and enqueue its original request over IPC.
    /// The child waits for the actual reply, servicing inspection commands meanwhile.
    pub(crate) fn queued_request(&self) -> Result<super::coordinator::HeldRequest> {
        self.queued.clone().ok_or_else(failure)
    }
    pub(crate) fn enqueue(&mut self, input: Input) -> Result<super::coordinator::HeldRequest> {
        if self.queued.is_some() {
            return Err(failure());
        }
        write_frame(&mut self.stream, &input)?;
        let ChildMessage::Submit(caller, operation) = self.next_message()? else {
            return Err(failure());
        };
        let request = super::coordinator::HeldRequest { caller, operation };
        self.queued = Some(request.clone());
        write_frame(&mut self.stream, &Input::TransportAccepted)?;
        Ok(request)
    }
    pub(crate) fn execute<T: DeserializeOwned>(
        &mut self,
        input: Input,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<T> {
        write_frame(&mut self.stream, &input)?;
        self.transport_replies(service, scope)
    }
    /// Hold only delivery: the original guarded request has already reached Service.
    pub(crate) fn hold_reply(
        &mut self,
        input: Input,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<()> {
        let request = self.enqueue(input)?;
        self.queued_response = Some(service.request(scope, request.caller, request.operation)?);
        Ok(())
    }
    fn transport_replies<T: DeserializeOwned>(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<T> {
        loop {
            match self.next_message()? {
                ChildMessage::Reply(result) => return Ok(serde_json::from_value(result.map_err(|_| failure())?)?),
                ChildMessage::Submit(caller, operation) => {
                    write_frame(&mut self.stream, &Input::TransportAccepted)?;
                    let response = service.request(scope, caller, operation)?;
                    write_frame(&mut self.stream, &Input::TransportResult(response))?;
                }
                ChildMessage::DrawKey => unreachable!(),
            }
        }
    }
    pub(crate) fn resume<T: DeserializeOwned>(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<T> {
        let request = self.queued.take().ok_or_else(failure)?;
        let response = match self.queued_response.take() {
            Some(response) => response,
            None => service.request(scope, request.caller, request.operation)?,
        };
        write_frame(&mut self.stream, &Input::TransportResult(response))?;
        self.transport_replies(service, scope)
    }
    pub fn reconcile_binding(&mut self) -> Result<()> {
        self.call(Input::Reconcile)
    }
    pub fn qualify_without_root(&mut self, instance: String) -> Result<()> {
        self.call(Input::Qualify(instance))
    }
    pub fn stored_runtime_state(&mut self) -> Result<runtime::StoredRuntimeState> {
        self.call(Input::StoredState)
    }
    /// Fixed catalog boundary only; this test-only crate cannot arm arbitrary code.
    pub fn arm_adoption_activation_crash(&mut self) -> Result<()> {
        self.call(Input::ArmAdoptionActivationCrash)
    }
    /// Change only the platform identity input after the last safety check.
    /// None represents sign-out; the real production port must return Unknown.
    pub fn arm_adoption_account_change(&mut self, account: Option<String>) -> Result<core::adoption::AdoptOp> {
        self.call(Input::ArmAdoptionAccountChange(account))
    }
    pub fn adoption_completion(&mut self, op: &core::adoption::AdoptOp) -> Result<AdoptionCompletion> {
        self.call(Input::AdoptionCompletion(op.clone()))
    }
    /// Called after the command's EOF. Require a real abort, never a rig hold.
    pub fn reap_adoption_activation_crash(&mut self) -> Result<()> {
        let mut child = self.child.take().ok_or_else(failure)?;
        let status = child.wait()?;
        if status.signal() != Some(libc::SIGABRT) {
            return Err(failure());
        }
        Ok(())
    }
    pub fn set_account_for_testing(&mut self, account: String) -> Result<()> {
        self.call(Input::Identity(Some(account)))
    }
    pub fn sign_out_for_testing(&mut self) -> Result<()> {
        self.call(Input::SignOut)
    }
    pub fn reopen_for_testing(&mut self) -> Result<String> {
        self.call(Input::Reopen)
    }
    pub fn build_for_instance(&mut self, instance: String, names: BTreeSet<String>) -> Result<BuiltBatch> {
        self.call(Input::Build(instance, names))
    }
    pub fn accept_root_for_instance(
        &mut self,
        instance: String,
        id: u64,
        envelope: Vec<u8>,
        fields: Vec<u8>,
    ) -> Result<bool> {
        self.call(Input::RootRead(instance, id, envelope, fields, false))
    }
    pub fn last_capture_body_reads(&mut self) -> Result<usize> {
        self.call(Input::BodyReads)
    }
    pub fn edit_note_fields(&mut self, id: i64, fields: &[&str]) -> Result<Edit> {
        self.call(Input::Edit(id, fields.iter().map(|s| (*s).to_owned()).collect()))
    }
    pub fn review_begin(&mut self, note: i64) -> Result<u64> {
        self.call(Input::Begin(note))
    }
    pub fn review_start(&mut self, session: u64) -> Result<codec::Card> {
        self.call(Input::Start(session))
    }
    pub fn review_show_answer(&mut self, s: u64, c: i64) -> Result<()> {
        self.call(Input::Show(s, c))
    }
    pub fn review_answer_good(&mut self, s: u64, c: i64) -> Result<ReviewAnswer> {
        self.call(Input::Good(s, c))
    }
    pub fn review_end_and_drain(&mut self, s: u64) -> Result<()> {
        self.call(Input::End(s))
    }
    pub fn read_review_hold(&mut self, s: u64) -> Result<ReviewHold> {
        self.call(Input::Hold(s))
    }
    pub fn read_card(&mut self, id: i64) -> Result<codec::Card> {
        self.call(Input::Card(id))
    }
    pub fn read_revlog(&mut self, id: i64) -> Result<codec::Revlog> {
        self.call(Input::Revlog(id))
    }
    pub fn collection_payloads(&mut self) -> Result<BTreeMap<String, codec::Payload>> {
        self.call(Input::Payloads)
    }
    pub(crate) fn kill_sigkill(&mut self) -> Result<Killed> {
        let mut child = self.child.take().ok_or_else(failure)?;
        child.kill()?;
        let status = child.wait()?;
        Ok(Killed {
            signal: status.signal(),
            clean_close: false,
        })
    }
    pub(crate) fn close(&mut self) -> Result<()> {
        if self.child.is_none() {
            return Ok(());
        }
        self.call::<()>(Input::Close)?;
        let mut child = self.child.take().ok_or_else(failure)?;
        let pid = child.id();
        struct Cleanup(u32, bool);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if self.1 {
                    unsafe {
                        libc::kill(self.0 as i32, libc::SIGKILL);
                    }
                }
            }
        }
        let mut cleanup = Cleanup(pid, true);
        let (tx, rx) = std::sync::mpsc::channel();
        let reaper = std::thread::spawn(move || {
            let result = child.wait();
            let _ = tx.send(result);
        });
        let status = match rx.recv_timeout(IPC_TIMEOUT) {
            Ok(status) => status?,
            Err(_) => {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                let _ = rx.recv_timeout(IPC_TIMEOUT);
                reaper.join().map_err(|_| failure())?;
                return Err(failure());
            }
        };
        cleanup.1 = false;
        reaper.join().map_err(|_| failure())?;
        if !status.success() {
            return Err(failure());
        }
        Ok(())
    }
}
impl Drop for ChildDriver {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct Account(Mutex<IdentityStatus>);
static ADOPTION_IDENTITY_CHANGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl IdentityProvider for Account {
    fn read_identity(&self, _: &core::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
struct Device {
    host: meguri_host::CoreHost,
    options: meguri_host::HostConfig,
    automatic: bool,
    account: Arc<Account>,
    clock: Arc<StepClock>,
    stable: Option<(i64, core::BindingId)>,
    deletion: Option<core::publication::ZoneDeletionGuard>,
    session_started: BTreeMap<u64, i64>,
    closed: bool,
}
impl Device {
    fn new(b: Boot, stream: &UnixStream) -> Result<Self> {
        let clock = Arc::new(StepClock::new(b.now, UtcOffset::UTC, "UTC"));
        let account = Arc::new(Account(Mutex::new(match b.identity {
            IdentityReads::Available(a) => IdentityStatus::Available(core::OpaqueBytes(a.into_bytes())),
            IdentityReads::FailAllSession => IdentityStatus::ReadFailed,
        })));
        let mut options = devices::options(&b.directory, b.now, b.seed);
        options.clock = clock.clone();
        options.cloud = Some(runtime::CloudInputs {
            scope: devices::build_scope(&b.scope)?,
            identity: account.clone(),
            files: Arc::new(devices::NativeFiles),
        });
        if b.stable.is_none() {
            options.cloud_key_bytes = Some(Arc::new(ChildKeys(Mutex::new(stream.try_clone()?))));
        }
        let host = meguri_host::CoreHost::open(options.clone())?;
        Ok(Self {
            host,
            options,
            automatic: b.automatic,
            account,
            clock,
            stable: b.stable,
            deletion: None,
            session_started: BTreeMap::new(),
            closed: false,
        })
    }
    fn runtime(&self) -> Result<&runtime::Runtime> {
        self.host.cloud_runtime().map(|r| r.as_ref()).ok_or_else(failure)
    }
    fn all_facts(&self) -> Result<Vec<EntityFacts>> {
        let mut out = Vec::new();
        let mut total = 0;
        for kind in core::row_payload::KINDS {
            let mut after = None;
            loop {
                let page = self.runtime()?.entity_facts_page(kind, after.as_deref())?;
                if page.is_empty() {
                    break;
                }
                for e in page {
                    after = Some(e.key.clone());
                    total += e.payload.encode()?.len();
                    if out.len() >= 2048 || total > 8 * 1024 * 1024 {
                        return Err(failure());
                    }
                    out.push(e);
                }
            }
        }
        Ok(out)
    }
    fn payloads(&self) -> Result<BTreeMap<String, codec::Payload>> {
        let r = self.runtime()?;
        let mut layouts = BTreeMap::new();
        for e in self.all_facts()? {
            if let Some(codec::Payload::Notetype(n)) = e.acked_payload {
                layouts.insert(n.id, n);
            }
        }
        let binding = r.status()?.binding.ok_or_else(failure)?;
        let c = self.host.collection()?;
        let lease = c.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        devices::collect(&*snapshot, &binding.facts.record_key.0, &layouts)
    }
    fn card(&self, id: i64) -> Result<codec::Card> {
        self.payloads()?
            .into_values()
            .find_map(|p| match p {
                codec::Payload::Card(c) if c.id == id => Some(c),
                _ => None,
            })
            .ok_or_else(failure)
    }
    fn revlog(&self, id: i64) -> Result<codec::Revlog> {
        self.payloads()?
            .into_values()
            .find_map(|p| match p {
                codec::Payload::Revlog(c) if c.id == id => Some(c),
                _ => None,
            })
            .ok_or_else(failure)
    }
    fn receipt(&self) -> Result<Receipt> {
        let c = self.host.collection()?;
        let lease = c.reader(ReaderRole::Cloud)?;
        let snap = lease.snapshot()?;
        let native = runtime::FileIdentityProvider::identity(&devices::NativeFiles, c.path())?;
        match c.wait_wal_covered(snap.wal_identity(), snap.seq(), IPC_TIMEOUT)? {
            WalCoverage::Covered(r) if r.identity() == snap.wal_identity() && r.through() >= snap.seq() => {
                Ok(Receipt {
                    through: r.through().0,
                    generation: snap.generation().0,
                    native_file: native,
                })
            }
            _ => Err(failure()),
        }
    }
    fn facts(&self) -> Result<devices::DeviceFacts> {
        let r = self.runtime()?;
        let status = r.status()?;
        let binding = status.binding.ok_or_else(failure)?;
        let (id, stable_binding) = self.stable.ok_or_else(failure)?;
        let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Note, &codec::id_key(id))?;
        let e = r.entity_facts(&name)?.ok_or_else(failure)?;
        let payloads = self.payloads()?;
        let Some(codec::Payload::Note(note)) = payloads.get(&name) else {
            return Err(failure());
        };
        let pending_names = self
            .all_facts()?
            .into_iter()
            .filter(|e| e.mark.is_some())
            .map(|e| e.name)
            .collect();
        Ok(devices::DeviceFacts {
            note: note.clone(),
            local_rev: e.local_rev,
            change_counter: status.change_counter,
            last_observed_digest: Some(e.payload.row_digest()?),
            mark: e.mark.map(|m| devices::DirtyMark {
                binding: stable_binding,
                source: m.source,
                change_seq: m.change_seq,
                payload_digest: m.payload_digest,
            }),
            pending_names,
            binding_state: binding.facts.state,
            scan_drained: status.scan_drained,
            sidecar_clean: false,
        })
    }
    fn handle(&mut self, input: Input) -> Result<serde_json::Value> {
        fn value<T: Serialize>(v: T) -> Result<serde_json::Value> {
            Ok(serde_json::to_value(v)?)
        }
        match input {
            Input::ArmAdoptionAccountChange(account) => {
                let op = self.runtime()?.adoption_op()?.ok_or_else(failure)?;
                if !matches!(&*self.account.0.lock().unwrap(), IdentityStatus::Available(current) if current == &op.account)
                {
                    return Err(failure());
                }
                let next = match account {
                    Some(account) => IdentityStatus::Available(core::OpaqueBytes(account.into_bytes())),
                    None => IdentityStatus::SignedOut,
                };
                let account = self.account.clone();
                ADOPTION_IDENTITY_CHANGES.store(0, std::sync::atomic::Ordering::SeqCst);
                self.runtime()?.set_current_account_hook_for_test(move || {
                    *account.0.lock().unwrap() = next;
                    ADOPTION_IDENTITY_CHANGES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                });
                value(op)
            }
            Input::AdoptionCompletion(op) => {
                let status = self.runtime()?.status()?;
                let collection = self.host.collection()?;
                let file = runtime::FileIdentityProvider::identity(&devices::NativeFiles, collection.path())?;
                let proof_matches = collection.cloud_adopt_activation()?.is_some_and(|proof| {
                    proof.matches(
                        meguri_engine::device_state::PublicationId::from_bytes(op.id),
                        &op.before.publication,
                        &op.candidate,
                    ) && proof.native_file() == (file.device, file.inode)
                });
                let slot_available = self
                    .host
                    .engine()
                    .acquire_slot(meguri_engine::LongOp::CloudAdopt)
                    .is_ok();
                value(AdoptionCompletion {
                    identity_changes: ADOPTION_IDENTITY_CHANGES.load(std::sync::atomic::Ordering::SeqCst),
                    slot_available,
                    stored_usable: status.stored_usable,
                    identity_verified: status.identity_verified,
                    proof_matches,
                })
            }
            Input::ArmAdoptionActivationCrash => {
                self.runtime()?.arm_adoption_activation_crash_for_testing();
                value(())
            }
            Input::Boot(_)
            | Input::KeyResult(_)
            | Input::TransportAccepted
            | Input::TransportResult(_)
            | Input::PublicationExecute(_)
            | Input::DeleteZone(_) => Err(failure()),
            Input::Validate(i) => {
                self.runtime()?.validate_caller(&i)?;
                value(())
            }
            Input::SignOut => {
                *self.account.0.lock().unwrap() = IdentityStatus::SignedOut;
                self.runtime()?.refresh_identity()?;
                value(())
            }
            Input::Reopen => {
                self.host.close()?;
                self.host = meguri_host::CoreHost::open(self.options.clone())?;
                self.runtime()?.refresh_identity()?;
                value(self.runtime()?.instance()?)
            }
            Input::StoredState => value(self.runtime()?.stored_state_for_testing()?),
            Input::BodyReads => value(self.runtime()?.last_capture_body_reads_for_testing()),
            Input::Instance => value(self.runtime()?.instance()?),
            Input::Identity(account) => {
                if let Some(a) = account {
                    *self.account.0.lock().unwrap() = IdentityStatus::Available(core::OpaqueBytes(a.into_bytes()));
                }
                self.runtime()?.refresh_identity()?;
                value(())
            }
            Input::Reconcile => {
                let c = self.host.collection()?;
                let media = self.host.media()?;
                if self.stable.is_none() {
                    media.store().register_full(|_, _| true).map_err(|_| failure())?;
                }
                self.runtime()?.reconcile_binding(&c, &media, IPC_TIMEOUT)?;
                value(())
            }
            Input::StartSDK(i) => {
                self.runtime()?.set_automatic(&i, self.automatic)?;
                self.runtime()?.start_sdk_instance(&i)?;
                value(())
            }
            Input::Facts => value(self.facts()?),
            Input::ScanState => {
                let s = self.runtime()?.status()?;
                value(ScanState {
                    scan_required: s.scan_required,
                    scan_running: s.scan_running,
                    scan_completed_this_open: s.scan_completed_this_open,
                })
            }
            Input::ScanProof => {
                let p = self.runtime()?.note_scan_proof().ok_or_else(failure)?;
                value(ScanProof {
                    snapshot_seq: p.snapshot_seq.0,
                    snapshot_generation: p.snapshot_generation.0,
                    snapshot_native_file: p.snapshot_native_file,
                    receipt: Receipt {
                        through: p.receipt.through().0,
                        generation: p.snapshot_generation.0,
                        native_file: p.snapshot_native_file,
                    },
                    recovered_names: p.recovered_names,
                })
            }
            Input::Capture(seq, generation) => {
                let p = self
                    .runtime()?
                    .capture_proof(Seq(seq), Generation(generation))
                    .ok_or_else(failure)?;
                value(CaptureProof {
                    observed_seq: p.observed_seq.0,
                    generation: p.generation.0,
                    attempted: p.attempted,
                    persisted: p.persisted,
                })
            }
            Input::Wal(seq) => {
                let r = self.receipt()?;
                if r.through < seq {
                    return Err(failure());
                }
                value(r)
            }
            Input::ArmLoss(id) => {
                self.runtime()?.arm_capture_loss_for_testing(id)?;
                value(())
            }
            Input::Edit(id, fields) => {
                let c = self.host.collection()?;
                let tags = {
                    let lease = c.reader(ReaderRole::Query)?;
                    lease.note(NoteId(id))?.ok_or_else(failure)?.tags
                };
                let result = meguri_engine::notes::update(
                    &c,
                    meguri_engine::notes::UpdateNote {
                        id: NoteId(id),
                        fields,
                        tags: tags.split_whitespace().map(str::to_owned).collect(),
                    },
                    OpSource::UserUndoable,
                )?;
                let note = self
                    .collection_rows()?
                    .into_iter()
                    .find_map(|p| match p {
                        codec::Payload::Note(n) if n.id == id => Some(n),
                        _ => None,
                    })
                    .ok_or_else(failure)?;
                value(Edit {
                    note,
                    seq: result.seq.0,
                    generation: result.generation.0,
                    native_file: runtime::FileIdentityProvider::identity(&devices::NativeFiles, c.path())?,
                })
            }
            Input::Payloads => value(self.payloads()?),
            Input::Entity(n) => value(self.runtime()?.entity_facts(&n)?.ok_or_else(failure)?),
            Input::AllEntities => value(self.all_facts()?),
            Input::Poll(i) => {
                self.runtime()?.poll_review(&i)?;
                value(())
            }
            Input::Trace => value(self.runtime()?.trace()),
            Input::Observations => value(self.runtime()?.observations()),
            Input::Emitted => value(self.runtime()?.emitted_update_ids()),
            Input::Updates(i) => value(self.runtime()?.take_pending_updates(&i)?),
            Input::Transition(i, k, p) => value(self.runtime()?.pending_transition(&i, k, p)?),
            Input::Sendables => value(self.runtime()?.tracking_sendables()),
            Input::Build(i, names) => value(self.runtime()?.build_collection_batch(
                &i,
                &self.host.collection()?,
                &names,
            )?),
            Input::EnterHandler(i, id) => {
                self.runtime()?.enter_sent_handler(&i, id)?;
                value(())
            }
            Input::ReturnHandler(i) => value(self.runtime()?.return_sent_handler(&i)?),
            Input::Sent(i, id, saved, failed, n) => {
                self.runtime()?.sent_results(&i, id, &saved, &failed, n)?;
                value(())
            }
            Input::AppliedUpdate(i, id, names) => {
                self.runtime()?.sdk_update_applied(&i, id, &names)?;
                value(())
            }
            Input::SavedRemoved(i, id, names) => {
                self.runtime()?.sdk_saved_removed(&i, id, &names)?;
                value(())
            }
            Input::Qualify(i) => {
                self.runtime()?.sdk_fetch_completed_without_root(&i)?;
                value(())
            }
            Input::RootRead(i, id, e, tags, lost) => value(self.runtime()?.accept_root_read(&i, id, &e, &tags, lost)?),
            Input::RootFetch(i, e, completed, id) => {
                value(self.runtime()?.accept_root_fetch(&i, e.as_ref(), completed, id)?)
            }
            Input::ReadDue => value(self.runtime()?.root_read_due()?),
            Input::RootEpoch => value(self.runtime()?.accepted_root_epoch()?),
            Input::SyncNow(i) => value(self.runtime()?.sync_now(&i, &self.host.collection()?)?),
            Input::FetchDue => value(self.runtime()?.exchange_fetch_due()),
            Input::FetchCompleted(i, phase, seen) => {
                self.runtime()?.exchange_fetch_completed(&i, phase, &seen)?;
                value(())
            }
            Input::IncomingIntents(b) => value(self.runtime()?.apply_intents(b)?),
            Input::IncomingInbox(b) => value(
                self.runtime()?
                    .inbox_entries(b)?
                    .into_iter()
                    .map(|stored| IncomingInboxEntry {
                        id: stored.id,
                        record_name: stored.entry.record_name,
                        erev: match stored.entry.value {
                            core::InboxValue::Record { erev, .. } => erev,
                            core::InboxValue::Deletion => 0,
                        },
                        reason: stored.entry.reason,
                        retry_count: stored.entry.retry_count,
                    })
                    .collect::<Vec<_>>(),
            ),
            Input::IncomingRefused(b) => value(
                self.runtime()?
                    .refused_entries(b)?
                    .into_iter()
                    .map(|e| IncomingRefusedEntry {
                        record_name: e.record_name,
                        erev: e.erev.unwrap_or(0),
                        version_known: e.erev.is_some(),
                        origin: e.origin,
                        reason: e.reason,
                    })
                    .collect::<Vec<_>>(),
            ),
            Input::BindingEntity(b, name) => value(self.runtime()?.binding_entity(b, &name)?),
            Input::DeliverCollection(records) => {
                self.runtime()?
                    .receive_collection_records(&self.runtime()?.instance()?, &records)?;
                value(())
            }
            Input::DrainIncoming(steps) => {
                if steps == 0 {
                    value(0usize)
                } else {
                    value(self.host.apply_cloud_incoming(IPC_TIMEOUT)?)
                }
            }
            Input::PauseAutomaticWAL(paused) => {
                self.host.collection()?.pause_automatic_wal_flush_for_testing(paused);
                value(())
            }
            Input::Fetched(i, records) => {
                self.host.receive_cloud_collection_records(&i, &records, IPC_TIMEOUT)?;
                value(())
            }
            Input::SendsFinished(i) => {
                self.runtime()?.exchange_sends_finished(&i)?;
                value(())
            }
            Input::WantsSend => value(self.runtime()?.wants_send()),
            Input::StartCall(i) => value(self.runtime()?.start_send_call(&i)?),
            Input::RecordCall(i, id, request) => {
                self.runtime()?.send_call_request(&i, id, request)?;
                value(())
            }
            Input::FinishCall(i, id) => {
                self.runtime()?.finish_send_call(&i, id)?;
                value(())
            }
            Input::Calls => value(self.runtime()?.send_calls()),
            Input::Exchange(id) => value(self.runtime()?.exchange_result(id).ok_or_else(failure)?),
            Input::Begin(note) => {
                let c = self.host.collection()?;
                let (id, deck) = {
                    let lease = c.reader(ReaderRole::Query)?;
                    let mut after = None;
                    let mut found = None;
                    loop {
                        let page = lease.row_page(Table::Cards, after.as_ref())?;
                        if page.is_empty() {
                            break;
                        }
                        for row in page {
                            let id = core::row_payload::int(&row, 0)?;
                            after = Some(RowKey::Id(id));
                            if core::row_payload::int(&row, 1)? == note {
                                found = Some((id, core::row_payload::int(&row, 2)?));
                                break;
                            }
                        }
                        if found.is_some() {
                            break;
                        }
                    }
                    found.ok_or_else(failure)?
                };
                let meguri_engine::session::StartResult::Started { session_id, .. } =
                    self.host.review().start(DeckId(deck), Some(CardId(id)))?
                else {
                    return Err(failure());
                };
                self.session_started.insert(session_id.0, self.clock.now_ms());
                self.runtime()?.poll_review(&self.runtime()?.instance()?)?;
                value(session_id.0)
            }
            Input::Start(s) => {
                let head = self.host.review().head(SessionId(s), 1)?;
                let item = head.items.first().ok_or_else(failure)?;
                self.host.review().render_head(
                    SessionId(s),
                    &[item.render_key],
                    meguri_engine::rendering::Platform::Mac,
                )?;
                value(self.card(item.card_id.0)?)
            }
            Input::Show(s, id) => {
                let head = self.host.review().head(SessionId(s), 1)?;
                let item = head.items.first().filter(|h| h.card_id.0 == id).ok_or_else(failure)?;
                self.host.review().render_head(
                    SessionId(s),
                    &[item.render_key],
                    meguri_engine::rendering::Platform::Mac,
                )?;
                value(())
            }
            Input::Good(s, id) => {
                let head = self.host.review().head(SessionId(s), 1)?;
                let item = head.items.first().filter(|h| h.card_id.0 == id).ok_or_else(failure)?;
                let at = self.clock.now_ms();
                let elapsed = u64::try_from(at - *self.session_started.get(&s).ok_or_else(failure)?)?;
                let answer = self.host.review().answer(
                    SessionId(s),
                    CardId(id),
                    meguri_sched::answer::Rating::Good,
                    item.state_token,
                    meguri_model::time::EpochMillis(at),
                    elapsed,
                )?;
                if !matches!(answer, meguri_engine::session::AnswerOutcome::Accepted { .. }) {
                    return Err(failure());
                }
                self.host.review().application_barrier_for_testing()?;
                let card = self.card(id)?;
                let hold = self.host.collection()?.held_back_run(Some(SessionId(s)))?;
                let op_id = hold.steps.first().ok_or_else(failure)?.id.0;
                value(ReviewAnswer {
                    revlog_id: card.last_review_id,
                    op_id,
                })
            }
            Input::End(s) => {
                self.host.review().end(SessionId(s))?;
                self.host.review().application_barrier_for_testing()?;
                self.runtime()?.poll_review(&self.runtime()?.instance()?)?;
                value(())
            }
            Input::Hold(s) => {
                let c = self.host.collection()?;
                let hold = c.held_back_run(Some(SessionId(s)))?;
                let key = self.runtime()?.status()?.binding.ok_or_else(failure)?.facts.record_key;
                let mut entity_names = BTreeSet::new();
                for row in hold.rows() {
                    let row = row?;
                    if let Some(kind) = core::row_payload::KINDS
                        .into_iter()
                        .find(|k| core::row_payload::table(*k) == Some(row.table))
                    {
                        let k = match row.key {
                            RowKey::Id(id) => codec::id_key(id).to_vec(),
                            RowKey::Text(t) => t,
                            _ => continue,
                        };
                        entity_names.insert(codec::entity_name(&key.0, kind, &k)?);
                    }
                }
                value(ReviewHold {
                    op_ids: hold.steps.iter().map(|s| s.id.0).collect(),
                    entity_names,
                })
            }
            Input::CloudHold(s) => {
                let c = self.host.collection()?;
                let hold = c.cloud_held_run(Some(SessionId(s)))?;
                let key = self.runtime()?.status()?.binding.ok_or_else(failure)?.facts.record_key;
                let mut entity_names = BTreeSet::new();
                for row in hold.rows() {
                    let row = row?;
                    if let Some(kind) = core::row_payload::KINDS
                        .into_iter()
                        .find(|k| core::row_payload::table(*k) == Some(row.table))
                    {
                        let k = match row.key {
                            RowKey::Id(id) => codec::id_key(id).to_vec(),
                            RowKey::Text(t) => t,
                            _ => continue,
                        };
                        entity_names.insert(codec::entity_name(&key.0, kind, &k)?);
                    }
                }
                value(ReviewHold {
                    op_ids: hold.steps.iter().map(|s| s.id.0).collect(),
                    entity_names,
                })
            }
            Input::Card(id) => value(self.card(id)?),
            Input::Revlog(id) => value(self.revlog(id)?),
            Input::Advance(ms) => {
                self.clock.step_ms(ms);
                value(())
            }
            input @ (Input::BackupPath(..)
            | Input::BackupNotices
            | Input::RegisterSDK(..)
            | Input::RootConfirmed
            | Input::PreviewBegin
            | Input::PreviewActive(..)
            | Input::PreviewEpoch(..)
            | Input::PreviewCancel
            | Input::PublicationConfirm(..)
            | Input::AdoptionConfirm(..)
            | Input::AdoptionReply(..)
            | Input::AdoptionLocal(..)
            | Input::PublicationNext
            | Input::PublicationLocal(..)
            | Input::PublicationCancel(..)
            | Input::PublicationTrigger
            | Input::PublicationFacts
            | Input::CollectionRows
            | Input::CollectionHeader
            | Input::BackupNow
            | Input::RestoreBackup(..)
            | Input::BackupRows(..)
            | Input::BackupRetention(..)
            | Input::PutMedia(..)
            | Input::MediaFiles
            | Input::RecoveryState
            | Input::ZonesSeen(..)
            | Input::CollectionDrained
            | Input::DeletionGuard
            | Input::DeletionCheck(..)) => publication_driver::handle(self, input),
            Input::Close => {
                self.host.close()?;
                self.closed = true;
                value(())
            }
        }
    }
}
/// Infrastructure entry only. A normal invocation performs no product check.
/// The parent selects this exact test in a subprocess and supplies a private fd.
pub fn child_entry() -> Result<()> {
    let Ok(fd) = std::env::var("MEGURI_CLOUD_CHILD_FD") else {
        return Ok(());
    };
    let fd: i32 = fd.parse()?;
    if fd < 3 {
        return Err(failure());
    }
    let mut stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_read_timeout(Some(IPC_TIMEOUT))?;
    stream.set_write_timeout(Some(IPC_TIMEOUT))?;
    let Input::Boot(boot) = read_frame(&mut stream)? else {
        return Err(failure());
    };
    let mut device = match Device::new(boot, &stream) {
        Ok(d) => {
            write_frame(&mut stream, &ChildMessage::Reply(Ok(serde_json::Value::Null)))?;
            d
        }
        Err(_) => {
            write_frame(&mut stream, &ChildMessage::Reply(Err("child open failed".to_owned())))?;
            return Err(failure());
        }
    };
    // Each command completes synchronously; EOF/timeouts unwind and close the host.
    for _ in 0..100_000 {
        let input: Input = read_frame(&mut stream)?;
        let close = matches!(input, Input::Close);
        let response =
            handle_transport_command(&mut device, input, &mut stream).map_err(|_| "child operation failed".to_owned());
        // A Close served during a reply wait has already sent its own reply.
        // Do not send a second reply for the abandoned transport command.
        if device.closed && !close {
            return Ok(());
        }
        write_frame(&mut stream, &ChildMessage::Reply(response))?;
        if close {
            return Ok(());
        }
    }
    Err(failure())
}

// Key IPC is driven by the production provider call, never by Boot/preview.
struct ChildKeys(Mutex<UnixStream>);
impl runtime::KeyByteSource for ChildKeys {
    fn draw_key(&self) -> std::result::Result<[u8; 32], runtime::KeySourceError> {
        let mut stream = self.0.lock().map_err(|_| runtime::KeySourceError::Unavailable)?;
        let mut exchange = || -> Result<_> {
            write_frame(&mut stream, &ChildMessage::DrawKey)?;
            match read_frame(&mut stream)? {
                Input::KeyResult(result) => Ok(result),
                _ => Err(failure()),
            }
        };
        exchange().unwrap_or(Err(runtime::KeySourceError::Unavailable))
    }
}
fn transport_enqueue(stream: &mut UnixStream, caller: super::Caller, operation: super::Operation) -> Result<()> {
    write_frame(stream, &ChildMessage::Submit(caller, operation))?;
    match read_frame(stream)? {
        Input::TransportAccepted => Ok(()),
        _ => Err(failure()),
    }
}
fn transport_reply(device: &mut Device, stream: &mut UnixStream) -> Result<super::Response> {
    loop {
        match read_frame(stream)? {
            Input::TransportResult(response) => return Ok(response),
            input => {
                let close = matches!(input, Input::Close);
                let response = device.handle(input).map_err(|_| "child operation failed".to_owned());
                write_frame(stream, &ChildMessage::Reply(response))?;
                if close {
                    return Err(failure());
                }
            }
        }
    }
}
struct ChildTransport<'a> {
    device: &'a mut Device,
    stream: &'a mut UnixStream,
}
impl super::coordinator::PublicationTransport for ChildTransport<'_> {
    type Enqueued = ();
    fn enqueue(&mut self, caller: super::Caller, operation: super::Operation) -> Result<()> {
        transport_enqueue(self.stream, caller, operation)
    }
    fn reply(&mut self, (): ()) -> Result<super::Response> {
        transport_reply(self.device, self.stream)
    }
}
fn handle_transport_command(device: &mut Device, input: Input, stream: &mut UnixStream) -> Result<serde_json::Value> {
    match input {
        Input::PublicationExecute(request) => {
            let runtime = device.host.cloud_runtime().cloned().ok_or_else(failure)?;
            let collection = device.host.collection()?;
            let (result, responses) = super::coordinator::execute_publication_transport(
                &request,
                &runtime,
                &collection,
                &mut ChildTransport { device, stream },
            )?;
            runtime.publication_result(&request, result, &collection)?;
            Ok(serde_json::to_value(responses)?)
        }
        Input::DeleteZone(zone) => {
            let runtime = device.host.cloud_runtime().cloned().ok_or_else(failure)?;
            let collection = device.host.collection()?;
            let guard = device.deletion.clone().ok_or_else(failure)?;
            runtime.submit_zone_deletion(&guard, &collection, || {
                transport_enqueue(
                    stream,
                    super::Caller {
                        own_operation: Some("publication-cleanup".into()),
                        ..Default::default()
                    },
                    super::Operation::DeleteZone {
                        zone: zone.clone(),
                        cause: super::RemovalCause::Deleted,
                    },
                )
            })??;
            let response = transport_reply(device, stream)?;
            let deleted = response.result.is_ok()
                || matches!(&response.result, Err(e) if e.code == super::ErrorCode::ZoneNotFound);
            runtime.zone_deletion_result(&guard, &zone, deleted, &collection)?;
            Ok(serde_json::to_value(response)?)
        }
        input => device.handle(input),
    }
}
#[path = "publication_driver.rs"]
mod publication_driver;
