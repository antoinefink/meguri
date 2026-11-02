//! K19.2/K19.4 ledger foundation. These are real SQLite fixtures, not SDK/Q11 proofs.
//! Authored before generation.rs/generation_store.rs; execution belongs to root.
use std::path::Path;

use meguri_engine::device_state::PublicationId;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_sync::cloud::{generation::*, *};
use rusqlite::{Connection, params};

const A: &str = "eaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "eaaaaaaaaaaaaaaaaaaaaaaaaae";
const C: &str = "eaaaaaaaaaaaaaaaaaaaaaaaaai";
const D: &str = "eaaaaaaaaaaaaaaaaaaaaaaaaam";
const E: &str = "eaaaaaaaaaaaaaaaaaaaaaaaaaq";

fn scope(container: &str, environment: Environment) -> BuildScope {
    BuildScope {
        container: ContainerId::new(container).unwrap(),
        environment,
    }
}
fn open(path: &Path, scope: BuildScope) -> TrackingStore {
    match TrackingStore::open(path, scope).unwrap() {
        OpenOutcome::Missing(s) | OpenOutcome::Opened(s) => s,
        _ => panic!("supported synthetic store"),
    }
}
fn binding(s: &mut TrackingStore, seed: u64) -> BindingId {
    s.create_binding(
        BindingFacts {
            account: OpaqueBytes(vec![17]),
            epoch: EpochId([31; 16]),
            record_key: RecordKey([91; 32]),
            publication: PublicationId::from_bytes([4; 16]),
            state: BindingState::Active,
        },
        &FixedClock::new(1000, UtcOffset::UTC),
        &SeededRandom::new(seed),
    )
    .unwrap()
    .id
}
// Binding fixtures obey the store's one-active-owner-per-scope constraint.
// These SQL edits stand in for completed account lifecycle work, not ledger APIs.
fn isolate(path: &Path, b: BindingId) {
    let c = Connection::open(path.join(TRACKING_FILE)).unwrap();
    c.execute(
        "UPDATE bindings SET facts=json_set(facts,'$.state',json('{\"Isolated\":\"SignedOut\"}')) WHERE id=?1",
        [b.0.as_slice()],
    )
    .unwrap();
}
fn activate(path: &Path, b: BindingId) {
    let c = Connection::open(path.join(TRACKING_FILE)).unwrap();
    c.execute(
        "UPDATE bindings SET facts=json_set(facts,'$.state','Active') WHERE id=?1",
        [b.0.as_slice()],
    )
    .unwrap();
}
fn shape(kind: NotetypeKind, fields: u32, templates: u32) -> LayoutShape {
    LayoutShape {
        kind,
        fields,
        templates,
    }
}
fn standard() -> LayoutShape {
    shape(NotetypeKind::Standard, 2, 2)
}
fn seed(nt: i64) -> LayoutSeed {
    LayoutSeed {
        notetype_id: nt,
        local: 0,
        acked: 0,
        shape: standard(),
    }
}
fn id(n: u8) -> GenerationId {
    GenerationId([n; 16])
}
fn map(fields: &[i32], templates: &[i32]) -> OrdinalMap {
    OrdinalMap {
        fields: fields.to_vec(),
        templates: templates.to_vec(),
    }
}
fn edit(nt: i64, anchor: &str, from: u64, fields: &[i32]) -> Change {
    Change::Layout(LayoutEdit {
        notetype_id: nt,
        anchor: anchor.into(),
        from_struct_gen: from,
        before: standard(),
        after: standard(),
        map: map(fields, &[0, 1]),
    })
}
fn op(n: u8, changes: Vec<Change>, members: &[(&str, u64)], reasons: &[u32]) -> LocalOperation {
    LocalOperation {
        id: id(n),
        changes,
        reasons: reasons.to_vec(),
        members: members
            .iter()
            .map(|(name, erev)| GenerationMember {
                record_name: (*name).into(),
                erev: *erev,
            })
            .collect(),
    }
}
fn rig() -> (tempfile::TempDir, TrackingStore, BindingId) {
    let dir = tempfile::tempdir().unwrap();
    let mut s = open(dir.path(), scope("synthetic-generation", Environment::Development));
    let b = binding(&mut s, 264);
    s.initialize_generation_ledger(b, &[seed(10), seed(20)]).unwrap();
    (dir, s, b)
}
fn local(s: &mut TrackingStore, b: BindingId, operation: LocalOperation) -> LocalGeneration {
    s.commit_local_generation(b, operation, &[]).unwrap()
}
fn stored_operation_orders(path: &Path, b: BindingId, generation: usize) -> Vec<i64> {
    let c = Connection::open(path.join(TRACKING_FILE)).unwrap();
    let mut q=c.prepare("SELECT json_extract(op.value,'$.order') FROM cloud_generation_ledgers AS ledger, json_each(ledger.content,?2) AS op WHERE ledger.binding=?1 ORDER BY CAST(op.key AS INTEGER)").unwrap();
    q.query_map(
        params![b.0.as_slice(), format!("$.generations[{generation}].operations")],
        |r| r.get(0),
    )
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

#[test]
fn anchorless_member_bridges_join_local_or_lost_wholes_without_reordering_independent_ops() {
    for lost in [false, true] {
        let (dir, mut s, b) = rig();
        local(&mut s, b, op(1, vec![], &[(A, 1), (C, 1)], &[8]));
        if lost {
            let claim_attempt = s.begin_generation_claim(b, id(1), 1).unwrap();
            s.record_generation_claim_result(b, claim_attempt, ClaimResult::Lost, &[])
                .unwrap();
        }
        let independent = local(&mut s, b, op(2, vec![], &[(B, 1)], &[7]));
        local(&mut s, b, op(3, vec![], &[(D, 1), (E, 1)], &[8]));
        // Only members connect these three imports; neither has any anchors.
        let bridge = local(&mut s, b, op(4, vec![], &[(C, 2), (D, 2)], &[8]));
        assert_eq!((bridge.id, bridge.order, bridge.last_order), (id(1), 1, 4));
        assert_eq!(
            bridge
                .members
                .iter()
                .map(|m| (m.record_name.as_str(), m.erev))
                .collect::<Vec<_>>(),
            vec![(A, 1), (C, 2), (D, 2), (E, 1)]
        );
        // Updating a member of either joined branch must retain the whole.
        let joined = local(&mut s, b, op(5, vec![], &[(E, 2), (A, 2)], &[8]));
        assert_eq!((joined.id, joined.order, joined.last_order), (id(1), 1, 5));
        assert_eq!(joined.state, if lost { LocalState::Lost } else { LocalState::Local });
        assert!(joined.anchors.is_empty() && joined.maps.is_empty() && joined.type_changes.is_empty());
        assert!(joined.waits_for.is_empty());
        assert_eq!(joined.reasons, vec![8]);
        assert_eq!(
            joined
                .members
                .iter()
                .map(|m| (m.record_name.as_str(), m.erev))
                .collect::<Vec<_>>(),
            vec![(A, 2), (C, 2), (D, 2), (E, 2)]
        );
        let after = s.generation_ledger(b).unwrap();
        assert_eq!(after.last_commit, 5);
        assert_eq!(after.generations, vec![joined.clone(), independent]);
        assert_eq!(stored_operation_orders(dir.path(), b, 0), vec![1, 3, 4, 5]);
        assert_eq!(stored_operation_orders(dir.path(), b, 1), vec![2]);
        // Backwards member revisions and reused operation ids stay rejected.
        for bad in [op(6, vec![], &[(C, 1)], &[8]), op(3, vec![], &[(C, 3)], &[8])] {
            assert_eq!(s.commit_local_generation(b, bad, &[]), Err(Error::InvalidData));
            assert_eq!(s.generation_ledger(b).unwrap(), after);
        }
        drop(s);
        let mut s = open(dir.path(), scope("synthetic-generation", Environment::Development));
        s.initialize_generation_ledger(b, &[]).unwrap();
        assert_eq!(s.generation_ledger(b).unwrap(), after);
        assert_eq!(s.begin_generation_claim(b, id(2), 2), Err(Error::InvalidData));
        assert_eq!(s.begin_generation_claim(b, id(1), 4), Err(Error::InvalidData));
        if lost {
            assert_eq!(s.begin_generation_claim(b, id(1), 5), Err(Error::InvalidData));
        } else {
            assert!(s.begin_generation_claim(b, id(1), 5).is_ok());
        }
    }
}

#[test]
fn anchorless_claiming_member_successors_wait_and_retry_or_loss_joins_durably_in_commit_order() {
    for outcome in [ClaimResult::Retry, ClaimResult::Lost] {
        let (dir, mut s, b) = rig();
        local(&mut s, b, op(1, vec![], &[(A, 1), (C, 1)], &[8]));
        let claim_attempt = s.begin_generation_claim(b, id(1), 1).unwrap();
        let independent = local(&mut s, b, op(2, vec![], &[(B, 1)], &[7]));
        let successor = local(&mut s, b, op(3, vec![], &[(C, 2), (D, 1)], &[8]));
        assert_eq!(successor.waits_for, vec![id(1)]);
        let later = local(&mut s, b, op(4, vec![], &[(E, 1)], &[8]));
        let successor = local(&mut s, b, op(5, vec![], &[(D, 2), (A, 2)], &[8]));
        assert_eq!((successor.id, successor.order, successor.last_order), (id(3), 3, 5));
        assert_eq!(successor.waits_for, vec![id(1)]);
        assert!(successor.anchors.is_empty());
        let before = s.generation_ledger(b).unwrap();
        assert_eq!(before.generations.len(), 4);
        assert_eq!(before.generations[0].state, LocalState::Claiming);
        assert!(before.generations[1].waits_for.is_empty());
        assert!(before.generations[3].waits_for.is_empty());
        assert_eq!(s.begin_generation_claim(b, id(3), 5), Err(Error::InvalidData));
        assert_eq!(s.begin_generation_claim(b, id(2), 2), Err(Error::InvalidData));
        assert_eq!(s.generation_ledger(b).unwrap(), before);
        drop(s);
        let mut s = open(dir.path(), scope("synthetic-generation", Environment::Development));
        s.initialize_generation_ledger(b, &[]).unwrap();
        assert_eq!(s.generation_ledger(b).unwrap(), before);
        // Failure after the in-memory result/join must roll back the whole
        // actual SQLite transaction, including counters and predecessor waits.
        let c = Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
        c.execute_batch("CREATE TRIGGER synthetic_member_join_abort BEFORE UPDATE ON cloud_generation_ledgers BEGIN SELECT RAISE(ABORT,'synthetic'); END;").unwrap();
        assert_eq!(
            s.record_generation_claim_result(b, claim_attempt, outcome, &[]),
            Err(Error::Constraint)
        );
        assert_eq!(s.generation_ledger(b).unwrap(), before);
        c.execute_batch("DROP TRIGGER synthetic_member_join_abort").unwrap();
        drop(c);
        let joined = s
            .record_generation_claim_result(b, claim_attempt, outcome, &[])
            .unwrap();
        assert_eq!((joined.id, joined.order, joined.last_order), (id(1), 1, 5));
        assert_eq!(
            joined.state,
            if outcome == ClaimResult::Lost {
                LocalState::Lost
            } else {
                LocalState::Local
            }
        );
        assert!(joined.waits_for.is_empty() && joined.anchors.is_empty());
        assert_eq!(joined.reasons, vec![8]);
        assert_eq!(
            joined
                .members
                .iter()
                .map(|m| (m.record_name.as_str(), m.erev))
                .collect::<Vec<_>>(),
            vec![(A, 2), (C, 2), (D, 2)]
        );
        let after = s.generation_ledger(b).unwrap();
        assert_eq!(after.generations, vec![joined, independent, later]);
        assert_eq!((after.last_commit, after.last_claim), (5, 1));
        assert_eq!(stored_operation_orders(dir.path(), b, 0), vec![1, 3, 5]);
        assert_eq!(stored_operation_orders(dir.path(), b, 1), vec![2]);
        assert_eq!(stored_operation_orders(dir.path(), b, 2), vec![4]);
        assert_eq!(
            s.record_generation_claim_result(b, claim_attempt, outcome, &[]),
            Err(Error::InvalidData)
        );
        assert_eq!(s.generation_ledger(b).unwrap(), after);
        drop(s);
        let s = open(dir.path(), scope("synthetic-generation", Environment::Development));
        assert_eq!(s.generation_ledger(b).unwrap(), after);
    }
}

#[test]
fn huge_valid_standard_template_count_with_short_cloze_identity_map_is_invalid_and_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let build = scope("synthetic-generation-huge-map", Environment::Development);
    let mut s = open(dir.path(), build.clone());
    let b = binding(&mut s, 302);
    // Both declared shapes are valid, including the maximum standard count.
    // Only the supplied identity prefix has the wrong length. Validation must
    // reject it without constructing a declared-count-sized expected Vec.
    let old = shape(NotetypeKind::Standard, 2, i32::MAX as u32);
    let new = shape(NotetypeKind::Cloze, 2, 1);
    s.initialize_generation_ledger(
        b,
        &[
            LayoutSeed { shape: old, ..seed(10) },
            LayoutSeed { shape: new, ..seed(20) },
        ],
    )
    .unwrap();
    let before = s.generation_ledger(b).unwrap();
    let bad = op(
        1,
        vec![Change::Type(TypeEdit {
            old_notetype_id: 10,
            old_struct_gen: 0,
            new_notetype_id: 20,
            new_struct_gen: 0,
            old,
            new,
            map: map(&[0, 1], &[0]),
        })],
        &[(C, 1)],
        &[6],
    );
    assert_eq!(s.commit_local_generation(b, bad, &[]), Err(Error::InvalidData));
    assert_eq!(s.generation_ledger(b).unwrap(), before);
    // Reuse the failed id and sequence position to prove no partial commit.
    let accepted = local(&mut s, b, op(1, vec![], &[(C, 1)], &[8]));
    assert_eq!((accepted.id, accepted.order, accepted.last_order), (id(1), 1, 1));
    let after = s.generation_ledger(b).unwrap();
    drop(s);
    let mut s = open(dir.path(), build);
    s.initialize_generation_ledger(b, &[]).unwrap();
    assert_eq!(s.generation_ledger(b).unwrap(), after);
}

