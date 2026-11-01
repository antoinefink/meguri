//! Service-foundation tests only: Q2/Q3.1/Q3.2/Q3.6/Q3.7 and K3.
//! These synthetic cases prove no P24, U117, Q5 or signed-lane acceptance.
use meguri_model::rng::SeededRandom;
use meguri_testkit::cloud::*;
use tempfile::TempDir;

type Cloud = Service<SeededRandom>;
fn scope() -> Scope {
    Scope {
        container: "fixture-container".into(),
        environment: "development".into(),
        account: "acct-a".into(),
    }
}
fn cloud(config: Config) -> (TempDir, Cloud) {
    let dir = tempfile::tempdir().unwrap();
    let service = Service::open(dir.path(), config, SeededRandom::new(23)).unwrap();
    (dir, service)
}
fn call(service: &mut Cloud, scope: &Scope, op: Operation) -> Result<Reply, ServiceError> {
    service.request(scope, Caller::default(), op).unwrap().result
}
fn create(service: &mut Cloud, scope: &Scope, zone: &str) {
    assert_eq!(
        call(service, scope, Operation::CreateZone { zone: zone.into() }),
        Ok(Reply::Done)
    );
}
fn input(name: &str, value: u8) -> RecordInput {
    RecordInput::encrypted(name, "MGEntity", vec![value])
}
fn save(record: RecordInput, tag: Option<SystemFields>) -> Mutation {
    Mutation::Save {
        record,
        if_unchanged: tag,
    }
}
fn batch(service: &mut Cloud, scope: &Scope, zone: &str, atomic: bool, changes: Vec<Mutation>) -> Vec<MutationResult> {
    match call(
        service,
        scope,
        Operation::Modify {
            zone: zone.into(),
            atomic,
            changes,
        },
    )
    .unwrap()
    {
        Reply::Modified(results) => results,
        _ => panic!("expected modify reply"),
    }
}
fn saved(result: &MutationResult) -> Record {
    match result.result.as_ref().unwrap() {
        MutationSuccess::Saved(record) => record.clone(),
        _ => panic!("expected saved record"),
    }
}
fn put(service: &mut Cloud, scope: &Scope, zone: &str, record: RecordInput, tag: Option<SystemFields>) -> Record {
    saved(&batch(service, scope, zone, true, vec![save(record, tag)])[0])
}
fn read(service: &mut Cloud, scope: &Scope, zone: &str, name: &str) -> Record {
    match call(
        service,
        scope,
        Operation::Read {
            zone: zone.into(),
            name: name.into(),
            fields: RequestedFields::All,
        },
    )
    .unwrap()
    {
        Reply::Record(r) => r,
        _ => panic!("expected record reply"),
    }
}
fn zone_page(service: &mut Cloud, scope: &Scope, zone: &str, token: Option<Token>) -> Page<RecordChange> {
    match call(
        service,
        scope,
        Operation::FetchZone {
            zone: zone.into(),
            token,
        },
    )
    .unwrap()
    {
        Reply::ZoneChanges(p) => p,
        _ => panic!("expected zone page"),
    }
}
fn db_page(service: &mut Cloud, scope: &Scope, token: Option<Token>) -> Page<ZoneChange> {
    match call(service, scope, Operation::FetchDatabase { token }).unwrap() {
        Reply::DatabaseChanges(p) => p,
        _ => panic!("expected database page"),
    }
}
fn code(result: &MutationResult) -> ErrorCode {
    result.result.as_ref().unwrap_err().code
}
fn configure(service: &mut Cloud, scope: &Scope, config: Config) {
    assert_eq!(call(service, scope, Operation::Configure(config)), Ok(Reply::Done));
}
fn quota(service: &mut Cloud, scope: &Scope, bytes: Option<u64>) {
    assert_eq!(call(service, scope, Operation::SetQuota { bytes }), Ok(Reply::Done));
}
fn delete(service: &mut Cloud, scope: &Scope, zone: &str, name: &str) -> MutationResult {
    batch(service, scope, zone, true, vec![Mutation::Delete { name: name.into() }]).remove(0)
}

