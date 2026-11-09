//! First bounded Q5 component. Local subproofs are deliberately separate from
//! whole Q5 requirements. This module cannot emit a lane/unit acceptance pass.
use super::devices::{Result, failure};
use super::{Caller, Field, Operation, Reply, RequestedFields, Scope, Service, child_driver::ChildDriver};
use meguri_host::CoreHost;
use meguri_model::rng::RandomSource;
use meguri_storage::{ReadRows, ReaderRole, RowKey};
use meguri_sync::cloud::{
    self as core,
    control_codec::Active,
    entity_codec as codec,
    row_payload::{self, View},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum DeviceId {
    A,
    B,
    C,
    D,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Timing {
    QuiescenceRelaunchAndFinalAfterOtherChecks,
    Quiescence,
    Always,
    AlwaysAndAfterApply,
    EngineStateSave,
    QuiescenceAndEveryLog,
    AfterExchange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Definition {
    pub name: &'static str,
    pub when: Timing,
}

/// Spec order and exact names from conformance/cloudkit-acceptance Q5.
/// Integrity additionally requires the final oracle check after all other checks.
pub const REGISTRY: [Definition; 21] = [
    Definition {
        name: "integrity",
        when: Timing::QuiescenceRelaunchAndFinalAfterOtherChecks,
    },
    Definition {
        name: "converged",
        when: Timing::Quiescence,
    },
    Definition {
        name: "history",
        when: Timing::Quiescence,
    },
    Definition {
        name: "no-loss",
        when: Timing::Quiescence,
    },
    Definition {
        name: "no-resurrection",
        when: Timing::Always,
    },
    Definition {
        name: "coherent",
        when: Timing::AlwaysAndAfterApply,
    },
    Definition {
        name: "holdback",
        when: Timing::Always,
    },
    Definition {
        name: "undo-safety",
        when: Timing::Always,
    },
    Definition {
        name: "cursor",
        when: Timing::EngineStateSave,
    },
    Definition {
        name: "isolation",
        when: Timing::Always,
    },
    Definition {
        name: "stale-refused",
        when: Timing::Always,
    },
    Definition {
        name: "one-engine",
        when: Timing::Always,
    },
    Definition {
        name: "confirmed-root",
        when: Timing::Always,
    },
    Definition {
        name: "no-timer",
        when: Timing::Always,
    },
    Definition {
        name: "no-anki",
        when: Timing::Always,
    },
    Definition {
        name: "content-free",
        when: Timing::QuiescenceAndEveryLog,
    },
    Definition {
        name: "copies",
        when: Timing::Always,
    },
    Definition {
        name: "media-safe",
        when: Timing::Always,
    },
    Definition {
        name: "crt",
        when: Timing::Always,
    },
    Definition {
        name: "bounded",
        when: Timing::Always,
    },
    Definition {
        name: "results",
        when: Timing::AfterExchange,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum ProofState {
    Proven,
    Failed,
    Missing,
}

#[derive(Clone, Debug, Serialize)]
pub struct Fact {
    pub name: &'static str,
    pub state: ProofState,
}

#[derive(Clone, Debug, Serialize)]
pub struct Coverage {
    pub definition: Definition,
    /// No whole requirement is Proven in this first component.
    pub state: ProofState,
    pub facts: Vec<Fact>,
    pub missing: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Membership {
    Compared,
    Unbound,
    Isolated,
    OtherEpoch,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct Member {
    pub device: DeviceId,
    pub membership: Membership,
}

/// Only fixed labels, kinds and counts. No identifiers, settings or payloads.
#[derive(Clone, Debug, Serialize)]
pub struct ComponentReport {
    pub checks: Vec<Coverage>,
    pub members: Vec<Member>,
    pub compared_devices: usize,
    pub compared_groups: usize,
    pub observed_entities: usize,
    pub observed_reviews: usize,
    pub input_complete: bool,
}

// This is internal IPC, never a report or a source of caller assertions. Only
// observe_host constructs it; no public API accepts snapshots or pass booleans.
#[derive(Serialize, Deserialize)]
pub(crate) struct Observation {
    binding: Option<core::Binding>,
    local: Local,
    collection: Option<Collection>,
}
impl std::fmt::Debug for Observation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Observation")
    }
}
#[derive(Serialize, Deserialize)]
struct Local {
    sqlite_ok: bool,
    schema_ok: bool,
    references_ok: bool,
    fields_ok: bool,
}
#[derive(Serialize, Deserialize)]
struct Collection {
    crt: i64,
    // K6 kinds 1..8 only. Kind 9 and actual file/missing-list evidence remain
    // unsupported; do not reuse a cached tracking MediaName as a file proxy.
    entities: BTreeMap<String, [u8; 32]>,
    reviews: BTreeMap<i64, [u8; 32]>,
}

/// One actual Host snapshot; SQLite/reference facts survive projection refusal.
/// Layout generation comes from actual acknowledged production facts, while
/// every payload body is projected from the live collection reader.
pub(crate) fn observe_host(host: &CoreHost) -> Result<Observation> {
    let runtime = host.cloud_runtime();
    let binding = runtime.map(|r| r.status()).transpose()?.and_then(|s| s.binding);
    let mut layouts = BTreeMap::new();
    if let Some(r) = runtime.filter(|_| binding.is_some()) {
        let mut after = None;
        loop {
            let page = r.entity_facts_page(codec::Kind::Notetype, after.as_deref())?;
            if page.is_empty() {
                break;
            }
            for entity in page {
                after = Some(entity.key);
                if let Some(codec::Payload::Notetype(n)) = entity.acked_payload {
                    layouts.insert(n.id, n);
                }
            }
        }
    }
    let collection = host.collection()?;
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    let integrity = snapshot.smoke_integrity()?;
    let local = Local {
        sqlite_ok: integrity.integrity_ok,
        schema_ok: integrity.schema_ok,
        references_ok: integrity.missing_card_notes == 0
            && integrity.missing_card_decks == 0
            && integrity.missing_note_types == 0,
        fields_ok: integrity.missing_field_types == 0 && integrity.wrong_field_counts == 0,
    };
    let projected = (|| -> Result<Collection> {
        let bound = binding.as_ref().ok_or_else(failure)?;
        let view = row_payload::ReadView(&*snapshot);
        let mut entities = BTreeMap::new();
        let mut reviews = BTreeMap::new();
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
                    let next = RowKey::from_stored(table, &row)?;
                    if after.as_ref() == Some(&next) {
                        return Err(failure());
                    }
                    after = Some(next);
                    let payload = row_payload::project(&view, kind, &row, |id| {
                        layouts.get(&id).cloned().ok_or(core::Error::InvalidData)
                    })?;
                    let digest = codec::sha256(&payload.encode()?);
                    let name = codec::entity_name(&bound.facts.record_key.0, kind, &row_payload::key(kind, &row)?)?;
                    if entities.insert(name, digest).is_some() {
                        return Err(failure());
                    }
                    if let codec::Payload::Revlog(review) = payload
                        && reviews.insert(review.id, digest).is_some()
                    {
                        return Err(failure());
                    }
                }
            }
        }
        Ok(Collection {
            crt: snapshot.col()?.created.0,
            entities,
            reviews,
        })
    })()
    .ok();
    // A binding transition cannot bless an observation collected across it.
    if runtime.map(|r| r.status()).transpose()?.and_then(|s| s.binding) != binding {
        return Err(failure());
    }
    Ok(Observation {
        binding,
        local,
        collection: projected,
    })
}

type Group = (Scope, [u8; 16]);

/// Retains review digests seen in actual snapshots of an active epoch. This
/// detects later removal even from every member, but cannot witness commits
/// made and lost between samples. Full history still needs the operation journal
/// and confirmed-replacement ancestry ledger; neither is invented here.
pub struct CheckRun {
    expected: BTreeSet<DeviceId>,
    valid_roster: bool,
    retained: BTreeMap<Group, BTreeMap<i64, BTreeSet<[u8; 32]>>>,
}
impl std::fmt::Debug for CheckRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CheckRun")
    }
}
impl CheckRun {
    pub fn new(devices: impl IntoIterator<Item = DeviceId>) -> Self {
        let mut expected = BTreeSet::new();
        let mut valid_roster = true;
        for device in devices {
            valid_roster &= expected.insert(device);
        }
        valid_roster &= !expected.is_empty();
        Self {
            expected,
            valid_roster,
            retained: BTreeMap::new(),
        }
    }

