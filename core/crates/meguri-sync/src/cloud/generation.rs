//! Durable local K19.2 ledger facts, distinct from `generation_codec`'s wire
//! messages. No SDK claims, inbox intake, member apply or rollback runs here.
//! Caller-supplied operation identities/maps describe committed collection ops.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::Error;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GenerationId(pub [u8; 16]);
impl std::fmt::Debug for GenerationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GenerationId")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotetypeKind {
    Standard,
    Cloze,
}

/// Template counts describe the notetype, not a cloze card's ordinal range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutShape {
    pub kind: NotetypeKind,
    pub fields: u32,
    pub templates: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrdinalMap {
    pub fields: Vec<i32>,
    pub templates: Vec<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutSeed {
    pub notetype_id: i64,
    pub local: u64,
    pub acked: u64,
    pub shape: LayoutShape,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutEdit {
    pub notetype_id: i64,
    pub anchor: String,
    pub from_struct_gen: u64,
    pub before: LayoutShape,
    pub after: LayoutShape,
    pub map: OrdinalMap,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeEdit {
    pub old_notetype_id: i64,
    pub old_struct_gen: u64,
    pub new_notetype_id: i64,
    pub new_struct_gen: u64,
    pub old: LayoutShape,
    pub new: LayoutShape,
    pub map: OrdinalMap,
}

/// Changes retain their sequence within the collection operation, so a type
/// move can be mapped through the layout edits that precede or follow it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Change {
    Layout(LayoutEdit),
    Type(TypeEdit),
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationMember {
    pub record_name: String,
    pub erev: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalOperation {
    pub id: GenerationId,
    pub reasons: Vec<u32>,
    pub members: Vec<GenerationMember>,
    pub changes: Vec<Change>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    Local,
}

/// Intake and apply will track remote generations separately. This ledger can
/// read final local states from storage; its APIs do not perform those applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocalState {
    Local,
    Claiming,
    Claimed,
    Complete,
    Lost,
    RolledBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinedLayout {
    pub notetype_id: i64,
    pub from_struct_gen: u64,
    pub to_struct_gen: u64,
    pub old: LayoutShape,
    pub new: LayoutShape,
    pub map: OrdinalMap,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGeneration {
    pub id: GenerationId,
    pub origin: Origin,
    pub state: LocalState,
    pub order: u64,
    pub last_order: u64,
    pub waits_for: Vec<GenerationId>,
    pub anchors: Vec<String>,
    pub members: Vec<GenerationMember>,
    pub reasons: Vec<u32>,
    pub maps: Vec<JoinedLayout>,
    pub type_changes: Vec<TypeEdit>,
    pub(super) operations: Vec<OrderedOperation>,
    pub(super) active_claim: Option<u64>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationLedger {
    pub last_commit: u64,
    pub last_claim: u64,
    pub layouts: BTreeMap<i64, LayoutSeed>,
    pub generations: Vec<LocalGeneration>,
    pub(super) seeds: BTreeMap<i64, LayoutSeed>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OrderedOperation {
    order: u64,
    facts: LocalOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClaimTicket {
    pub generation: GenerationId,
    pub attempt: u64,
}

/// Only no-manifest retry/loss decisions are supported. Landed claims need
/// manifest system fields and member/anchor acknowledgement wiring elsewhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimResult {
    Retry,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorVersion {
    AssetPending,
    Live { struct_gen: u64 },
    Tombstone,
}

/// Validated semantic observations supplied by the caller, not raw inbox rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnchorFact {
    pub notetype_id: i64,
    pub version: AnchorVersion,
}

macro_rules! opaque_debug {
    ($($t:ty),+ $(,)?) => {$(impl std::fmt::Debug for $t {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(stringify!($t)) }
    })+};
}
opaque_debug!(
    LayoutEdit,
    GenerationMember,
    LocalOperation,
    LocalGeneration,
    GenerationLedger,
    OrderedOperation
);

pub(super) fn checked(value: u64) -> Result<u64, Error> {
    i64::try_from(value).map(|_| value).map_err(|_| Error::InvalidData)
}
fn increment(value: u64) -> Result<u64, Error> {
    checked(value.checked_add(1).ok_or(Error::InvalidData)?)
}
fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 27
        && b[0] == b'e'
        && b[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(b))
        && b"aeimquy4".contains(&b[26])
}
fn validate_shape(shape: LayoutShape) -> Result<(), Error> {
    if shape.fields == 0
        || shape.fields > i32::MAX as u32
        || shape.templates == 0
        || shape.templates > i32::MAX as u32
        || (shape.kind == NotetypeKind::Cloze && shape.templates != 1)
    {
        return Err(Error::InvalidData);
    }
    Ok(())
}
fn prefix(old: LayoutShape, new: LayoutShape) -> bool {
    old.kind == NotetypeKind::Cloze || new.kind == NotetypeKind::Cloze
}
fn validate_map(map: &OrdinalMap, old: LayoutShape, new: LayoutShape) -> Result<(), Error> {
    validate_shape(old)?;
    validate_shape(new)?;
    if map.fields.len() != usize::try_from(new.fields).map_err(|_| Error::InvalidData)?
        || map.fields.iter().any(|&v| v < -1 || (v >= 0 && v as u32 >= old.fields))
        || (!prefix(old, new)
            && map.templates.len() != usize::try_from(new.templates).map_err(|_| Error::InvalidData)?)
        || (map.templates.is_empty() && old.kind != NotetypeKind::Cloze)
        || map
            .templates
            .iter()
            .any(|&v| v < -1 || (v >= 0 && old.kind == NotetypeKind::Standard && v as u32 >= old.templates))
    {
        return Err(Error::InvalidData);
    }
    Ok(())
}
fn template_at(map: &[i32], old: LayoutShape, new: LayoutShape, i: usize) -> Result<i32, Error> {
    if new.kind == NotetypeKind::Standard && i >= usize::try_from(new.templates).map_err(|_| Error::InvalidData)? {
        return Ok(-1);
    }
    match map.get(i) {
        Some(&v) => Ok(v),
        None if prefix(old, new) && old.kind == NotetypeKind::Cloze => i32::try_from(i).map_err(|_| Error::InvalidData),
        None => Ok(-1),
    }
}

/// K19.2 old->middle->new, reading a cloze prefix's omitted tail using its
/// explicit old kind. This composes layout maps before/after a type change;
/// composition of two successive kind-changing type changes is unsupported.
pub fn compose_maps(
    before: &OrdinalMap,
    old: LayoutShape,
    middle: LayoutShape,
    after: &OrdinalMap,
    new: LayoutShape,
) -> Result<OrdinalMap, Error> {
    validate_map(before, old, middle)?;
    validate_map(after, middle, new)?;
    if old.kind != middle.kind && middle.kind != new.kind {
        return Err(Error::InvalidData);
    }
    let fields = after
        .fields
        .iter()
        .map(|&v| {
            if v == -1 {
                Ok(-1)
            } else {
                before
                    .fields
                    .get(usize::try_from(v).map_err(|_| Error::InvalidData)?)
                    .copied()
                    .ok_or(Error::InvalidData)
            }
        })
        .collect::<Result<_, _>>()?;
    let len = if prefix(old, middle) || prefix(middle, new) {
        before.templates.len().max(after.templates.len())
    } else {
        after.templates.len()
    };
    let templates = (0..len)
        .map(|i| {
            let v = template_at(&after.templates, middle, new, i)?;
            if v == -1 {
                Ok(-1)
            } else {
                template_at(
                    &before.templates,
                    old,
                    middle,
                    usize::try_from(v).map_err(|_| Error::InvalidData)?,
                )
            }
        })
        .collect::<Result<_, _>>()?;
    let result = OrdinalMap { fields, templates };
    validate_map(&result, old, new)?;
    Ok(result)
}

impl LocalOperation {
    fn canonicalize(&mut self) -> Result<(), Error> {
        self.reasons.sort_unstable();
        self.reasons.dedup();
        if self.reasons.iter().any(|r| !(1..=10).contains(r)) || (self.reasons.is_empty() && self.changes.is_empty()) {
            return Err(Error::InvalidData);
        }
        self.members.sort_by(|a, b| a.record_name.cmp(&b.record_name));
        for member in &self.members {
            if !valid_name(&member.record_name) {
                return Err(Error::InvalidData);
            }
            checked(member.erev)?;
        }
        if self.members.windows(2).any(|p| p[0].record_name == p[1].record_name) {
            return Err(Error::InvalidData);
        }
        let mut layouts = BTreeSet::new();
        for change in &self.changes {
            match change {
                Change::Layout(edit) => {
                    if !valid_name(&edit.anchor)
                        || !layouts.insert(edit.notetype_id)
                        || !self.members.iter().any(|m| m.record_name == edit.anchor)
                    {
                        return Err(Error::InvalidData);
                    }
                    checked(edit.from_struct_gen)?;
                    validate_map(&edit.map, edit.before, edit.after)?;
                }
                Change::Type(edit) => {
                    if edit.old_notetype_id == edit.new_notetype_id {
                        return Err(Error::InvalidData);
                    }
                    checked(edit.old_struct_gen)?;
                    checked(edit.new_struct_gen)?;
                    validate_map(&edit.map, edit.old, edit.new)?;
                    // Initial operation facts follow K19.2's no-template-map
                    // reporting rule. Joined maps may have a longer prefix.
                    if prefix(edit.old, edit.new) {
                        let required = if edit.old.kind == NotetypeKind::Cloze {
                            0
                        } else {
                            usize::try_from(edit.old.templates).map_err(|_| Error::InvalidData)?
                        };
                        // Reject a short supplied map before inspecting its
                        // identity entries; never allocate from declared counts.
                        if edit.map.templates.len() != required
                            || edit
                                .map
                                .templates
                                .iter()
                                .enumerate()
                                .any(|(i, &v)| i32::try_from(i).ok() != Some(v))
                        {
                            return Err(Error::InvalidData);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// Deterministically replay a committed op, with checked local layout increments.
/// The result records the op's exact before/after versions for later joins.
fn resolve(
    operation: &LocalOperation,
    layouts: &mut BTreeMap<i64, LayoutSeed>,
    names: &mut BTreeMap<i64, String>,
) -> Result<Vec<ResolvedChange>, Error> {
    let mut result = Vec::new();
    for change in &operation.changes {
        match change {
            Change::Layout(edit) => {
                let layout = layouts.get_mut(&edit.notetype_id).ok_or(Error::InvalidData)?;
                if layout.local != edit.from_struct_gen || layout.shape != edit.before {
                    return Err(Error::InvalidData);
                }
                if names.get(&edit.notetype_id).is_some_and(|n| n != &edit.anchor)
                    || names.iter().any(|(id, n)| *id != edit.notetype_id && n == &edit.anchor)
                {
                    return Err(Error::InvalidData);
                }
                names.insert(edit.notetype_id, edit.anchor.clone());
                let next = increment(layout.local.max(layout.acked))?;
                result.push(ResolvedChange::Layout(JoinedLayout {
                    notetype_id: edit.notetype_id,
                    from_struct_gen: layout.local,
                    to_struct_gen: next,
                    old: edit.before,
                    new: edit.after,
                    map: edit.map.clone(),
                }));
                layout.local = next;
                layout.shape = edit.after;
            }
            Change::Type(edit) => {
                let old = layouts.get(&edit.old_notetype_id).ok_or(Error::InvalidData)?;
                let new = layouts.get(&edit.new_notetype_id).ok_or(Error::InvalidData)?;
                if old.local != edit.old_struct_gen
                    || new.local != edit.new_struct_gen
                    || old.shape != edit.old
                    || new.shape != edit.new
                {
                    return Err(Error::InvalidData);
                }
                result.push(ResolvedChange::Type(edit.clone()));
            }
        }
    }
    Ok(result)
}

enum ResolvedChange {
    Layout(JoinedLayout),
    Type(TypeEdit),
}
type Replay = (BTreeMap<i64, LayoutSeed>, BTreeMap<u64, Vec<ResolvedChange>>);

impl LocalGeneration {
    fn empty(op: OrderedOperation) -> Self {
        Self {
            id: op.facts.id,
            origin: Origin::Local,
            state: LocalState::Local,
            order: op.order,
            last_order: op.order,
            waits_for: Vec::new(),
            anchors: Vec::new(),
            members: Vec::new(),
            reasons: Vec::new(),
            maps: Vec::new(),
            type_changes: Vec::new(),
            operations: vec![op],
            active_claim: None,
        }
    }
    fn joinable(&self) -> bool {
        matches!(self.state, LocalState::Local | LocalState::Lost)
    }
    fn touches(&self, other: &Self) -> bool {
        self.members
            .iter()
            .any(|m| other.members.iter().any(|n| m.record_name == n.record_name))
            || self.anchors.iter().any(|a| other.anchors.contains(a))
            || self.maps.iter().any(|m| {
                other
                    .type_changes
                    .iter()
                    .any(|t| m.notetype_id == t.old_notetype_id || m.notetype_id == t.new_notetype_id)
            })
            || other.maps.iter().any(|m| {
                self.type_changes
                    .iter()
                    .any(|t| m.notetype_id == t.old_notetype_id || m.notetype_id == t.new_notetype_id)
            })
    }
    fn rebuild(&mut self, resolved: &BTreeMap<u64, Vec<ResolvedChange>>) -> Result<(), Error> {
        self.operations.sort_by_key(|op| op.order);
        let first = self.operations.first().ok_or(Error::InvalidData)?;
        self.id = first.facts.id;
        self.order = first.order;
        self.last_order = self.operations.last().ok_or(Error::InvalidData)?.order;
        let mut anchors = BTreeSet::new();
        let mut members = BTreeMap::new();
        let mut reasons = BTreeSet::new();
        let mut maps: BTreeMap<i64, JoinedLayout> = BTreeMap::new();
        let mut types: BTreeMap<(i64, i64), TypeEdit> = BTreeMap::new();
        for op in &self.operations {
            reasons.extend(op.facts.reasons.iter().copied());
            for member in &op.facts.members {
                if members.get(&member.record_name).is_some_and(|&v| v > member.erev) {
                    return Err(Error::InvalidData);
                }
                members.insert(member.record_name.clone(), member.erev);
            }
            for change in &op.facts.changes {
                if let Change::Layout(edit) = change {
                    anchors.insert(edit.anchor.clone());
                }
            }
            for change in resolved.get(&op.order).ok_or(Error::InvalidData)? {
                match change {
                    ResolvedChange::Layout(edit) => {
                        for t in types.values_mut().filter(|t| t.new_notetype_id == edit.notetype_id) {
                            if t.new_struct_gen != edit.from_struct_gen || t.new != edit.old {
                                return Err(Error::InvalidData);
                            }
                            t.map = compose_maps(&t.map, t.old, t.new, &edit.map, edit.new)?;
                            t.new_struct_gen = edit.to_struct_gen;
                            t.new = edit.new;
                        }
                        if let Some(prior) = maps.get_mut(&edit.notetype_id) {
                            if prior.to_struct_gen != edit.from_struct_gen || prior.new != edit.old {
                                return Err(Error::InvalidData);
                            }
                            prior.map = compose_maps(&prior.map, prior.old, prior.new, &edit.map, edit.new)?;
                            prior.to_struct_gen = edit.to_struct_gen;
                            prior.new = edit.new;
                        } else {
                            maps.insert(edit.notetype_id, edit.clone());
                        }
                    }
                    ResolvedChange::Type(edit) => {
                        let mut edit = edit.clone();
                        if let Some(prior) = maps.get(&edit.old_notetype_id) {
                            if prior.to_struct_gen != edit.old_struct_gen || prior.new != edit.old {
                                return Err(Error::InvalidData);
                            }
                            edit.map = compose_maps(&prior.map, prior.old, prior.new, &edit.map, edit.new)?;
                            edit.old_struct_gen = prior.from_struct_gen;
                            edit.old = prior.old;
                        }
                        let key = (edit.old_notetype_id, edit.new_notetype_id);
                        // One manifest has one map per pair. Incompatible
                        // repeated pairs cannot represent both operations.
                        if types.get(&key).is_some_and(|prior| prior != &edit) {
                            return Err(Error::InvalidData);
                        }
                        types.insert(key, edit);
                    }
                }
            }
        }
        self.anchors = anchors.into_iter().collect();
        self.members = members
            .into_iter()
            .map(|(record_name, erev)| GenerationMember { record_name, erev })
            .collect();
        self.reasons = reasons.into_iter().collect();
        self.maps = maps.into_values().collect();
        self.type_changes = types.into_values().collect();
        Ok(())
    }
}

impl GenerationLedger {
    pub(super) fn new(seeds: &[LayoutSeed]) -> Result<Self, Error> {
        let mut initial = BTreeMap::new();
        for seed in seeds {
            validate_shape(seed.shape)?;
            checked(seed.local)?;
            checked(seed.acked)?;
            if initial.insert(seed.notetype_id, seed.clone()).is_some() {
                return Err(Error::InvalidData);
            }
        }
        Ok(Self {
            last_commit: 0,
            last_claim: 0,
            layouts: initial.clone(),
            generations: Vec::new(),
            seeds: initial,
        })
    }
    /// K19.5's overlap hold only; completeness and apply remain the caller's.
    pub fn holds_remote_apply(&self, anchors: &[String]) -> bool {
        self.generations.iter().any(|g| {
            matches!(g.state, LocalState::Local | LocalState::Claiming) && g.anchors.iter().any(|a| anchors.contains(a))
        })
    }
    fn replay(&self) -> Result<Replay, Error> {
        let mut layouts = self.seeds.clone();
        let mut resolved = BTreeMap::new();
        let mut names = BTreeMap::new();
        let mut operations: Vec<_> = self.generations.iter().flat_map(|g| &g.operations).collect();
        operations.sort_by_key(|op| op.order);
        let mut order = 0;
        let mut ids = BTreeSet::new();
        for op in operations {
            order = increment(order)?;
            if op.order != order || !ids.insert(op.facts.id) {
                return Err(Error::InvalidData);
            }
            let mut canonical = op.facts.clone();
            canonical.canonicalize()?;
            if canonical != op.facts {
                return Err(Error::InvalidData);
            }
            resolved.insert(op.order, resolve(&op.facts, &mut layouts, &mut names)?);
        }
        if order != self.last_commit {
            return Err(Error::InvalidData);
        }
        Ok((layouts, resolved))
    }
    pub(super) fn validate(&self) -> Result<(), Error> {
        checked(self.last_commit)?;
        checked(self.last_claim)?;
        for (id, seed) in &self.seeds {
            if *id != seed.notetype_id {
                return Err(Error::InvalidData);
            }
            validate_shape(seed.shape)?;
            checked(seed.local)?;
            checked(seed.acked)?;
        }
        let (layouts, resolved) = self.replay()?;
        if layouts != self.layouts || self.generations.windows(2).any(|p| p[0].order >= p[1].order) {
            return Err(Error::InvalidData);
        }
        let mut active_claims = BTreeSet::new();
        for g in &self.generations {
            let mut expected = g.clone();
            expected.rebuild(&resolved)?;
            if &expected != g {
                return Err(Error::InvalidData);
            }
            match (g.state, g.active_claim) {
                (LocalState::Claiming, Some(n)) if n > 0 && n <= self.last_claim && active_claims.insert(n) => (),
                (LocalState::Claiming, _) | (_, Some(_)) => return Err(Error::InvalidData),
                _ => (),
            }
            let expected_wait: Vec<_> = self
                .generations
                .iter()
                .filter(|p| p.order < g.order && p.state == LocalState::Claiming && p.touches(g))
                .map(|p| p.id)
                .collect();
            if g.waits_for != expected_wait || (!g.waits_for.is_empty() && g.state != LocalState::Local) {
                return Err(Error::InvalidData);
            }
            if g.state == LocalState::Claiming
                && self.generations.iter().any(|prior| {
                    prior.order < g.order
                        && matches!(prior.state, LocalState::Local | LocalState::Claiming | LocalState::Lost)
                })
            {
                return Err(Error::InvalidData);
            }
        }
        if active_claims.len() > 1 {
            return Err(Error::InvalidData);
        }
        for (i, g) in self.generations.iter().enumerate() {
            for other in &self.generations[i + 1..] {
                if g.joinable() && other.joinable() && g.touches(other) {
                    return Err(Error::InvalidData);
                }
                let unclaimed = |state| matches!(state, LocalState::Local | LocalState::Claiming | LocalState::Lost);
                if unclaimed(g.state)
                    && unclaimed(other.state)
                    && g.members
                        .iter()
                        .any(|m| other.members.iter().any(|n| m.record_name == n.record_name))
                    && !(g.state == LocalState::Claiming
                        && other.state == LocalState::Local
                        && other.waits_for.contains(&g.id))
                {
                    return Err(Error::InvalidData);
                }
            }
        }
        Ok(())
    }
    fn rebuild_all(&mut self) -> Result<(), Error> {
        let (layouts, resolved) = self.replay()?;
        self.layouts = layouts;
        for g in &mut self.generations {
            g.rebuild(&resolved)?;
        }
        self.generations.sort_by_key(|g| g.order);
        let waits: Vec<Vec<_>> = self
            .generations
            .iter()
            .map(|g| {
                self.generations
                    .iter()
                    .filter(|p| p.order < g.order && p.state == LocalState::Claiming && p.touches(g))
                    .map(|p| p.id)
                    .collect()
            })
            .collect();
        for (g, wait) in self.generations.iter_mut().zip(waits) {
            g.waits_for = wait;
        }
        Ok(())
    }
    /// Join the entire connected overlap, retaining the earliest operation's
    /// identity/order, and replay all maps in original commit order.
    fn join(&mut self, id: GenerationId) -> Result<GenerationId, Error> {
        let mut index = self
            .generations
            .iter()
            .position(|g| g.id == id)
            .ok_or(Error::InvalidData)?;
        loop {
            let other = self
                .generations
                .iter()
                .enumerate()
                .find(|(i, g)| {
                    *i != index
                        && g.joinable()
                        && self.generations[index].joinable()
                        && g.touches(&self.generations[index])
                })
                .map(|(i, _)| i);
            let Some(other) = other else {
                break;
            };
            let keep = index.min(other);
            let remove = index.max(other);
            let removed = self.generations.remove(remove);
            let target = &mut self.generations[keep];
            if removed.state == LocalState::Lost {
                target.state = LocalState::Lost;
            }
            target.operations.extend(removed.operations);
            target.active_claim = None;
            self.rebuild_all()?;
            index = keep;
        }
        Ok(self.generations[index].id)
    }
    pub(super) fn local(
        &mut self,
        mut operation: LocalOperation,
        facts: &[AnchorFact],
    ) -> Result<LocalGeneration, Error> {
        operation.canonicalize()?;
        if self
            .generations
            .iter()
            .flat_map(|g| &g.operations)
            .any(|op| op.facts.id == operation.id)
        {
            return Err(Error::InvalidData);
        }
        self.last_commit = increment(self.last_commit)?;
        let id = operation.id;
        self.generations.push(LocalGeneration::empty(OrderedOperation {
            order: self.last_commit,
            facts: operation,
        }));
        self.rebuild_all()?;
        let joined = self.join(id)?;
        self.check_anchors(facts)?;
        self.validate()?;
        self.generations
            .iter()
            .find(|g| g.id == joined)
            .cloned()
            .ok_or(Error::InvalidData)
    }
    pub(super) fn begin(&mut self, id: GenerationId, last_order: u64) -> Result<ClaimTicket, Error> {
        let index = self
            .generations
            .iter()
            .position(|g| g.id == id)
            .ok_or(Error::InvalidData)?;
        let g = &self.generations[index];
        if g.state != LocalState::Local
            || g.last_order != last_order
            || !g.waits_for.is_empty()
            || self.generations[..index]
                .iter()
                .any(|g| matches!(g.state, LocalState::Local | LocalState::Claiming | LocalState::Lost))
        {
            return Err(Error::InvalidData);
        }
        self.last_claim = increment(self.last_claim)?;
        let g = &mut self.generations[index];
        g.state = LocalState::Claiming;
        g.active_claim = Some(self.last_claim);
        self.validate()?;
        Ok(ClaimTicket {
            generation: id,
            attempt: self.last_claim,
        })
    }
    pub(super) fn result(
        &mut self,
        claim_attempt: ClaimTicket,
        result: ClaimResult,
        facts: &[AnchorFact],
    ) -> Result<LocalGeneration, Error> {
        let g = self
            .generations
            .iter_mut()
            .find(|g| g.id == claim_attempt.generation)
            .ok_or(Error::InvalidData)?;
        if g.state != LocalState::Claiming || g.active_claim != Some(claim_attempt.attempt) {
            return Err(Error::InvalidData);
        }
        g.state = match result {
            ClaimResult::Retry => LocalState::Local,
            ClaimResult::Lost => LocalState::Lost,
        };
        g.active_claim = None;
        let joined = self.join(claim_attempt.generation)?;
        self.rebuild_all()?;
        self.check_anchors(facts)?;
        self.validate()?;
        self.generations
            .iter()
            .find(|g| g.id == joined)
            .cloned()
            .ok_or(Error::InvalidData)
    }
    pub(super) fn check_anchors(&mut self, facts: &[AnchorFact]) -> Result<(), Error> {
        let mut seen = BTreeSet::new();
        for fact in facts {
            if !seen.insert(fact.notetype_id) || !self.layouts.contains_key(&fact.notetype_id) {
                return Err(Error::InvalidData);
            }
            if let AnchorVersion::Live { struct_gen } = fact.version {
                checked(struct_gen)?;
            }
        }
        for g in &mut self.generations {
            if g.state != LocalState::Local || !g.waits_for.is_empty() {
                continue;
            }
            for map in &g.maps {
                if facts.iter().any(|f| {
                    f.notetype_id == map.notetype_id
                        && match f.version {
                            AnchorVersion::Live { struct_gen } => struct_gen > map.from_struct_gen,
                            AnchorVersion::Tombstone => true,
                            AnchorVersion::AssetPending => false,
                        }
                }) {
                    g.state = LocalState::Lost;
                    break;
                }
            }
        }
        Ok(())
    }
}