/// Q3.1/Q3.2/Q3.7: every dimension (including zone and config/quota) is isolated.
#[test]
fn all_scopes_and_zones_are_isolated() {
    let (_dir, mut s) = cloud(Config::default());
    let a = scope();
    let mut scopes = vec![a.clone()];
    let mut b = a.clone();
    b.container = "other-container".into();
    scopes.push(b);
    let mut b = a.clone();
    b.environment = "other-environment".into();
    scopes.push(b);
    let mut b = a.clone();
    b.account = "acct-b".into();
    scopes.push(b);
    for (index, sc) in scopes.iter().enumerate() {
        for (zone, value) in [("z", index as u8), ("other-zone", index as u8 + 10)] {
            create(&mut s, sc, zone);
            put(&mut s, sc, zone, input("same-name", value), None);
        }
    }
    for (index, sc) in scopes.iter().enumerate() {
        for (zone, value) in [("z", index as u8), ("other-zone", index as u8 + 10)] {
            assert_eq!(
                read(&mut s, sc, zone, "same-name").fields["e"],
                Field::EncryptedBytes(vec![value])
            );
        }
    }
    let token = zone_page(&mut s, &a, "z", None).token;
    for sc in &scopes[1..] {
        assert_eq!(
            call(
                &mut s,
                sc,
                Operation::FetchZone {
                    zone: "z".into(),
                    token: Some(token.clone())
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::InvalidArguments
        );
    }
    assert_eq!(
        call(
            &mut s,
            &a,
            Operation::FetchZone {
                zone: "other-zone".into(),
                token: Some(token)
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArguments
    );
    quota(&mut s, &a, Some(0));
    configure(
        &mut s,
        &a,
        Config {
            page_size: 1,
            ..Config::default()
        },
    );
    for sc in &scopes[1..] {
        assert_eq!(s.config(sc).unwrap(), Config::default());
        assert_eq!(s.inspect(sc).unwrap().unwrap().quota, None);
    }
}

/// Q3.1: records, logs, outcomes, snapshots, causes and settings survive reopen.
#[test]
fn reopen_preserves_state_and_snapshot_continuations() {
    let (dir, mut s) = cloud(Config {
        page_size: 1,
        ..Config::default()
    });
    let sc = scope();
    create(&mut s, &sc, "z");
    put(&mut s, &sc, "z", input("a", 1), None);
    put(&mut s, &sc, "z", input("b", 2), None);
    create(&mut s, &sc, "gone");
    call(
        &mut s,
        &sc,
        Operation::DeleteZone {
            zone: "gone".into(),
            cause: RemovalCause::Purged,
        },
    )
    .unwrap();
    quota(&mut s, &sc, Some(900));
    let first = zone_page(&mut s, &sc, "z", None);
    assert!(first.more);
    let before = s.inspect(&sc).unwrap();
    let logs = s.request_log().unwrap();
    let outcomes = s.outcomes().unwrap();
    let random = s.into_random();
    assert!(dir.path().join("fake-cloud.db").is_file());
    let mut s = Service::open(dir.path(), Config::default(), random).unwrap();
    assert_eq!(s.inspect(&sc).unwrap(), before);
    assert_eq!(s.request_log().unwrap(), logs);
    assert_eq!(s.outcomes().unwrap(), outcomes);
    let last = zone_page(&mut s, &sc, "z", Some(first.token));
    assert!(!last.more);
    assert!(matches!(&last.changes[0], RecordChange::Saved(r) if r.name == "b"));
    assert_eq!(s.config(&sc).unwrap().page_size, 1);
    let response = s
        .request(
            &sc,
            Caller::default(),
            Operation::Read {
                zone: "gone".into(),
                name: "x".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    assert_eq!(response.request_id, logs.len() as u64 + 2);
    assert_eq!(response.result.unwrap_err().code, ErrorCode::UserDeletedZone);
}

/// Q3.2/A20 zone lifecycle.
#[test]
fn q3_zone_lifecycle() {
    let (_dir, mut s) = cloud(Config::default());
    let sc = scope();
    for cause in [
        RemovalCause::Deleted,
        RemovalCause::Purged,
        RemovalCause::EncryptedDataReset,
    ] {
        create(&mut s, &sc, "z");
        put(&mut s, &sc, "z", input("a", 1), None);
        let before = s.inspect(&sc).unwrap();
        create(&mut s, &sc, "z");
        assert_eq!(s.inspect(&sc).unwrap(), before);
        let old = zone_page(&mut s, &sc, "z", None).token;
        call(
            &mut s,
            &sc,
            Operation::DeleteZone {
                zone: "z".into(),
                cause,
            },
        )
        .unwrap();
        let expected = if cause == RemovalCause::Purged {
            ErrorCode::UserDeletedZone
        } else {
            ErrorCode::ZoneNotFound
        };
        for op in [
            Operation::Read {
                zone: "z".into(),
                name: "a".into(),
                fields: RequestedFields::All,
            },
            Operation::Modify {
                zone: "z".into(),
                atomic: true,
                changes: vec![save(input("a", 2), None)],
            },
            Operation::FetchZone {
                zone: "z".into(),
                token: None,
            },
            Operation::DeleteZone {
                zone: "z".into(),
                cause: RemovalCause::Deleted,
            },
        ] {
            let err = call(&mut s, &sc, op).unwrap_err();
            assert_eq!(err.code, expected);
            assert_eq!(err.encrypted_data_reset, cause == RemovalCause::EncryptedDataReset);
        }
        assert_eq!(s.inspect(&sc).unwrap().unwrap().zones["z"].removed, Some(cause));
        let removed_history = s.inspect(&sc).unwrap().unwrap().zones["z"].changes.clone();
        create(&mut s, &sc, "z");
        let recreated = s.inspect(&sc).unwrap().unwrap();
        assert_eq!(recreated.zones["z"].changes, removed_history);
        assert!(
            recreated.zones["z"]
                .changes
                .iter()
                .all(|c| c.generation != recreated.zones["z"].generation)
        );
        assert!(zone_page(&mut s, &sc, "z", None).changes.is_empty());
        assert_eq!(
            call(
                &mut s,
                &sc,
                Operation::FetchZone {
                    zone: "z".into(),
                    token: Some(old)
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::ChangeTokenExpired
        );
        assert_eq!(
            call(
                &mut s,
                &sc,
                Operation::Read {
                    zone: "z".into(),
                    name: "a".into(),
                    fields: RequestedFields::All
                }
            )
            .unwrap_err()
            .code,
            ErrorCode::UnknownItem
        );
    }
    assert_eq!(
        call(
            &mut s,
            &sc,
            Operation::Read {
                zone: "never-created".into(),
                name: "a".into(),
                fields: RequestedFields::All
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::ZoneNotFound
    );
}

/// Q3.2/A5: current/stale/new saves and both tagged-after-delete behaviors.
#[test]
fn conditional_saves_derive_conflicts_from_server_state() {
    let (_dir, mut s) = cloud(Config::default());
    let sc = scope();
    create(&mut s, &sc, "z");
    let first = put(&mut s, &sc, "z", input("a", 1), None);
    let conflict = batch(&mut s, &sc, "z", false, vec![save(input("a", 2), None)]).remove(0);
    assert_eq!(code(&conflict), ErrorCode::ServerRecordChanged);
    assert_eq!(conflict.result.unwrap_err().server_record.as_deref(), Some(&first));
    let current = put(&mut s, &sc, "z", input("a", 2), Some(first.system_fields.clone()));
    assert_ne!(current.system_fields, first.system_fields);
    let stale = batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![save(input("a", 3), Some(first.system_fields.clone()))],
    )
    .remove(0);
    assert_eq!(code(&stale), ErrorCode::ServerRecordChanged);
    assert_eq!(stale.result.unwrap_err().server_record.as_deref(), Some(&current));
    assert_eq!(read(&mut s, &sc, "z", "a"), current);
    assert_eq!(delete(&mut s, &sc, "z", "a").result, Ok(MutationSuccess::Deleted));
    assert_eq!(code(&delete(&mut s, &sc, "z", "a")), ErrorCode::UnknownItem);
    let missing = batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![save(input("a", 4), Some(current.system_fields.clone()))],
    );
    assert_eq!(code(&missing[0]), ErrorCode::UnknownItem);
    configure(
        &mut s,
        &sc,
        Config {
            tagged_missing: TaggedMissing::Recreate,
            ..Config::default()
        },
    );
    let recreated = put(&mut s, &sc, "z", input("a", 4), Some(current.system_fields));
    assert_ne!(recreated.system_fields, first.system_fields);
    // System identities cannot be transplanted to another record/zone.
    let wrong = batch(
        &mut s,
        &sc,
        "z",
        false,
        vec![save(input("b", 5), Some(recreated.system_fields))],
    );
    assert_eq!(code(&wrong[0]), ErrorCode::InvalidArguments);
}

/// K3/Q3.2: only encrypted e and the exact optional asset field are allowed.
#[test]
fn k3_types_fields_and_named_field_reads() {
    let (_dir, mut s) = cloud(Config::default());
    let sc = scope();
    create(&mut s, &sc, "z");
    for (index, (ty, asset)) in [
        ("MGActive", None),
        ("MGEpoch", None),
        ("MGEntity", Some("pa")),
        ("MGGeneration", Some("ma")),
        ("MGMediaRoot", None),
        ("MGBlob", Some("a")),
    ]
    .into_iter()
    .enumerate()
    {
        let mut record = RecordInput::encrypted(format!("r{index}"), ty, vec![0, 255, 1]);
        if let Some(field) = asset {
            record
                .fields
                .insert(field.into(), Field::Asset(Asset::synthetic(b"abc".to_vec())));
        }
        let full = put(&mut s, &sc, "z", record, None);
        let Reply::Record(only_e) = call(
            &mut s,
            &sc,
            Operation::Read {
                zone: "z".into(),
                name: full.name.clone(),
                fields: RequestedFields::Named(vec!["e".into()]),
            },
        )
        .unwrap() else {
            panic!("record")
        };
        assert_eq!(only_e.fields.len(), 1);
        assert_eq!(only_e.fields["e"], full.fields["e"]);
        assert_eq!(only_e.system_fields, full.system_fields);
        let Reply::Record(none) = call(
            &mut s,
            &sc,
            Operation::Read {
                zone: "z".into(),
                name: full.name.clone(),
                fields: RequestedFields::Named(vec!["not-a-field".into()]),
            },
        )
        .unwrap() else {
            panic!("record")
        };
        assert!(none.fields.is_empty());
        if let Some(field) = asset {
            let Field::Asset(a) = &full.fields[field] else {
                panic!("asset")
            };
            assert_eq!(a.size(), 3);
            assert_eq!(a.bytes(), b"abc");
            let Reply::Record(asset_only) = call(
                &mut s,
                &sc,
                Operation::Read {
                    zone: "z".into(),
                    name: full.name.clone(),
                    fields: RequestedFields::Named(vec![field.into()]),
                },
            )
            .unwrap() else {
                panic!("record")
            };
            assert_eq!(asset_only.fields.len(), 1);
            assert_eq!(asset_only.fields[field], full.fields[field]);
            assert!(!asset_only.fields.contains_key("e"));
            assert_eq!(
                a.sha256(),
                [
                    0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23,
                    0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad
                ]
            );
            assert_eq!(read(&mut s, &sc, "z", &full.name), full);
        }
        let base = RecordInput::encrypted("bad", ty, vec![1]);
        let mut invalid = vec![];
        let mut v = base.clone();
        v.fields.insert("extra".into(), Field::Text("opaque-marker".into()));
        invalid.push(v);
        let mut v = base.clone();
        v.fields.insert("e".into(), Field::Bytes(vec![1]));
        invalid.push(v);
        let mut v = base.clone();
        v.fields.clear();
        invalid.push(v);
        let mut v = base.clone();
        v.fields
            .insert("wrong-asset".into(), Field::Asset(Asset::synthetic(vec![2])));
        invalid.push(v);
        let mut v = base.clone();
        v.fields.insert("e".into(), Field::Integer(1));
        invalid.push(v);
        if let Some(field) = asset {
            let mut v = base;
            v.fields.insert(field.into(), Field::EncryptedBytes(vec![1]));
            invalid.push(v);
        }
        for v in invalid {
            assert_eq!(
                code(&batch(&mut s, &sc, "z", false, vec![save(v, None)])[0]),
                ErrorCode::InvalidArguments
            );
        }
    }
    let unknown = RecordInput::encrypted("bad", "MGUnknown", vec![1]);
    assert_eq!(
        code(&batch(&mut s, &sc, "z", true, vec![save(unknown, None)])[0]),
        ErrorCode::InvalidArguments
    );
}

/// Q3.2/A8: conflicts, invalid fields and per-record limits roll back atomic work.
#[test]
fn atomic_mixed_failures_rollback_and_nonatomic_preserves_survivors() {
    let (_dir, mut s) = cloud(Config::default());
    let sc = scope();
    create(&mut s, &sc, "z");
    put(&mut s, &sc, "z", input("a", 1), None);
    let mut invalid = input("bad", 1);
    invalid.fields.insert("extra".into(), Field::Integer(3));
    let cases = [
        save(input("a", 2), None),
        save(invalid, None),
        Mutation::Delete { name: "missing".into() },
    ];
    for (index, failure) in cases.into_iter().enumerate() {
        let name = format!("new{index}");
        let before = s.inspect(&sc).unwrap();
        let changes = vec![
            Mutation::Delete { name: "a".into() },
            save(input(&name, 7), None),
            failure.clone(),
        ];
        // Avoid a duplicate-name request when the failure is the save over a.
        let changes = if index == 0 {
            vec![save(input(&name, 7), None), failure.clone()]
        } else {
            changes
        };
        let r = batch(&mut s, &sc, "z", true, changes);
        assert!(r.iter().all(|r| r.result.is_err()));
        assert_eq!(code(&r[0]), ErrorCode::BatchRequestFailed);
        assert_eq!(s.inspect(&sc).unwrap(), before);
        let r = batch(&mut s, &sc, "z", false, vec![save(input(&name, 7), None), failure]);
        assert!(r[0].result.is_ok());
        assert!(r[1].result.is_err());
        assert_eq!(
            read(&mut s, &sc, "z", &name).fields["e"],
            Field::EncryptedBytes(vec![7])
        );
    }
    let before = s.inspect(&sc).unwrap();
    let duplicate = call(
        &mut s,
        &sc,
        Operation::Modify {
            zone: "z".into(),
            atomic: false,
            changes: vec![save(input("dup", 1), None), save(input("dup", 2), None)],
        },
    )
    .unwrap_err();
    assert_eq!(duplicate.code, ErrorCode::InvalidArguments);
    assert_eq!(s.inspect(&sc).unwrap(), before);
}

/// Q3.2/A1/A1.1/A2 limits.
#[test]
fn q3_limit_boundaries() {
    let (_dir, mut s) = cloud(Config::default());
    let sc = scope();
    create(&mut s, &sc, "z");
    // Q3 A1 wire fixture.
    assert_eq!(record_non_asset_bytes("z", &input("a", 1)), 114);
    let mut cfg = Config::default();
    cfg.limits.records_per_request = 2;
    cfg.limits.non_asset_per_request = 228;
    configure(&mut s, &sc, cfg.clone());
    let exact = batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![save(input("a", 1), None), save(input("b", 2), None)],
    );
    assert!(exact.iter().all(|r| r.result.is_ok()));
    let old = saved(&exact[0]);
    let before = s.inspect(&sc).unwrap();
    let over_count = call(
        &mut s,
        &sc,
        Operation::Modify {
            zone: "z".into(),
            atomic: false,
            changes: vec![
                save(input("c", 1), None),
                save(input("d", 1), None),
                save(input("f", 1), None),
            ],
        },
    );
    assert_eq!(over_count.unwrap_err().code, ErrorCode::LimitExceeded);
    let mut larger = input("a", 3);
    larger.fields.insert("e".into(), Field::EncryptedBytes(vec![3, 3]));
    let over_bytes = call(
        &mut s,
        &sc,
        Operation::Modify {
            zone: "z".into(),
            atomic: true,
            changes: vec![
                save(larger.clone(), Some(old.system_fields.clone())),
                save(input("c", 1), None),
            ],
        },
    );
    assert_eq!(over_bytes.unwrap_err().code, ErrorCode::LimitExceeded);
    assert_eq!(s.inspect(&sc).unwrap(), before);
    cfg.limits.non_asset_per_request = 1000;
    cfg.limits.non_asset_per_record = 114;
    configure(&mut s, &sc, cfg.clone());
    let before = s.inspect(&sc).unwrap();
    let r = batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![
            save(input("c", 1), None),
            save(larger.clone(), Some(old.system_fields.clone())),
        ],
    );
    assert_eq!(code(&r[0]), ErrorCode::BatchRequestFailed);
    assert_eq!(code(&r[1]), ErrorCode::LimitExceeded);
    assert_eq!(s.inspect(&sc).unwrap(), before);
    let r = batch(
        &mut s,
        &sc,
        "z",
        false,
        vec![save(input("c", 1), None), save(larger, Some(old.system_fields.clone()))],
    );
    assert!(r[0].result.is_ok());
    assert_eq!(code(&r[1]), ErrorCode::LimitExceeded);
    assert_eq!(read(&mut s, &sc, "z", "a"), old);
    cfg.limits.non_asset_per_record = 1000;
    cfg.limits.asset_bytes = 3;
    configure(&mut s, &sc, cfg);
    let mut with_asset = input("v", 1);
    with_asset
        .fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(vec![4; 3])));
    assert_eq!(record_non_asset_bytes("z", &with_asset), 165);
    let old_asset = put(&mut s, &sc, "z", with_asset.clone(), None);
    with_asset
        .fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(vec![4; 4])));
    let before = s.inspect(&sc).unwrap();
    let r = batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![save(with_asset, Some(old_asset.system_fields.clone()))],
    );
    assert_eq!(code(&r[0]), ErrorCode::LimitExceeded);
    assert_eq!(s.inspect(&sc).unwrap(), before);
    assert_eq!(read(&mut s, &sc, "z", "v"), old_asset);
    // A bad configuration is itself a logged refusal, preserving old settings.
    let config = s.config(&sc).unwrap();
    assert_eq!(
        call(
            &mut s,
            &sc,
            Operation::Configure(Config {
                page_size: 201,
                ..Config::default()
            })
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArguments
    );
    assert_eq!(s.config(&sc).unwrap(), config);
}

/// Q3.2: account total includes assets and every zone; replacement failures keep
/// the previous record, deletes free space, atomic quota failures undo deletes.
#[test]
fn account_quota_boundaries_deletions_and_atomicity() {
    let (_dir, mut s) = cloud(Config::default());
    let sc = scope();
    create(&mut s, &sc, "z");
    create(&mut s, &sc, "w");
    quota(&mut s, &sc, Some(228));
    let a = put(&mut s, &sc, "z", input("a", 1), None);
    let r = batch(
        &mut s,
        &sc,
        "w",
        false,
        vec![save(input("b", 2), None), save(input("c", 3), None)],
    );
    assert!(r[0].result.is_ok());
    assert_eq!(code(&r[1]), ErrorCode::QuotaExceeded);
    assert_eq!(s.inspect(&sc).unwrap().unwrap().used_bytes, 228);
    let mut larger = input("a", 4);
    larger.fields.insert("e".into(), Field::EncryptedBytes(vec![4, 4]));
    let r = batch(
        &mut s,
        &sc,
        "z",
        false,
        vec![save(larger, Some(a.system_fields.clone()))],
    );
    assert_eq!(code(&r[0]), ErrorCode::QuotaExceeded);
    assert_eq!(read(&mut s, &sc, "z", "a"), a);
    quota(&mut s, &sc, Some(100)); // lowering quota never evicts existing records
    assert_eq!(s.inspect(&sc).unwrap().unwrap().used_bytes, 228);
    let before = s.inspect(&sc).unwrap();
    let r = batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![Mutation::Delete { name: "a".into() }, save(input("d", 5), None)],
    );
    assert_eq!(code(&r[0]), ErrorCode::BatchRequestFailed);
    assert_eq!(code(&r[1]), ErrorCode::QuotaExceeded);
    assert_eq!(s.inspect(&sc).unwrap(), before);
    assert!(delete(&mut s, &sc, "z", "a").result.is_ok());
    assert_eq!(s.inspect(&sc).unwrap().unwrap().used_bytes, 114);
    quota(&mut s, &sc, Some(228));
    put(&mut s, &sc, "z", input("d", 5), None);
    call(
        &mut s,
        &sc,
        Operation::DeleteZone {
            zone: "w".into(),
            cause: RemovalCause::Deleted,
        },
    )
    .unwrap();
    assert_eq!(s.inspect(&sc).unwrap().unwrap().used_bytes, 114);
    let mut asset = input("f", 6);
    asset
        .fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(vec![9; 3])));
    // 114 existing + 165 metadata + 3 asset bytes = 282.
    quota(&mut s, &sc, Some(281));
    assert_eq!(
        code(&batch(&mut s, &sc, "z", false, vec![save(asset.clone(), None)])[0]),
        ErrorCode::QuotaExceeded
    );
    quota(&mut s, &sc, Some(282));
    put(&mut s, &sc, "z", asset, None);
    assert_eq!(s.inspect(&sc).unwrap().unwrap().used_bytes, 282);
    assert!(delete(&mut s, &sc, "z", "f").result.is_ok());
    assert_eq!(s.inspect(&sc).unwrap().unwrap().used_bytes, 114);
    quota(&mut s, &sc, None);
    put(&mut s, &sc, "z", input("q", 8), None);
}