#[test]
fn cloze_type_reporting_keeps_exact_standard_identity_and_empty_cloze_prefix_rules() {
    for old_is_cloze in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut s = open(
            dir.path(),
            scope("synthetic-generation-type-report", Environment::Development),
        );
        let b = binding(&mut s, 303);
        let cloze = shape(NotetypeKind::Cloze, 2, 1);
        let (old, new) = if old_is_cloze {
            (cloze, standard())
        } else {
            (standard(), cloze)
        };
        s.initialize_generation_ledger(
            b,
            &[
                LayoutSeed { shape: old, ..seed(10) },
                LayoutSeed { shape: new, ..seed(20) },
            ],
        )
        .unwrap();
        let operation = |templates: &[i32]| {
            op(
                1,
                vec![Change::Type(TypeEdit {
                    old_notetype_id: 10,
                    old_struct_gen: 0,
                    new_notetype_id: 20,
                    new_struct_gen: 0,
                    old,
                    new,
                    map: map(&[1, -1], templates),
                })],
                &[(C, 1)],
                &[6],
            )
        };
        let before = s.generation_ledger(b).unwrap();
        let malformed = if old_is_cloze {
            vec![vec![0], vec![-1], vec![0, 1]]
        } else {
            vec![vec![], vec![0], vec![0, 0], vec![1, 0], vec![0, 1, -1]]
        };
        for templates in malformed {
            assert_eq!(
                s.commit_local_generation(b, operation(&templates), &[]),
                Err(Error::InvalidData)
            );
            assert_eq!(s.generation_ledger(b).unwrap(), before);
        }
        let templates: &[i32] = if old_is_cloze { &[] } else { &[0, 1] };
        let accepted = local(&mut s, b, operation(templates));
        assert_eq!(accepted.type_changes[0].map, map(&[1, -1], templates));
        let after = s.generation_ledger(b).unwrap();
        drop(s);
        let s = open(
            dir.path(),
            scope("synthetic-generation-type-report", Environment::Development),
        );
        assert_eq!(s.generation_ledger(b).unwrap(), after);
    }
}