    /// Call after a real drain. This neither drives devices nor asserts that
    /// quiescence occurred. It reads current Active by ordinary Service reads
    /// (logged as checker own operations), never capped fixture inspection.
    /// Every expected member must be supplied exactly once. Read/IPC/projection
    /// failures remain missing observations and cannot yield a local pass.
    pub fn at_quiescence<R: RandomSource>(
        &mut self,
        service: &mut Service<R>,
        devices: &mut [(DeviceId, &mut ChildDriver)],
    ) -> ComponentReport {
        let mut seen = BTreeSet::new();
        let mut observations = BTreeMap::new();
        let mut complete = self.valid_roster;
        for (id, driver) in devices {
            if !self.expected.contains(id) || !seen.insert(*id) {
                complete = false;
                continue;
            }
            match driver.check_observation() {
                Ok(observation) => {
                    observations.insert(*id, observation);
                }
                Err(_) => complete = false,
            }
        }
        self.evaluate(service, seen, observations, complete)
    }

    /// Same collector and predicates for Q4's in-process devices. Host handles
    /// are the only input; callers cannot inject expected rows or assertions.
    pub fn at_quiescence_hosts<R: RandomSource>(
        &mut self,
        service: &mut Service<R>,
        devices: &[(DeviceId, &CoreHost)],
    ) -> ComponentReport {
        let mut seen = BTreeSet::new();
        let mut hosts: Vec<&CoreHost> = Vec::new();
        let mut observations = BTreeMap::new();
        let mut complete = self.valid_roster;
        for (id, host) in devices {
            if !self.expected.contains(id) || !seen.insert(*id) || hosts.iter().any(|other| std::ptr::eq(*other, *host))
            {
                complete = false;
                continue;
            }
            hosts.push(*host);
            match observe_host(host) {
                Ok(observation) => {
                    observations.insert(*id, observation);
                }
                Err(_) => complete = false,
            }
        }
        self.evaluate(service, seen, observations, complete)
    }

