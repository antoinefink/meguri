//! One bounded IPC driver shared by the cloud integration targets.
use super::{
    Scope,
    devices::{self, IdentityReads, PreboundDevice, Result, failure},
};
use meguri_engine::{Generation, ReaderRole, Seq, SessionId, WalCoverage};
use meguri_model::{
    changes::OpSource,
    clock::{Clock, StepClock, UtcOffset},
    ids::{CardId, DeckId, NoteId},
    rng::SeededRandom,
};
use meguri_storage::{ReadRows, RowKey, Table};
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
#[derive(Clone, Debug)]
pub struct Killed {
    pub signal: Option<i32>,
    pub clean_close: bool,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Boot {
    directory: std::path::PathBuf,
    scope: Scope,
    note_id: i64,
    binding: core::BindingId,
    identity: IdentityReads,
    automatic: bool,
    now: i64,
    seed: u64,
}
#[derive(Serialize, Deserialize)]
pub(crate) enum Input {
    Boot(Boot),
    Instance,
    Identity(Option<String>),
    Reconcile,
    StartSDK,
    Facts,
    ScanState,
    ScanProof,
    Capture(u64, u64),
    Wal(u64),
    ArmLoss(i64),
    Edit(i64, Vec<String>),
    Payloads,
    Entity(String),
    AllEntities,
    Poll,
    Trace,
    Observations,
    Emitted,
    Updates,
    Transition(UpdateKind, PendingSnapshot),
    Sendables,
    Build(BTreeSet<String>),
    EnterHandler(u64),
    ReturnHandler,
    Sent(u64, Vec<Saved>, Vec<(String, SendError)>, usize),
    AppliedUpdate(u64, Vec<String>),
    SavedRemoved(u64, Vec<String>),
    Qualify(String),
    RootRead(String, u64, Vec<u8>, Vec<u8>, bool),
    RootFetch(String, Option<Saved>, bool, u64),
    ReadDue,
    RootEpoch,
    SyncNow,
    FetchDue,
    FetchCompleted(FetchPhase, BTreeSet<String>),
    Fetched(Vec<Saved>),
    SendsFinished,
    WantsSend,
    StartCall,
    RecordCall(u64, u64),
    FinishCall(u64),
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
}
impl ChildDriver {
    pub(crate) fn spawn(
        files: &PreboundDevice,
        identity: IdentityReads,
        automatic: bool,
        now: i64,
        seed: u64,
    ) -> Result<Self> {
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
        };
        driver.call::<()>(Input::Boot(Boot {
            directory: files.directory.clone(),
            scope: files.scope.clone(),
            note_id: files.note.id,
            binding: files.binding.id,
            identity,
            automatic,
            now,
            seed,
        }))?;
        Ok(driver)
    }
    pub(crate) fn call<T: DeserializeOwned>(&mut self, input: Input) -> Result<T> {
        write_frame(&mut self.stream, &input)?;
        let result: std::result::Result<serde_json::Value, String> = read_frame(&mut self.stream)?;
        Ok(serde_json::from_value(result.map_err(|_| failure())?)?)
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
impl IdentityProvider for Account {
    fn read_identity(&self, _: &core::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
struct Device {
    host: meguri_host::CoreHost,
    account: Arc<Account>,
    clock: Arc<StepClock>,
    binding: core::BindingId,
    note: i64,
    session_started: BTreeMap<u64, i64>,
}
impl Device {
    fn new(b: Boot) -> Result<Self> {
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
        let host = meguri_host::CoreHost::open(options)?;
        host.cloud_runtime().ok_or_else(failure)?.set_automatic(b.automatic);
        Ok(Self {
            host,
            account,
            clock,
            binding: b.binding,
            note: b.note_id,
            session_started: BTreeMap::new(),
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
        let name = codec::entity_name(
            &binding.facts.record_key.0,
            codec::Kind::Note,
            &codec::id_key(self.note),
        )?;
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
                binding: self.binding,
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
            Input::Boot(_) => Err(failure()),
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
                self.runtime()?
                    .reconcile_binding(&c, &self.host.media()?, IPC_TIMEOUT)?;
                value(())
            }
            Input::StartSDK => {
                self.runtime()?.start_sdk_instance()?;
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
                    .payloads()?
                    .into_values()
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
            Input::Poll => {
                self.runtime()?.poll_review()?;
                value(())
            }
            Input::Trace => value(self.runtime()?.trace()),
            Input::Observations => value(self.runtime()?.observations()),
            Input::Emitted => value(self.runtime()?.emitted_update_ids()),
            Input::Updates => value(self.runtime()?.take_pending_updates()),
            Input::Transition(k, p) => value(self.runtime()?.pending_transition(k, p)),
            Input::Sendables => value(self.runtime()?.tracking_sendables()),
            Input::Build(names) => value(
                self.runtime()?
                    .build_collection_batch(&self.host.collection()?, &names)?,
            ),
            Input::EnterHandler(id) => {
                self.runtime()?.enter_sent_handler(id)?;
                value(())
            }
            Input::ReturnHandler => value(self.runtime()?.return_sent_handler()?),
            Input::Sent(id, saved, failed, n) => {
                self.runtime()?.sent_results(id, &saved, &failed, n)?;
                value(())
            }
            Input::AppliedUpdate(id, names) => {
                self.runtime()?.sdk_update_applied(id, &names)?;
                value(())
            }
            Input::SavedRemoved(id, names) => {
                self.runtime()?.sdk_saved_removed(id, &names)?;
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
            Input::SyncNow => value(self.runtime()?.sync_now(&self.host.collection()?)?),
            Input::FetchDue => value(self.runtime()?.exchange_fetch_due()),
            Input::FetchCompleted(phase, seen) => {
                self.runtime()?.exchange_fetch_completed(phase, &seen)?;
                value(())
            }
            Input::Fetched(records) => {
                self.runtime()?.accept_fetched_baseline(&records)?;
                value(())
            }
            Input::SendsFinished => {
                self.runtime()?.exchange_sends_finished()?;
                value(())
            }
            Input::WantsSend => value(self.runtime()?.wants_send()),
            Input::StartCall => value(self.runtime()?.start_send_call()?),
            Input::RecordCall(id, request) => {
                self.runtime()?.send_call_request(id, request)?;
                value(())
            }
            Input::FinishCall(id) => {
                self.runtime()?.finish_send_call(id)?;
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
                self.runtime()?.poll_review()?;
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
                self.runtime()?.poll_review()?;
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
            Input::Card(id) => value(self.card(id)?),
            Input::Revlog(id) => value(self.revlog(id)?),
            Input::Advance(ms) => {
                self.clock.step_ms(ms);
                value(())
            }
            Input::Close => {
                self.host.close()?;
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
    let mut device = match Device::new(boot) {
        Ok(d) => {
            write_frame(&mut stream, &Ok::<_, String>(serde_json::Value::Null))?;
            d
        }
        Err(_) => {
            write_frame(
                &mut stream,
                &Err::<serde_json::Value, _>("child open failed".to_owned()),
            )?;
            return Err(failure());
        }
    };
    // Each command completes synchronously; EOF/timeouts unwind and close the host.
    for _ in 0..100_000 {
        let input: Input = read_frame(&mut stream)?;
        let close = matches!(input, Input::Close);
        let response = device.handle(input).map_err(|_| "child operation failed".to_owned());
        write_frame(&mut stream, &response)?;
        if close {
            return Ok(());
        }
    }
    Err(failure())
}