#[test]
fn overlapping_ops_canonicalize_whole_manifest_keep_order_and_undo_identity() {
    let (_dir, mut s, b) = rig();
    let first = local(
        &mut s,
        b,
        op(1, vec![edit(10, A, 0, &[1, 0])], &[(C, 1), (A, 1)], &[2, 1]),
    );
    let joined = local(
        &mut s,
        b,
        op(2, vec![edit(10, A, 1, &[1, 0])], &[(A, 2), (D, 1)], &[3, 1]),
    );
    assert_eq!(joined.id, first.id);
    assert_eq!((joined.order, joined.last_order), (1, 2));
    assert_eq!(joined.reasons, vec![1, 2, 3]);
    assert_eq!(joined.anchors, vec![A]);
    assert_eq!(
        joined
            .members
            .iter()
            .map(|m| (m.record_name.as_str(), m.erev))
            .collect::<Vec<_>>(),
        vec![(A, 2), (C, 1), (D, 1)]
    );
    assert_eq!(joined.maps[0].from_struct_gen, 0);
    assert_eq!(joined.maps[0].to_struct_gen, 2);
    assert_eq!(joined.maps[0].map.fields, vec![0, 1]);
    let ledger = s.generation_ledger(b).unwrap();
    assert_eq!(ledger.generations.len(), 1);
    assert_eq!(ledger.layouts[&10].local, 2);
    assert!(ledger.holds_remote_apply(&[A.to_owned()]));
    assert_eq!(s.begin_generation_claim(b, id(1), 1), Err(Error::InvalidData));
    assert!(s.begin_generation_claim(b, id(1), 2).is_ok());
}