    fn evaluate<R: RandomSource>(
        &mut self,
        service: &mut Service<R>,
        seen: BTreeSet<DeviceId>,
        observations: BTreeMap<DeviceId, Observation>,
        mut complete: bool,
    ) -> ComponentReport {
        complete &= seen == self.expected && observations.len() == self.expected.len();
        let mut active = BTreeMap::new();
        let mut groups: BTreeMap<Group, Vec<&Observation>> = BTreeMap::new();
        let mut members = Vec::new();
        for id in &self.expected {
            let membership = match observations.get(id) {
                None => Membership::Unavailable,
                Some(observation) => match &observation.binding {
                    None => Membership::Unbound,
                    Some(b) if b.facts.state != core::BindingState::Active => Membership::Isolated,
                    Some(b) => match scope(b) {
                        Err(_) => {
                            complete = false;
                            Membership::Unavailable
                        }
                        Ok(scope) => {
                            let epoch = *active
                                .entry(scope.clone())
                                .or_insert_with(|| read_active(service, &scope).ok());
                            match epoch {
                                None => {
                                    complete = false;
                                    Membership::Unavailable
                                }
                                Some(epoch) if epoch != b.facts.epoch.0 => Membership::OtherEpoch,
                                Some(epoch) => {
                                    groups.entry((scope, epoch)).or_default().push(observation);
                                    Membership::Compared
                                }
                            }
                        }
                    },
                },
            };
            members.push(Member {
                device: *id,
                membership,
            });
        }
        let compared_devices = groups.values().map(Vec::len).sum();
        let mut compared_complete = complete && compared_devices > 0;
        let mut canonical_ok = true;
        let mut crt_ok = true;
        let mut history_ok = true;
        let mut observed_entities = 0;
        let mut observed_reviews = 0;
        for (group, peers) in &groups {
            let retained = self.retained.entry(group.clone()).or_default();
            for peer in peers {
                if let Some(c) = &peer.collection {
                    observed_entities += c.entities.len();
                    observed_reviews += c.reviews.len();
                    for (id, digest) in &c.reviews {
                        retained.entry(*id).or_default().insert(*digest);
                    }
                } else {
                    compared_complete = false;
                }
            }
            let first = peers[0].collection.as_ref();
            for peer in peers {
                if let (Some(first), Some(c)) = (first, &peer.collection) {
                    canonical_ok &= c.entities == first.entities
                        && peer.binding.as_ref().map(|b| &b.facts.record_key)
                            == peers[0].binding.as_ref().map(|b| &b.facts.record_key);
                    crt_ok &= c.crt == first.crt;
                    history_ok &= retained.iter().all(|(id, digests)| {
                        digests.len() == 1 && c.reviews.get(id).is_some_and(|d| digests.contains(d))
                    });
                }
            }
        }
        let local_state = |predicate: fn(&Local) -> bool| {
            if observations.values().any(|o| !predicate(&o.local)) {
                ProofState::Failed
            } else if complete {
                ProofState::Proven
            } else {
                ProofState::Missing
            }
        };
        let compared_state = |ok: bool| {
            if !ok {
                ProofState::Failed
            } else if compared_complete {
                ProofState::Proven
            } else {
                ProofState::Missing
            }
        };
        let mut checks: Vec<_> = REGISTRY
            .into_iter()
            .map(|definition| Coverage {
                definition,
                state: ProofState::Missing,
                facts: Vec::new(),
                missing: vec!["not-implemented"],
            })
            .collect();
        for check in &mut checks {
            let fact = |name, state| Fact { name, state };
            match check.definition.name {
                "integrity" => {
                    check.facts = vec![
                        fact("local-sqlite-integrity", local_state(|l| l.sqlite_ok)),
                        fact("local-schema-18-expected-objects", local_state(|l| l.schema_ok)),
                        fact("local-references", local_state(|l| l.references_ok)),
                    ];
                    check.missing = vec![
                        "oracle-check-database-on-copy-plus-wal-schema18-frozen-clock-unchanged-scm",
                        "relaunch-and-final-after-all-checks-integration",
                    ];
                }
                "coherent" => {
                    check.facts = vec![
                        fact("local-references", local_state(|l| l.references_ok)),
                        fact("local-field-counts", local_state(|l| l.fields_ok)),
                    ];
                    check.missing = vec![
                        "complete-structural-generation-and-epoch",
                        "every-step-and-apply-integration",
                    ];
                }
                "converged" => {
                    check.facts = vec![
                        fact(
                            "canonical-collection-payloads-kinds-1-through-8",
                            compared_state(canonical_ok),
                        ),
                        fact("equal-col-crt", compared_state(crt_ok)),
                    ];
                    check.missing = vec![
                        "canonical-media-name-entities",
                        "actual-media-file-hashes-and-media-missing-lists",
                        "confirmed-replacement-ancestry-ledger",
                        "quiescence-proof",
                    ];
                }
                "history" => {
                    check.facts = vec![fact(
                        "retained-observed-committed-review-rows",
                        compared_state(history_ok),
                    )];
                    check.missing = vec![
                        "all-committed-review-operations-between-samples",
                        "confirmed-replacement-ancestry-ledger",
                        "quiescence-proof",
                    ];
                }
                _ => {}
            }
            if check.facts.iter().any(|f| f.state == ProofState::Failed) {
                check.state = ProofState::Failed;
            }
        }
        ComponentReport {
            checks,
            members,
            compared_devices,
            compared_groups: groups.len(),
            observed_entities,
            observed_reviews,
            input_complete: complete,
        }
    }
}

fn scope(binding: &core::Binding) -> Result<Scope> {
    Ok(Scope {
        container: binding.scope.container.as_str().to_owned(),
        environment: match binding.scope.environment {
            core::Environment::Development => "development",
            core::Environment::Production => "production",
        }
        .into(),
        account: std::str::from_utf8(&binding.facts.account.0)?.to_owned(),
    })
}
fn read_active<R: RandomSource>(service: &mut Service<R>, scope: &Scope) -> Result<[u8; 16]> {
    let response = service.request(
        scope,
        Caller {
            own_operation: Some("q5-check-active".into()),
            ..Caller::default()
        },
        Operation::Read {
            zone: "meguri-root".into(),
            name: "active".into(),
            fields: RequestedFields::Named(vec!["e".into()]),
        },
    )?;
    let Ok(Reply::Record(record)) = response.result else {
        return Err(failure());
    };
    if record.record_type != "MGActive" {
        return Err(failure());
    }
    let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
        return Err(failure());
    };
    Ok(Active::decode(e)?.epoch)
}