/// Q3.2/Q3.7: latest changes coalesce per record; snapshot pagination does not
/// skip concurrent-interleaved saves/deletions/recreations in the next fetch.
#[test]
fn persistent_record_logs_and_interleaved_snapshot_pages() {
    let (_dir, mut s) = cloud(Config {
        page_size: 1,
        ..Config::default()
    });
    let sc = scope();
    create(&mut s, &sc, "z");
    let a1 = put(&mut s, &sc, "z", input("a", 1), None);
    put(&mut s, &sc, "z", input("b", 1), None);
    put(&mut s, &sc, "z", input("c", 1), None);
    let mut p = zone_page(&mut s, &sc, "z", None);
    let mut initial = p.changes.len();
    while p.more {
        p = zone_page(&mut s, &sc, "z", Some(p.token));
        initial += p.changes.len();
    }
    assert_eq!(initial, 3);
    let baseline = p.token;
    let a2 = put(&mut s, &sc, "z", input("a", 2), Some(a1.system_fields));
    let a3 = put(&mut s, &sc, "z", input("a", 3), Some(a2.system_fields));
    assert!(delete(&mut s, &sc, "z", "b").result.is_ok());
    let d1 = put(&mut s, &sc, "z", input("d", 1), None);
    let first = zone_page(&mut s, &sc, "z", Some(baseline));
    assert!(first.more);
    assert_eq!(first.changes, vec![RecordChange::Saved(a3)]);
    let d2 = put(&mut s, &sc, "z", input("d", 2), Some(d1.system_fields.clone()));
    let b2 = put(&mut s, &sc, "z", input("b", 2), None);
    assert!(delete(&mut s, &sc, "z", "c").result.is_ok());
    let second = zone_page(&mut s, &sc, "z", Some(first.token));
    assert!(second.more);
    assert!(matches!(&second.changes[0], RecordChange::Deleted { name, .. } if name == "b"));
    let third = zone_page(&mut s, &sc, "z", Some(second.token));
    assert!(!third.more);
    assert_eq!(third.changes, vec![RecordChange::Saved(d1)]);
    let mut next = zone_page(&mut s, &sc, "z", Some(third.token));
    let mut changes = next.changes.clone();
    while next.more {
        next = zone_page(&mut s, &sc, "z", Some(next.token));
        changes.extend(next.changes.clone());
    }
    assert_eq!(
        changes,
        vec![
            RecordChange::Saved(b2),
            RecordChange::Deleted {
                name: "c".into(),
                record_type: "MGEntity".into()
            },
            RecordChange::Saved(d2)
        ]
    );
    assert!(zone_page(&mut s, &sc, "z", Some(next.token)).changes.is_empty());
    let db = s.inspect(&sc).unwrap().unwrap();
    let log = &db.zones["z"].changes;
    assert_eq!(log.len(), 10);
    assert!(log.windows(2).all(|pair| pair[0].sequence < pair[1].sequence));
    assert_eq!(
        log.iter()
            .filter(|r| matches!(&r.change, RecordChange::Deleted { .. }))
            .count(),
        2
    );
}

