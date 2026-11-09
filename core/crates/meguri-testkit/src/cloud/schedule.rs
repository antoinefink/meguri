//! Seeded ordinary exchanges over actual PublicationRig/CoreHost children.
//! Publication and adoption are real setup operations, outside the grant stream.
//! This component cannot issue a Q4.4 or whole-Q5 acceptance receipt.
use super::{
    Config, LogOutcome, RequestKind, Service,
    adapter::{FetchScope, OrdinaryAction, OrdinaryReceipt},
    checks::DeviceId,
    coordinator::PublicationRig,
    devices::{IdentityReads, Result, failure},
};
use crate::generate::{
    cloud_small,
    recipe::{Recipe, generate_recipe},
};
use meguri_model::rng::{ChaCha20Random, RandomSource, SeededRandom};
use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::Path,
    sync::Arc,
};

pub const DEFAULT_GRANTS: usize = 10_000;

/// Arguments stay private to the program. Debug reveals only the operation kind.
pub enum Step {
    Edit {
        note: i64,
        fields: Vec<String>,
    },
    StartReview {
        note: i64,
    },
    ShowAnswer,
    AnswerGood,
    EndReview,
    PutMedia {
        name: String,
        bytes: Vec<u8>,
    },
    Sync,
    HoldSent,
    ReleaseSent,
    HoldFetch,
    ReleaseFetch,
    /// These programs require future scheduler components; never silently drain.
    Unsupported(Unsupported),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    Publication,
    Adoption,
    Account,
    Lifecycle,
    Fault,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProgramKind {
    Edit,
    StartReview,
    ShowAnswer,
    AnswerGood,
    EndReview,
    PutMedia,
    Sync,
    HoldSent,
    ReleaseSent,
    HoldFetch,
    ReleaseFetch,
    Unsupported,
}
impl Step {
    fn kind(&self) -> ProgramKind {
        match self {
            Self::Edit { .. } => ProgramKind::Edit,
            Self::StartReview { .. } => ProgramKind::StartReview,
            Self::ShowAnswer => ProgramKind::ShowAnswer,
            Self::AnswerGood => ProgramKind::AnswerGood,
            Self::EndReview => ProgramKind::EndReview,
            Self::PutMedia { .. } => ProgramKind::PutMedia,
            Self::Sync => ProgramKind::Sync,
            Self::HoldSent => ProgramKind::HoldSent,
            Self::ReleaseSent => ProgramKind::ReleaseSent,
            Self::HoldFetch => ProgramKind::HoldFetch,
            Self::ReleaseFetch => ProgramKind::ReleaseFetch,
            Self::Unsupported(_) => ProgramKind::Unsupported,
        }
    }
}
impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.kind(), f)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActionKey {
    Program(ProgramKind),
    Engine(OrdinaryAction),
    Refresh,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trace {
    pub device: DeviceId,
    pub action: ActionKey,
    pub receipt: OrdinaryReceipt,
    pub revision_before: u64,
    pub revision_after: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestReceipt {
    pub id: u64,
    pub device: Option<DeviceId>,
    pub kind: RequestKind,
    pub outcome: LogOutcome,
    pub record_outcomes: Vec<LogOutcome>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blocker {
    ProgramPending,
    EngineWork,
    RetainedCallback,
    PendingEntity,
    InFlightEntity,
    InboxEntries,
    StaleRootDrain,
    StaleCollectionDrain,
    AuthorityClosed,
    RecoveryUnsupported,
    // Missing observations never become zero, an empty list, or a pass.
    OnlineNotObserved,
    GateNotObserved,
    CompleteHeldSetNotObserved,
    PeerResolvableInboxNotObserved,
    ServeableMediaNotObserved,
    DueOwnOperationsNotObserved,
    FactsUnavailable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrainFacts {
    pub device: DeviceId,
    pub root_read_revision: Option<u64>,
    pub collection_read_revision: Option<u64>,
    pub blockers: Vec<Blocker>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DrainOutcome {
    NotProved { grants: usize, facts: Vec<DrainFacts> },
    NoQuiescence { grants: usize, facts: Vec<DrainFacts> },
}
#[derive(Debug)]
pub struct UnsupportedProgram(pub Unsupported);
impl std::fmt::Display for UnsupportedProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported seeded program: {:?}", self.0)
    }
}
impl std::error::Error for UnsupportedProgram {}
struct Keys(ChaCha20Random);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> std::result::Result<[u8; 32], KeySourceError> {
        let mut bytes = [0; 32];
        for part in bytes.chunks_exact_mut(8) {
            part.copy_from_slice(&self.0.next_u64().to_le_bytes());
        }
        Ok(bytes)
    }
}
struct Member {
    rig: PublicationRig,
    program: VecDeque<Step>,
    hold_sent: bool,
    hold_fetch: bool,
    review: Option<(u64, i64)>,
    read: [Option<u64>; 2],
    drained: [Option<u64>; 2],
}
impl Member {
    fn fresh(&self, revision: u64) -> bool {
        self.drained == [Some(revision); 2]
    }
}
/// The Service and member rigs are private so changes cannot bypass revision
/// accounting. Scheduler choices use one pinned ChaCha20 stream through modelrng.
/// Legacy Service tags/page permutations and child CoreHost randomness still use
/// SeededRandom; this is explicitly not full Q3.6 RNG migration or lane acceptance.
pub struct Runner {
    service: Service<SeededRandom>,
    members: BTreeMap<DeviceId, Member>,
    random: ChaCha20Random,
    revision: u64,
    trace: Vec<Trace>,
    request_base: u64,
}
impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SeededOrdinaryRunner")
    }
}
impl Runner {
    /// Q4.2's generated small base, real publication and real adoption. Stable
    /// ordinary setup only; no prebound records, expected snapshots or ACK seeds.
    pub fn setup(directory: &Path, devices: &[DeviceId], seed: u64, config: Config) -> Result<Self> {
        let roster: BTreeSet<_> = devices.iter().copied().collect();
        if roster.len() != devices.len()
            || !(2..=4).contains(&roster.len())
            || config.page_size == 0
            || config.page_size > 32
        {
            return Err(failure());
        }
        let random = ChaCha20Random::new(seed);
        let scope = super::Scope {
            container: "test.meguri.seeded-ordinary".into(),
            environment: "development".into(),
            account: "synthetic-seeded".into(),
        };
        let mut service = Service::open(directory, config, SeededRandom::new(random.next_u64()))?;
        let mut members = BTreeMap::new();
        for (ordinal, device) in roster.into_iter().enumerate() {
            let path = directory.join(format!("device-{ordinal}"));
            let fixture_seed = random.next_u64();
            if ordinal == 0 {
                cloud_small::generate(&cloud_small::Request::new(fixture_seed), &path)?;
            } else {
                let recipe = Recipe::parse(
                    "recipe: seeded-adopter\ndescription: Synthetic adoption destination\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Synthetic\n    notetype: basic\n    new: 1\n    fields: [\"before adoption {n}\", \"answer {n}\"]\n",
                )?;
                generate_recipe(&recipe, fixture_seed, &path)?;
            }
            let mut rig = PublicationRig::open(
                &path,
                &scope,
                IdentityReads::Available(scope.account.clone()),
                false,
                cloud_small::NOW_MS,
                random.next_u64(),
                Arc::new(Keys(ChaCha20Random::new(random.next_u64()))),
            )?;
            let preview = rig.preview(&mut service, DEFAULT_GRANTS)?;
            if ordinal == 0 {
                rig.confirm_publication(&preview)?;
            } else {
                rig.adopt_cloud(&preview)?;
            }
            rig.drive_ready(&mut service, DEFAULT_GRANTS)?;
            rig.sync_now_and_drain(&mut service, DEFAULT_GRANTS)?;
            if !rig.facts()?.recovery_ops.is_empty() {
                return Err(failure());
            }
            members.insert(
                device,
                Member {
                    rig,
                    program: VecDeque::new(),
                    hold_sent: false,
                    hold_fetch: false,
                    review: None,
                    read: [None; 2],
                    drained: [None; 2],
                },
            );
        }
        let request_base = service.request_log()?.last().map_or(0, |r| r.request_id);
        Ok(Self {
            service,
            members,
            random,
            revision: 0,
            trace: Vec::new(),
            request_base,
        })
    }
    pub fn enqueue(&mut self, device: DeviceId, step: Step) -> Result<()> {
        if let Step::Unsupported(kind) = &step {
            return Err(Box::new(UnsupportedProgram(*kind)));
        }
        self.members
            .get_mut(&device)
            .ok_or_else(failure)?
            .program
            .push_back(step);
        Ok(())
    }
    pub fn trace(&self) -> &[Trace] {
        &self.trace
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn candidates(&self, refresh: bool) -> Vec<(DeviceId, ActionKey)> {
        let mut ready = BTreeSet::new();
        for (&id, m) in &self.members {
            if let Some(step) = m.program.front() {
                if !matches!(step, Step::Sync) || m.rig.ordinary_idle() {
                    ready.insert((id, ActionKey::Program(step.kind())));
                }
            }
            if let Some(action) = m.rig.ordinary_action() {
                if !(m.hold_sent && matches!(action, OrdinaryAction::Sent | OrdinaryAction::ReturnSent))
                    && !(m.hold_fetch
                        && matches!(action, OrdinaryAction::Fetched)
                        && m.rig.ordinary_callback_scope() == Some(FetchScope::Collection))
                {
                    ready.insert((id, ActionKey::Engine(action)));
                }
            } else if refresh && m.program.is_empty() && !m.fresh(self.revision) {
                ready.insert((id, ActionKey::Refresh));
            }
        }
        ready.into_iter().collect()
    }
    /// Exactly one granted item. Returns None when all implemented work is
    /// blocked/idle. Child transport callbacks retain original command ownership.
    pub fn grant(&mut self) -> Result<Option<Trace>> {
        self.grant_inner(false)
    }
    fn grant_inner(&mut self, refresh: bool) -> Result<Option<Trace>> {
        let ready = self.candidates(refresh);
        if ready.is_empty() {
            return Ok(None);
        }
        let index = self.choose(ready.len());
        let (device, action) = ready[index];
        let before = self.revision;
        let m = self.members.get_mut(&device).ok_or_else(failure)?;
        let receipt = match action {
            ActionKey::Refresh => {
                m.rig.begin_ordinary()?;
                OrdinaryReceipt::default()
            }
            ActionKey::Engine(_) => m.rig.ordinary_step(&mut self.service)?,
            ActionKey::Program(_) => {
                let step = m.program.pop_front().ok_or_else(failure)?;
                let mut receipt = OrdinaryReceipt::default();
                match step {
                    Step::Edit { note, fields } => {
                        let fields: Vec<_> = fields.iter().map(String::as_str).collect();
                        m.rig.driver().edit_note_fields(note, &fields)?;
                        receipt.changed = true;
                    }
                    Step::StartReview { note } => {
                        if m.review.is_some() {
                            return Err(failure());
                        }
                        let session = m.rig.driver().review_begin(note)?;
                        let card = m.rig.driver().review_start(session)?;
                        m.review = Some((session, card.id));
                    }
                    Step::ShowAnswer => {
                        let (s, c) = m.review.ok_or_else(failure)?;
                        m.rig.driver().review_show_answer(s, c)?;
                    }
                    Step::AnswerGood => {
                        let (s, c) = m.review.ok_or_else(failure)?;
                        m.rig.driver().review_answer_good(s, c)?;
                        receipt.changed = true;
                    }
                    Step::EndReview => {
                        let (s, _) = m.review.ok_or_else(failure)?;
                        m.rig.driver().review_end_and_drain(s)?;
                        m.review = None;
                    }
                    Step::PutMedia { name, bytes } => {
                        m.rig.driver().put_media(&name, &bytes)?;
                        receipt.changed = true;
                    }
                    Step::Sync => m.rig.begin_ordinary()?,
                    Step::HoldSent => m.hold_sent = true,
                    Step::ReleaseSent => m.hold_sent = false,
                    Step::HoldFetch => m.hold_fetch = true,
                    Step::ReleaseFetch => m.hold_fetch = false,
                    Step::Unsupported(kind) => return Err(Box::new(UnsupportedProgram(kind))),
                }
                receipt
            }
        };
        if let Some(scope) = receipt.read_started {
            m.read[scope_index(scope)] = Some(before);
        }
        if receipt.changed {
            self.revision = self.revision.checked_add(1).ok_or_else(failure)?;
        }
        if let Some(scope) = receipt.drained {
            m.drained[scope_index(scope)] = m.read[scope_index(scope)];
        }
        let trace = Trace {
            device,
            action,
            receipt,
            revision_before: before,
            revision_after: self.revision,
        };
        self.trace.push(trace.clone());
        Ok(Some(trace))
    }
    fn choose(&self, count: usize) -> usize {
        // Rejection sampling avoids architecture-dependent or modulo-biased
        // choices. All runtime random draws remain in the model seam.
        let count = count as u64;
        let threshold = count.wrapping_neg() % count;
        loop {
            let draw = self.random.next_u64();
            if draw >= threshold {
                return (draw % count) as usize;
            }
        }
    }
    /// Real Service log projection: request IDs + kinds, no names or settings.
    pub fn request_order(&self) -> Result<Vec<RequestReceipt>> {
        let mut out = Vec::new();
        let mut after = self.request_base;
        loop {
            let page = self.service.request_log_page(after, 2048)?;
            if page.is_empty() {
                return Ok(out);
            }
            for r in page {
                after = r.request_id;
                let device = self
                    .trace
                    .iter()
                    .find(|t| t.receipt.request == Some(r.request_id))
                    .map(|t| t.device);
                out.push(RequestReceipt {
                    id: r.request_id,
                    device,
                    kind: r.kind,
                    outcome: r.outcome,
                    record_outcomes: r.records.into_iter().map(|r| r.outcome).collect(),
                });
            }
        }
    }
    pub fn drain_facts(&mut self) -> Vec<DrainFacts> {
        self.members
            .iter_mut()
            .map(|(&device, m)| {
                let mut blockers = vec![
                    Blocker::OnlineNotObserved,
                    Blocker::GateNotObserved,
                    Blocker::CompleteHeldSetNotObserved,
                    Blocker::PeerResolvableInboxNotObserved,
                    Blocker::ServeableMediaNotObserved,
                    Blocker::DueOwnOperationsNotObserved,
                ];
                if !m.program.is_empty() {
                    blockers.push(Blocker::ProgramPending);
                }
                if !m.rig.ordinary_idle() {
                    blockers.push(Blocker::EngineWork);
                }
                if m.rig.ordinary_has_callback() {
                    blockers.push(Blocker::RetainedCallback);
                }
                if m.drained[0] != Some(self.revision) {
                    blockers.push(Blocker::StaleRootDrain);
                }
                if m.drained[1] != Some(self.revision) {
                    blockers.push(Blocker::StaleCollectionDrain);
                }
                match m.rig.authority_status() {
                    Ok((true, true)) => (),
                    Ok(_) => blockers.push(Blocker::AuthorityClosed),
                    Err(_) => blockers.push(Blocker::FactsUnavailable),
                }
                match m.rig.facts() {
                    Ok(f) if f.recovery_ops.is_empty() => (),
                    Ok(_) => blockers.push(Blocker::RecoveryUnsupported),
                    Err(_) => blockers.push(Blocker::FactsUnavailable),
                }
                match m.rig.ordinary_counts() {
                    Ok((pending, flights, inbox)) => {
                        if pending > 0 {
                            blockers.push(Blocker::PendingEntity);
                        }
                        if flights > 0 {
                            blockers.push(Blocker::InFlightEntity);
                        }
                        if inbox > 0 {
                            blockers.push(Blocker::InboxEntries);
                        }
                    }
                    Err(_) => blockers.push(Blocker::FactsUnavailable),
                }
                DrainFacts {
                    device,
                    root_read_revision: m.drained[0],
                    collection_read_revision: m.drained[1],
                    blockers,
                }
            })
            .collect()
    }
    pub fn drain(&mut self) -> Result<DrainOutcome> {
        self.drain_with_limit(DEFAULT_GRANTS)
    }
    pub fn drain_with_limit(&mut self, limit: usize) -> Result<DrainOutcome> {
        for grants in 0..limit {
            let facts = self.drain_facts();
            if self.candidates(true).is_empty() {
                return Ok(DrainOutcome::NotProved { grants, facts });
            }
            self.grant_inner(true)?;
        }
        Ok(DrainOutcome::NoQuiescence {
            grants: limit,
            facts: self.drain_facts(),
        })
    }
    pub fn collect_components(&mut self, run: &mut super::checks::CheckRun) -> super::checks::ComponentReport {
        let mut children = Vec::with_capacity(self.members.len());
        for (device, member) in &mut self.members {
            children.push((*device, member.rig.driver()));
        }
        run.at_quiescence(&mut self.service, children.as_mut_slice())
    }
    pub fn media_files(&mut self, device: DeviceId) -> Result<BTreeMap<String, Vec<u8>>> {
        self.members
            .get_mut(&device)
            .ok_or_else(failure)?
            .rig
            .driver()
            .media_files()
    }
    /// IDs/payloads are for fixture programming/assertions only, never traces.
    pub fn collection_rows(&mut self, device: DeviceId) -> Result<Vec<meguri_sync::cloud::entity_codec::Payload>> {
        self.members
            .get_mut(&device)
            .ok_or_else(failure)?
            .rig
            .driver()
            .collection_rows()
    }
    pub fn close(mut self) -> Result<()> {
        for m in self.members.values_mut() {
            m.rig.close_clean()?;
        }
        Ok(())
    }
}
fn scope_index(scope: FetchScope) -> usize {
    match scope {
        FetchScope::Root => 0,
        FetchScope::Collection => 1,
    }
}