#[test]
fn bridge_two_generations_retains_oldest_order_whole_maps_and_single_membership() {
    let (_dir, mut s, b) = rig();
    local(&mut s, b, op(1, vec![edit(10, A, 0, &[1, 0])], &[(A, 1), (C, 1)], &[1]));
    local(&mut s, b, op(2, vec![edit(20, B, 0, &[1, 0])], &[(B, 1), (D, 1)], &[2]));
    let joined = local(
        &mut s,
        b,
        op(
            3,
            vec![edit(20, B, 1, &[1, 0]), edit(10, A, 1, &[1, 0])],
            &[(B, 2), (A, 2)],
            &[3],
        ),
    );
    assert_eq!((joined.id, joined.order, joined.last_order), (id(1), 1, 3));
    assert_eq!(joined.anchors, vec![A, B]);
    assert_eq!(joined.members.len(), 4);
    assert_eq!(
        joined.maps.iter().map(|m| m.notetype_id).collect::<Vec<_>>(),
        vec![10, 20]
    );
    assert!(
        joined
            .maps
            .iter()
            .all(|m| m.map.fields == [0, 1] && m.from_struct_gen == 0 && m.to_struct_gen == 2)
    );
    assert_eq!(s.generation_ledger(b).unwrap().generations.len(), 1);
    assert_eq!(
        s.commit_local_generation(b, op(2, vec![], &[], &[8]), &[]),
        Err(Error::InvalidData)
    );
}