/// Q3.2: fresh database fetch lists live zones; database pages are snapshots and
/// report subsequent removals with their retained causes.
#[test]
fn database_change_pages_and_fresh_empty_zones() {
    let (_dir, mut s) = cloud(Config {
        page_size: 1,
        ..Config::default()
    });
    let sc = scope();
    create(&mut s, &sc, "az");
    create(&mut s, &sc, "bz");
    assert!(zone_page(&mut s, &sc, "az", None).changes.is_empty());
    let first = db_page(&mut s, &sc, None);
    assert_eq!(
        first.changes,
        vec![ZoneChange {
            zone: "az".into(),
            removed: None
        }]
    );
    assert!(first.more);
    call(
        &mut s,
        &sc,
        Operation::DeleteZone {
            zone: "bz".into(),
            cause: RemovalCause::Purged,
        },
    )
    .unwrap();
    create(&mut s, &sc, "cz");
    let second = db_page(&mut s, &sc, Some(first.token));
    assert_eq!(
        second.changes,
        vec![ZoneChange {
            zone: "bz".into(),
            removed: None
        }]
    );
    assert!(!second.more);
    let delta = db_page(&mut s, &sc, Some(second.token));
    assert_eq!(
        delta.changes,
        vec![ZoneChange {
            zone: "bz".into(),
            removed: Some(RemovalCause::Purged)
        }]
    );
    let delta_end = db_page(&mut s, &sc, Some(delta.token));
    assert_eq!(
        delta_end.changes,
        vec![ZoneChange {
            zone: "cz".into(),
            removed: None
        }]
    );
    assert!(!delta_end.more);
    create(&mut s, &sc, "bz");
    let recreated = db_page(&mut s, &sc, Some(delta_end.token));
    assert_eq!(
        recreated.changes,
        vec![ZoneChange {
            zone: "bz".into(),
            removed: None
        }]
    );
    assert_eq!(s.inspect(&sc).unwrap().unwrap().changes.len(), 5);
    assert_eq!(
        call(
            &mut s,
            &sc,
            Operation::FetchZone {
                zone: "az".into(),
                token: Some(recreated.token)
            }
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidArguments
    );
}

/// Q3.6/Q3.7: identical injected seed and public request program yield identical
/// values, version tags, tokens, outcomes and metadata logs. No sleep/time IO.
#[test]
fn deterministic_identical_seed_and_request_program() {
    let (dir_a, mut a) = cloud(Config {
        page_size: 1,
        ..Config::default()
    });
    let (_dir_b, mut b) = cloud(Config {
        page_size: 1,
        ..Config::default()
    });
    let sc = scope();
    let mut program = vec![
        Operation::CreateZone { zone: "z".into() },
        Operation::SetQuota { bytes: Some(500) },
    ];
    for name in ["a", "b"] {
        program.push(Operation::Modify {
            zone: "z".into(),
            atomic: true,
            changes: vec![save(input(name, 1), None)],
        });
    }
    program.extend([
        Operation::Modify {
            zone: "z".into(),
            atomic: false,
            changes: vec![save(input("a", 3), None), save(input("c", 1), None)],
        },
        Operation::Read {
            zone: "z".into(),
            name: "missing".into(),
            fields: RequestedFields::All,
        },
        Operation::FetchZone {
            zone: "z".into(),
            token: None,
        },
    ]);
    let caller = Caller {
        device: Some("fixture-device".into()),
        instance: Some("fixture-instance".into()),
        own_operation: None,
    };
    for op in program {
        assert_eq!(
            a.request(&sc, caller.clone(), op.clone()).unwrap(),
            b.request(&sc, caller.clone(), op).unwrap()
        );
    }
    // Reopen one side without resetting its source. The run remains replayable.
    let source = a.into_random();
    let mut a = Service::open(dir_a.path(), Config::default(), source).unwrap();
    let token = match a.outcomes().unwrap().last().unwrap().result.as_ref().unwrap() {
        ReplyReference::ZoneChanges(p) => p.token.clone(),
        _ => panic!("page"),
    };
    assert_eq!(
        a.request(
            &sc,
            caller.clone(),
            Operation::FetchZone {
                zone: "z".into(),
                token: Some(token.clone())
            }
        )
        .unwrap(),
        b.request(
            &sc,
            caller.clone(),
            Operation::FetchZone {
                zone: "z".into(),
                token: Some(token)
            }
        )
        .unwrap()
    );
    let old = read(&mut a, &sc, "z", "a");
    assert_eq!(old, read(&mut b, &sc, "z", "a"));
    let op = Operation::Modify {
        zone: "z".into(),
        atomic: true,
        changes: vec![save(input("a", 4), Some(old.system_fields))],
    };
    assert_eq!(
        a.request(&sc, caller.clone(), op.clone()).unwrap(),
        b.request(&sc, caller, op).unwrap()
    );
    assert_eq!(a.inspect(&sc).unwrap(), b.inspect(&sc).unwrap());
    let logs = a.request_log().unwrap();
    assert_eq!(logs, b.request_log().unwrap());
    assert_eq!(a.outcomes().unwrap(), b.outcomes().unwrap());
    assert!(
        logs.iter()
            .enumerate()
            .all(|(index, log)| log.request_id == index as u64 + 1)
    );
    assert_eq!(
        logs[4].records[0].outcome,
        LogOutcome::Failure(ErrorCode::ServerRecordChanged)
    );
    assert_eq!(logs[4].records[1].outcome, LogOutcome::Success);
    assert_eq!(logs[5].outcome, LogOutcome::Failure(ErrorCode::UnknownItem));
    assert_eq!(logs[2].records[0].non_asset_bytes, 114);
    assert_eq!(logs[2].caller.device.as_deref(), Some("fixture-device"));
}

/// Q3.7: diagnostics cannot expose opaque values or fixture identifiers.
#[test]
fn debug_and_errors_are_content_free() {
    let (_dir, mut s) = cloud(Config::default());
    let sc = scope();
    create(&mut s, &sc, "z");
    let marker = "private-fixture-marker";
    let mut i = RecordInput::encrypted(marker, "MGEntity", marker.as_bytes().to_vec());
    i.fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(marker.as_bytes().to_vec())));
    let r = put(&mut s, &sc, "z", i.clone(), None);
    let conflict = batch(&mut s, &sc, "z", true, vec![save(i.clone(), None)])
        .remove(0)
        .result
        .unwrap_err();
    let debug = format!(
        "{i:?} {r:?} {conflict:?} {:?} {:?} {:?}",
        sc,
        s.request_log().unwrap(),
        s.outcomes().unwrap()
    );
    for sensitive in [marker, "fixture-container", "acct-a", "MGEntity"] {
        assert!(!debug.contains(sensitive));
    }
    assert_eq!(conflict.code, ErrorCode::ServerRecordChanged);
    assert!(conflict.server_record.is_some());
}