#[test]
fn claiming_successor_waits_and_retry_or_loss_joins_in_one_durable_result_transaction() {
    for outcome in [ClaimResult::Retry, ClaimResult::Lost] {
        let (dir, mut s, b) = rig();
        local(&mut s, b, op(1, vec![edit(10, A, 0, &[1, 0])], &[(A, 1), (C, 1)], &[1]));
        let claim_attempt = s.begin_generation_claim(b, id(1), 1).unwrap();
        let successor = local(
            &mut s,
            b,
            op(
                2,
                vec![edit(10, A, 1, &[1, 0]), edit(20, B, 0, &[1, 0])],
                &[(A, 2), (B, 1)],
                &[2],
            ),
        );
        assert_eq!(successor.waits_for, vec![id(1)]);
        let newer = AnchorFact {
            notetype_id: 10,
            version: AnchorVersion::Live { struct_gen: 2 },
        };
        s.check_generation_anchors(b, &[newer]).unwrap();
        let before = s.generation_ledger(b).unwrap();
        assert_eq!(before.generations[0].state, LocalState::Claiming);
        assert_eq!(before.generations[1].state, LocalState::Local);
        assert_eq!(s.begin_generation_claim(b, id(2), 2), Err(Error::InvalidData));
        drop(s);
        let mut s = open(dir.path(), scope("synthetic-generation", Environment::Development));
        s.initialize_generation_ledger(b, &[]).unwrap();
        assert_eq!(s.generation_ledger(b).unwrap(), before);
        let joined = s
            .record_generation_claim_result(b, claim_attempt, outcome, &[])
            .unwrap();
        assert_eq!(
            joined.state,
            if outcome == ClaimResult::Lost {
                LocalState::Lost
            } else {
                LocalState::Local
            }
        );
        assert_eq!((joined.id, joined.order, joined.last_order), (id(1), 1, 2));
        assert!(joined.waits_for.is_empty());
        assert_eq!(joined.maps.len(), 2);
        assert_eq!(joined.maps[0].map.fields, vec![0, 1]);
        assert_eq!(joined.maps[0].to_struct_gen, 2);
        assert_eq!(joined.members.len(), 3);
        let after = s.generation_ledger(b).unwrap();
        assert_eq!(after.generations.len(), 1);
        assert_eq!(
            s.record_generation_claim_result(b, claim_attempt, outcome, &[]),
            Err(Error::InvalidData)
        );
        assert_eq!(s.generation_ledger(b).unwrap(), after);
        drop(s);
        let s = open(dir.path(), scope("synthetic-generation", Environment::Development));
        assert_eq!(s.generation_ledger(b).unwrap(), after);
    }
}

#[test]
fn fetched_anchor_decides_local_and_lost_edit_joins_but_rolled_back_fixture_is_final() {
    let (dir, mut s, b) = rig();
    let newer = AnchorFact {
        notetype_id: 10,
        version: AnchorVersion::Live { struct_gen: 4 },
    };
    let g = s
        .commit_local_generation(b, op(1, vec![edit(10, A, 0, &[1, 0])], &[(A, 1)], &[1]), &[newer])
        .unwrap();
    assert_eq!(g.state, LocalState::Lost);
    let joined = local(&mut s, b, op(2, vec![edit(10, A, 1, &[1, 0])], &[(A, 2)], &[2]));
    assert_eq!((joined.id, joined.state), (id(1), LocalState::Lost));
    assert!(!s.generation_ledger(b).unwrap().holds_remote_apply(&[A.into()]));
    drop(s);
    // Fixture for the future completed-rollback writer. This component exposes
    // no fake rollback/apply function. Layouts deliberately remain unchanged.
    let c = Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    c.execute("UPDATE cloud_generation_ledgers SET content=json_set(content,'$.generations[0].state','RolledBack') WHERE binding=?1",[b.0.as_slice()]).unwrap();
    drop(c);
    let mut s = open(dir.path(), scope("synthetic-generation", Environment::Development));
    s.initialize_generation_ledger(b, &[]).unwrap();
    let next = local(&mut s, b, op(3, vec![edit(10, A, 2, &[0, 1])], &[(A, 3)], &[1]));
    assert_eq!((next.id, next.state), (id(3), LocalState::Local));
    assert_eq!(
        s.generation_ledger(b).unwrap().generations[0].state,
        LocalState::RolledBack
    );
}

#[test]
fn type_changes_compose_old_layout_before_and_new_layout_after() {
    let (_dir, mut s, b) = rig();
    local(&mut s, b, op(1, vec![edit(10, A, 0, &[1, 0])], &[(A, 1)], &[1]));
    let type_change = Change::Type(TypeEdit {
        old_notetype_id: 10,
        new_notetype_id: 20,
        old_struct_gen: 1,
        new_struct_gen: 0,
        old: standard(),
        new: standard(),
        map: map(&[1, -1], &[1, 0]),
    });
    // An anchor-bearing op also changes notes from the old notetype.
    local(
        &mut s,
        b,
        op(2, vec![type_change, edit(10, A, 1, &[0, 1])], &[(C, 1), (A, 2)], &[6]),
    );
    let joined = local(
        &mut s,
        b,
        op(
            3,
            vec![edit(20, B, 0, &[1, 0]), edit(10, A, 2, &[0, 1])],
            &[(B, 1), (A, 3)],
            &[2],
        ),
    );
    assert_eq!(joined.type_changes.len(), 1);
    let t = &joined.type_changes[0];
    assert_eq!((t.old_struct_gen, t.new_struct_gen), (0, 1));
    assert_eq!(t.map.fields, vec![-1, 0]);
    assert_eq!(t.map.templates, vec![1, 0]);
}

#[test]
fn prefix_composition_normative_and_longer_omitted_tail_cases() {
    let std3 = shape(NotetypeKind::Standard, 2, 3);
    let std2 = standard();
    let cloze = shape(NotetypeKind::Cloze, 2, 1);
    // K19.2: standard 3 -> standard 2 -> cloze.
    assert_eq!(
        compose_maps(&map(&[0, 1], &[0, 1]), std3, std2, &map(&[0, 1], &[0, 1]), cloze)
            .unwrap()
            .templates,
        vec![0, 1]
    );
    // K19.2: cloze -> standard 2 -> swapped standard 2.
    assert_eq!(
        compose_maps(&map(&[0, 1], &[]), cloze, std2, &map(&[0, 1], &[1, 0]), std2)
            .unwrap()
            .templates,
        vec![1, 0]
    );
    // Prefix entries beyond the shorter first map use the cloze identity tail.
    assert_eq!(
        compose_maps(
            &map(&[1, 0], &[2, -1]),
            cloze,
            cloze,
            &map(&[1, -1], &[3, 0, 1, 5]),
            cloze
        )
        .unwrap(),
        map(&[0, -1], &[3, 2, -1, 5])
    );
    // A shorter second map still preserves the first map's longer prefix.
    assert_eq!(
        compose_maps(&map(&[0, 1], &[3, 2, -1, 0]), cloze, cloze, &map(&[0, 1], &[1]), cloze)
            .unwrap()
            .templates,
        vec![2, 2, -1, 0]
    );
    // Standard input has no identity tail, even with cloze output.
    assert_eq!(
        compose_maps(&map(&[0, 1], &[1, 0]), std2, cloze, &map(&[0, 1], &[4, 1, 0]), cloze)
            .unwrap()
            .templates,
        vec![-1, 0, 1]
    );
    // Two kind-changing type changes are outside layout/type composition.
    assert_eq!(
        compose_maps(&map(&[0, 1], &[]), cloze, std2, &map(&[0, 1], &[0, 1]), cloze),
        Err(Error::InvalidData)
    );
    assert_eq!(
        compose_maps(&map(&[0, 1], &[7]), std2, cloze, &map(&[0, 1], &[]), cloze),
        Err(Error::InvalidData)
    );
}