fn owned_saved(mut results: Vec<MutationResult>) -> Record {
    assert_eq!(results.len(), 1);
    match results.remove(0).result.unwrap() {
        MutationSuccess::Saved(record) => record,
        MutationSuccess::Deleted => panic!("save"),
    }
}
fn asset_input(name: &str, bytes: usize) -> RecordInput {
    let mut record = input(name, 1);
    record
        .fields
        .insert("pa".into(), Field::Asset(Asset::synthetic(vec![90; bytes])));
    record
}
fn take_tag(record: Record) -> SystemFields {
    record.system_fields
}
fn assert_small_persisted_metadata(dir: &TempDir) {
    let c = rusqlite::Connection::open(dir.path().join("fake-cloud.db")).unwrap();
    let (log, outcome): (i64, i64) = c
        .query_row(
            "SELECT coalesce(max(length(log)),0),coalesce(max(length(outcome)),0) FROM requests",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(log < 8192);
    assert!(outcome < 8192);
    let bad_refs: i64 = c
        .query_row(
            "SELECT count(*) FROM versions WHERE length(encrypted)!=32 OR (asset IS NOT NULL AND length(asset)!=32)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(bad_refs, 0);
}
fn asset_size(record: &Record) -> u64 {
    match &record.fields["pa"] {
        Field::Asset(a) => a.size(),
        _ => panic!("asset"),
    }
}

/// Q3 storage: repeated outcomes and pages share bodies with immutable versions.
#[test]
fn q3_payload_retention_reopen_and_conditional_pages() {
    const BYTES: usize = 256 * 1024;
    let (dir, mut s) = cloud(Config {
        page_size: 1,
        ..Config::default()
    });
    let sc = scope();
    create(&mut s, &sc, "z");
    let initial = owned_saved(batch(&mut s, &sc, "z", true, vec![save(asset_input("a", BYTES), None)]));
    assert_eq!(asset_size(&initial), BYTES as u64);
    let original_tag = take_tag(initial);
    let mut tag = original_tag.clone();
    let b = put(&mut s, &sc, "z", input("b", 2), None);
    let captured = zone_page(&mut s, &sc, "z", None);
    assert!(captured.more);
    assert!(matches!(&captured.changes[0],RecordChange::Saved(r) if r.system_fields == tag));
    let captured_token = captured.token;
    let base = s.storage_stats().unwrap();
    assert_eq!(base.payload_bytes, BYTES as u64 + 2);
    assert_eq!(base.payloads, 3);
    let newest_b = put(&mut s, &sc, "z", input("b", 3), Some(b.system_fields));
    for _ in 0..16 {
        let record = owned_saved(batch(
            &mut s,
            &sc,
            "z",
            true,
            vec![save(asset_input("a", BYTES), Some(tag.clone()))],
        ));
        assert_ne!(record.system_fields, tag);
        tag = take_tag(record);
        for _ in 0..3 {
            let page = zone_page(&mut s, &sc, "z", None);
            assert!(page.more);
            assert!(matches!(&page.changes[0],RecordChange::Saved(r) if asset_size(r)==BYTES as u64));
            let last = zone_page(&mut s, &sc, "z", Some(page.token));
            assert!(!last.more);
        }
        let stats = s.storage_stats().unwrap();
        assert_eq!(stats.payload_bytes, BYTES as u64 + 3);
        assert_eq!(stats.payloads, 4);
    }
    let retained = s.storage_stats().unwrap();
    assert_eq!(retained.versions, 19);
    assert_eq!(retained.record_changes, 19);
    assert_eq!(retained.current_records, 2);
    assert!(retained.memberships > retained.versions);
    // Reset injected entropy deliberately: persisted version identity still advances.
    drop(s);
    let mut s = Service::open(dir.path(), Config::default(), SeededRandom::new(23)).unwrap();
    assert_eq!(s.storage_stats().unwrap(), retained);
    let last = zone_page(&mut s, &sc, "z", Some(captured_token.clone()));
    assert!(!last.more);
    assert!(matches!(&last.changes[0],RecordChange::Saved(r) if r.fields["e"]==Field::EncryptedBytes(vec![2])));
    let delta = zone_page(&mut s, &sc, "z", Some(last.token));
    assert!(delta.more);
    assert!(matches!(&delta.changes[0],RecordChange::Saved(r) if r.system_fields==tag));
    let last = zone_page(&mut s, &sc, "z", Some(delta.token));
    assert_eq!(last.changes, vec![RecordChange::Saved(newest_b)]);
    let record = owned_saved(batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![save(asset_input("a", BYTES), Some(tag.clone()))],
    ));
    assert_ne!(record.system_fields, tag);
    let latest_tag = take_tag(record);
    let conflict = batch(&mut s, &sc, "z", true, vec![save(input("a", 4), Some(original_tag))])
        .remove(0)
        .result
        .unwrap_err();
    assert_eq!(conflict.code, ErrorCode::ServerRecordChanged);
    assert_eq!(conflict.server_record.unwrap().system_fields, latest_tag);
    let final_stats = s.storage_stats().unwrap();
    assert_eq!(final_stats.payload_bytes, retained.payload_bytes);
    assert_eq!(final_stats.versions, retained.versions + 1);
    let replay = zone_page(&mut s, &sc, "z", Some(captured_token));
    assert!(matches!(&replay.changes[0],RecordChange::Saved(r) if r.fields["e"]==Field::EncryptedBytes(vec![2])));
    assert_small_persisted_metadata(&dir);
    let page = s.outcomes_page(0, 2).unwrap();
    assert_eq!(page.len(), 2);
    let reference = match page[1].result.as_ref().unwrap() {
        ReplyReference::Modified(items) => items[0].result.as_ref().unwrap().as_ref().unwrap(),
        _ => panic!("save reference"),
    };
    let original = s.resolve_record(&sc, reference).unwrap();
    assert_eq!(asset_size(&original), BYTES as u64);
    let mut other = sc.clone();
    other.account = "other".into();
    assert!(s.resolve_record(&other, reference).is_err());
}

/// Q3/A2: the actual maximum asset fits one raw SQLite value.
#[test]
fn q3_full_allowed_asset_uses_one_body() {
    let (dir, mut s) = cloud(Config::default());
    let sc = scope();
    create(&mut s, &sc, "z");
    let bytes = Limits::default().asset_bytes;
    assert_eq!(bytes, 100 * 1024 * 1024);
    let result = owned_saved(batch(
        &mut s,
        &sc,
        "z",
        true,
        vec![save(asset_input("large", bytes as usize), None)],
    ));
    assert_eq!(asset_size(&result), bytes);
    let tag = take_tag(result);
    let stats = s.storage_stats().unwrap();
    assert_eq!(stats.payload_bytes, bytes + 1);
    assert_eq!(stats.payloads, 2);
    assert_eq!(stats.versions, 1);
    assert_eq!(s.inspect(&sc).unwrap_err(), StorageError::InspectionLimit);
    drop(s);
    let mut s = Service::open(dir.path(), Config::default(), SeededRandom::new(23)).unwrap();
    let page = zone_page(&mut s, &sc, "z", None);
    assert!(!page.more);
    assert!(matches!(&page.changes[0],RecordChange::Saved(r) if asset_size(r)==bytes && r.system_fields==tag));
    let after = s.storage_stats().unwrap();
    assert_eq!(after.payload_bytes, stats.payload_bytes);
    assert_eq!(after.payloads, stats.payloads);
    assert_eq!(after.versions, 1);
    assert_small_persisted_metadata(&dir);
    assert!(matches!(
        &s.outcomes().unwrap()[1].result,
        Ok(ReplyReference::Modified(_))
    ));
}

/// Q3 storage: rows scale with versions; reads and pages add no record bodies.
#[test]
fn q3_many_requests_keep_normalized_rows() {
    const COUNT: usize = 1200;
    let (dir, mut s) = cloud(Config {
        page_size: 73,
        ..Config::default()
    });
    let sc = scope();
    create(&mut s, &sc, "z");
    for index in 0..COUNT {
        let name = format!("r{index:04}");
        let record = owned_saved(batch(&mut s, &sc, "z", true, vec![save(input(&name, 1), None)]));
        assert_eq!(record.name, name);
    }
    let base = s.storage_stats().unwrap();
    assert_eq!(base.versions, COUNT as u64);
    assert_eq!(base.current_records, COUNT as u64);
    assert_eq!(base.record_changes, COUNT as u64);
    assert_eq!(base.payloads, 1);
    assert_eq!(base.payload_bytes, 1);
    assert_eq!(base.requests, COUNT as u64 + 1);
    assert_eq!(s.inspect(&sc).unwrap_err(), StorageError::InspectionLimit);
    let first = zone_page(&mut s, &sc, "z", None);
    assert!(first.more);
    let mut seen = first.changes.len();
    let source = s.into_random();
    let mut s = Service::open(dir.path(), Config::default(), source).unwrap();
    let mut page = zone_page(&mut s, &sc, "z", Some(first.token));
    loop {
        for change in &page.changes {
            assert!(matches!(change,RecordChange::Saved(r) if r.name==format!("r{seen:04}")));
            seen += 1;
        }
        if !page.more {
            break;
        }
        page = zone_page(&mut s, &sc, "z", Some(page.token));
    }
    assert_eq!(seen, COUNT);
    assert!(zone_page(&mut s, &sc, "z", Some(page.token)).changes.is_empty());
    for index in 0..COUNT {
        assert_eq!(
            read(&mut s, &sc, "z", &format!("r{index:04}")).fields["e"],
            Field::EncryptedBytes(vec![1])
        );
    }
    let after = s.storage_stats().unwrap();
    assert_eq!(after.versions, base.versions);
    assert_eq!(after.current_records, base.current_records);
    assert_eq!(after.record_changes, base.record_changes);
    assert_eq!(after.payload_bytes, 1);
    assert_eq!(after.memberships, COUNT as u64);
    assert_eq!(s.outcomes().unwrap_err(), StorageError::InspectionLimit);
    assert_eq!(s.request_log().unwrap_err(), StorageError::InspectionLimit);
    let mut cursor = 0;
    let mut inspected = 0;
    loop {
        let outcomes = s.outcomes_page(cursor, 113).unwrap();
        if outcomes.is_empty() {
            break;
        }
        for outcome in &outcomes {
            assert!(outcome.request_id > cursor);
            cursor = outcome.request_id;
            inspected += 1;
        }
    }
    assert_eq!(inspected, after.requests);
}

/// Q3 storage identity: unrelated and previous layouts are left intact.
#[test]
fn q3_unexpected_database_format_is_refused() {
    for sql in [
        "CREATE TABLE unrelated(value TEXT)",
        "CREATE TABLE fake_service(id INTEGER PRIMARY KEY,state BLOB)",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake-cloud.db");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(sql).unwrap();
        drop(c);
        let before = std::fs::read(&path).unwrap();
        assert!(Service::open(dir.path(), Config::default(), SeededRandom::new(23)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fake-cloud.db");
    std::fs::write(&path, []).unwrap();
    assert!(Service::open(dir.path(), Config::default(), SeededRandom::new(23)).is_err());
    assert!(std::fs::read(path).unwrap().is_empty());
}

#[test]
fn q3_invalid_sql_integers_are_rejected() {
    for sql in [
        "UPDATE databases SET used=-1",
        "UPDATE databases SET used=9223372036854775808.0",
        "UPDATE record_changes SET version=-1",
    ] {
        let (dir, mut s) = cloud(Config::default());
        let sc = scope();
        create(&mut s, &sc, "z");
        put(&mut s, &sc, "z", input("a", 1), None);
        let c = rusqlite::Connection::open(dir.path().join("fake-cloud.db")).unwrap();
        c.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        c.execute_batch(sql).unwrap();
        assert_eq!(s.inspect(&sc), Err(StorageError::Database));
    }
    let (_dir, s) = cloud(Config::default());
    let reference = RecordReference {
        version: u64::MAX,
        fields: vec!["e".into()],
    };
    assert_eq!(s.resolve_record(&scope(), &reference), Err(StorageError::Exhausted));
    assert_eq!(s.outcomes_page(u64::MAX, 1), Err(StorageError::Exhausted));
    assert_eq!(s.request_log_page(u64::MAX, 1), Err(StorageError::Exhausted));
}

#[test]
fn zone_list_requests_record_original_results() {
    let (_directory, mut service) = cloud(Config {
        page_size: 1,
        ..Config::default()
    });
    let scope = scope();
    let mut other = scope.clone();
    other.account = "other-account".into();
    create(&mut service, &scope, "z");
    create(&mut service, &scope, "a");
    create(&mut service, &other, "foreign");
    call(
        &mut service,
        &scope,
        Operation::DeleteZone {
            zone: "z".into(),
            cause: RemovalCause::Deleted,
        },
    )
    .unwrap();
    let caller = Caller {
        own_operation: Some("all-zone-inventory".into()),
        ..Caller::default()
    };
    let original = service
        .request(&scope, caller.clone(), Operation::FetchAllZones)
        .unwrap();
    assert_eq!(original.result, Ok(Reply::AllZones(vec!["a".into()])));
    create(&mut service, &scope, "z");
    let fresh = service
        .request(&scope, caller.clone(), Operation::FetchAllZones)
        .unwrap();
    assert_eq!(fresh.result, Ok(Reply::AllZones(vec!["a".into(), "z".into()])));
    let log = service.request_log().unwrap();
    let entry = log.iter().find(|e| e.request_id == original.request_id).unwrap();
    assert_eq!(entry.kind, RequestKind::FetchAllZones);
    assert_eq!(entry.caller, caller);
    assert_eq!(entry.zone, None);
    let outcome = service
        .outcomes()
        .unwrap()
        .into_iter()
        .find(|e| e.request_id == original.request_id)
        .unwrap();
    assert_eq!(outcome.result, Ok(ReplyReference::AllZones(vec!["a".into()])));
}