#[test]
fn prefix_layout_and_type_maps_join_and_survive_sqlite_reopen() {
    let std3 = shape(NotetypeKind::Standard, 2, 3);
    let std2 = standard();
    let cloze = shape(NotetypeKind::Cloze, 2, 1);
    for case in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let build = scope("synthetic-prefix-ledger", Environment::Development);
        let mut s = open(dir.path(), build.clone());
        let b = binding(&mut s, 300 + case);
        let (old, new) = if case == 0 { (std3, cloze) } else { (cloze, std2) };
        s.initialize_generation_ledger(
            b,
            &[
                LayoutSeed { shape: old, ..seed(10) },
                LayoutSeed { shape: new, ..seed(20) },
            ],
        )
        .unwrap();
        let joined = match case {
            0 => {
                local(
                    &mut s,
                    b,
                    op(
                        1,
                        vec![Change::Layout(LayoutEdit {
                            notetype_id: 10,
                            anchor: A.into(),
                            from_struct_gen: 0,
                            before: std3,
                            after: std2,
                            map: map(&[0, 1], &[0, 1]),
                        })],
                        &[(A, 1)],
                        &[2],
                    ),
                );
                // The type-only op joins the generation of its old anchor.
                local(
                    &mut s,
                    b,
                    op(
                        2,
                        vec![Change::Type(TypeEdit {
                            old_notetype_id: 10,
                            old_struct_gen: 1,
                            new_notetype_id: 20,
                            new_struct_gen: 0,
                            old: std2,
                            new: cloze,
                            map: map(&[0, 1], &[0, 1]),
                        })],
                        &[(C, 1)],
                        &[6],
                    ),
                )
            }
            1 => {
                local(
                    &mut s,
                    b,
                    op(
                        1,
                        vec![Change::Type(TypeEdit {
                            old_notetype_id: 10,
                            old_struct_gen: 0,
                            new_notetype_id: 20,
                            new_struct_gen: 0,
                            old: cloze,
                            new: std2,
                            map: map(&[0, 1], &[]),
                        })],
                        &[(C, 1)],
                        &[6],
                    ),
                );
                local(
                    &mut s,
                    b,
                    op(
                        2,
                        vec![Change::Layout(LayoutEdit {
                            notetype_id: 20,
                            anchor: B.into(),
                            from_struct_gen: 0,
                            before: std2,
                            after: std2,
                            map: map(&[0, 1], &[1, 0]),
                        })],
                        &[(B, 1)],
                        &[2],
                    ),
                )
            }
            _ => {
                local(
                    &mut s,
                    b,
                    op(
                        1,
                        vec![Change::Layout(LayoutEdit {
                            notetype_id: 10,
                            anchor: A.into(),
                            from_struct_gen: 0,
                            before: cloze,
                            after: cloze,
                            map: map(&[1, 0], &[2, -1]),
                        })],
                        &[(A, 1)],
                        &[2],
                    ),
                );
                local(
                    &mut s,
                    b,
                    op(
                        2,
                        vec![Change::Layout(LayoutEdit {
                            notetype_id: 10,
                            anchor: A.into(),
                            from_struct_gen: 1,
                            before: cloze,
                            after: cloze,
                            map: map(&[1, -1], &[3, 0, 1, 5]),
                        })],
                        &[(A, 2)],
                        &[2],
                    ),
                )
            }
        };
        assert_eq!((joined.id, joined.order, joined.last_order), (id(1), 1, 2));
        match case {
            0 => {
                assert_eq!(joined.type_changes[0].old_struct_gen, 0);
                assert_eq!(joined.type_changes[0].map.templates, vec![0, 1]);
            }
            1 => {
                assert_eq!(joined.type_changes[0].new_struct_gen, 1);
                assert_eq!(joined.type_changes[0].map.templates, vec![1, 0]);
            }
            _ => {
                assert_eq!(joined.maps[0].map, map(&[0, -1], &[3, 2, -1, 5]));
            }
        }
        let before = s.generation_ledger(b).unwrap();
        drop(s);
        let mut s = open(dir.path(), build);
        s.initialize_generation_ledger(b, &[]).unwrap();
        assert_eq!(s.generation_ledger(b).unwrap(), before);
    }
}

#[test]
fn max_local_acked_checked_arithmetic_and_malformed_operations_are_atomic() {
    let (dir, mut s, b) = rig();
    let before = s.generation_ledger(b).unwrap();
    for bad in [
        op(1, vec![edit(10, A, 9, &[0, 1])], &[(A, 1)], &[1]),
        op(1, vec![edit(10, A, 0, &[2, 0])], &[(A, 1)], &[1]),
        op(1, vec![edit(10, A, 0, &[-2, 0])], &[(A, 1)], &[1]),
        op(1, vec![edit(10, A, 0, &[0, 1])], &[(A, 1)], &[11]),
        op(1, vec![edit(10, "invalid", 0, &[0, 1])], &[(A, 1)], &[1]),
        op(
            1,
            vec![edit(10, A, 0, &[0, 1]), edit(20, B, 9, &[0, 1])],
            &[(A, 1), (B, 1)],
            &[1],
        ),
    ] {
        assert_eq!(s.commit_local_generation(b, bad, &[]), Err(Error::InvalidData));
        assert_eq!(s.generation_ledger(b).unwrap(), before);
    }
    isolate(dir.path(), b);
    let b2 = binding(&mut s, 265);
    s.initialize_generation_ledger(
        b2,
        &[LayoutSeed {
            local: 2,
            acked: 7,
            ..seed(10)
        }],
    )
    .unwrap();
    let g = local(&mut s, b2, op(1, vec![edit(10, A, 2, &[0, 1])], &[(A, 1)], &[1]));
    assert_eq!((g.maps[0].from_struct_gen, g.maps[0].to_struct_gen), (2, 8));
    isolate(dir.path(), b2);
    let b3 = binding(&mut s, 266);
    s.initialize_generation_ledger(
        b3,
        &[LayoutSeed {
            acked: i64::MAX as u64,
            ..seed(10)
        }],
    )
    .unwrap();
    let before = s.generation_ledger(b3).unwrap();
    assert_eq!(
        s.commit_local_generation(b3, op(1, vec![edit(10, A, 0, &[0, 1])], &[(A, 1)], &[1]), &[]),
        Err(Error::InvalidData)
    );
    assert_eq!(s.generation_ledger(b3).unwrap(), before);
}

#[test]
fn sqlite_failure_rolls_back_result_join_counters_and_members_then_reopens_unchanged() {
    let (dir, mut s, b) = rig();
    local(&mut s, b, op(1, vec![edit(10, A, 0, &[1, 0])], &[(A, 1)], &[1]));
    let claim_attempt = s.begin_generation_claim(b, id(1), 1).unwrap();
    local(&mut s, b, op(2, vec![edit(10, A, 1, &[1, 0])], &[(A, 2)], &[2]));
    let before = s.generation_ledger(b).unwrap();
    let c = Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    c.execute_batch("CREATE TRIGGER synthetic_generation_abort BEFORE UPDATE ON cloud_generation_ledgers BEGIN SELECT RAISE(ABORT,'synthetic opaque value'); END;").unwrap();
    let err = s
        .record_generation_claim_result(b, claim_attempt, ClaimResult::Lost, &[])
        .unwrap_err();
    assert_eq!(err, Error::Constraint);
    assert!(!err.to_string().contains("opaque"));
    assert_eq!(s.generation_ledger(b).unwrap(), before);
    c.execute_batch("DROP TRIGGER synthetic_generation_abort").unwrap();
    drop(c);
    drop(s);
    let s = open(dir.path(), scope("synthetic-generation", Environment::Development));
    assert_eq!(s.generation_ledger(b).unwrap(), before);
}

#[test]
fn independent_bindings_scopes_future_schema_and_invalid_persisted_json_refuse() {
    let (dir, mut s, b) = rig();
    local(&mut s, b, op(1, vec![edit(10, A, 0, &[1, 0])], &[(A, 1)], &[1]));
    let original = s.generation_ledger(b).unwrap();
    isolate(dir.path(), b);
    let b2 = binding(&mut s, 267);
    s.initialize_generation_ledger(b2, &[seed(10)]).unwrap();
    assert_eq!(
        s.commit_local_generation(b, op(2, vec![], &[], &[8]), &[]),
        Err(Error::IsolatedBinding)
    );
    assert_eq!(s.generation_ledger(b).unwrap(), original);
    assert_eq!(s.generation_ledger(b2).unwrap().last_commit, 0);
    isolate(dir.path(), b2);
    activate(dir.path(), b);
    drop(s);
    for other in [
        scope("other-generation", Environment::Development),
        scope("synthetic-generation", Environment::Production),
    ] {
        let mut other = open(dir.path(), other);
        assert_eq!(other.initialize_generation_ledger(b, &[]), Err(Error::BindingNotFound));
        assert_eq!(other.generation_ledger(b), Err(Error::BindingNotFound));
        assert_eq!(
            other.commit_local_generation(b, op(2, vec![], &[], &[8]), &[]),
            Err(Error::BindingNotFound)
        );
    }
    let mut s = open(dir.path(), scope("synthetic-generation", Environment::Development));
    let before = s.generation_ledger(b).unwrap();
    let c = Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    c.execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE cloud_generation_schema SET version=99;")
        .unwrap();
    assert_eq!(
        s.initialize_generation_ledger(b, &[]),
        Err(Error::RuntimeSchemaUnsupported)
    );
    assert_eq!(s.generation_ledger(b), Err(Error::RuntimeSchemaUnsupported));
    c.execute_batch("UPDATE cloud_generation_schema SET version=1;")
        .unwrap();
    assert_eq!(s.generation_ledger(b).unwrap(), before);
    c.execute(
        "UPDATE cloud_generation_ledgers SET content=json_set(content,'$.last_commit',-1) WHERE binding=?1",
        params![b.0.as_slice()],
    )
    .unwrap();
    assert_eq!(s.initialize_generation_ledger(b, &[]), Err(Error::InvalidData));
    assert_eq!(s.generation_ledger(b), Err(Error::InvalidData));
}

#[test]
fn missing_initializer_orphan_shape_and_claim_counter_overflow_refuse_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = open(
        dir.path(),
        scope("synthetic-generation-shape", Environment::Development),
    );
    let b = binding(&mut s, 301);
    assert_eq!(s.generation_ledger(b), Err(Error::RuntimeSchemaUnsupported));
    let c = Connection::open(dir.path().join(TRACKING_FILE)).unwrap();
    c.execute_batch("CREATE TABLE cloud_generation_ledgers(content TEXT)")
        .unwrap();
    assert_eq!(s.initialize_generation_ledger(b, &[seed(10)]), Err(Error::InvalidData));
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM pragma_table_info('cloud_generation_ledgers')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    c.execute_batch("DROP TABLE cloud_generation_ledgers").unwrap();
    s.initialize_generation_ledger(b, &[seed(10)]).unwrap();
    local(&mut s, b, op(1, vec![edit(10, A, 0, &[0, 1])], &[(A, 1)], &[1]));
    c.execute(
        "UPDATE cloud_generation_ledgers SET content=json_set(content,'$.last_claim',?2) WHERE binding=?1",
        params![b.0.as_slice(), i64::MAX],
    )
    .unwrap();
    let before = s.generation_ledger(b).unwrap();
    assert_eq!(s.begin_generation_claim(b, id(1), 1), Err(Error::InvalidData));
    assert_eq!(s.generation_ledger(b).unwrap(), before);
    c.execute_batch("ALTER TABLE cloud_generation_ledgers ADD COLUMN future INTEGER")
        .unwrap();
    assert_eq!(s.initialize_generation_ledger(b, &[]), Err(Error::InvalidData));
    assert_eq!(s.generation_ledger(b), Err(Error::InvalidData));
}
