//! Real raw-row capture, reconciliation and policy projection through CoreHost.
use super::*;
use meguri_engine::{OpSource, OpSpec};
use meguri_storage::StampMode;
use meguri_sync::cloud::{policy::{Attention, Lifecycle}, row_payload::RowFailure};

fn config_row(key: &str, readable: bool) -> Vec<Cell> {
    vec![Cell::text(key), Cell::Integer(-1), if readable { Cell::Integer(0) } else { Cell::text("bad timestamp") }, Cell::Blob(b"1".to_vec())]
}
fn restore(rig: &Rig, table: Table, key: RowKey, row: Option<Vec<Cell>>) {
    rig.host.collection().unwrap().run(
        OpSpec::named("raw classification input", OpSource::SyncIncoming).stamp(StampMode::Raw),
        move |ctx| { ctx.scope().restore(table, &key, row.as_ref())?; Ok(()) },
    ).unwrap();
}
fn attention(rig: &Rig, item: Attention) -> u64 {
    let projection = rig.host.cloud_runtime().unwrap().policy_projection(Lifecycle::Ready, rig.host.engine().generation().0).unwrap();
    assert!(!projection.facts.summary.attention_known);
    projection.facts.summary.attention.get(&item).copied().unwrap_or(0)
}
fn scan(rig: &Rig) {
    rig.host.cloud_runtime().unwrap().reconcile_binding(&rig.host.collection().unwrap(), &rig.host.media().unwrap(), Duration::from_secs(10)).unwrap();
}
fn name(rig: &Rig, kind: codec::Kind, key: &[u8]) -> String {
    let binding = rig.host.cloud_runtime().unwrap().status().unwrap().binding.unwrap();
    codec::entity_name(&binding.facts.record_key.0, kind, key).unwrap()
}

#[test]
fn k10_rows_real_observer_unreadable_without_payload_reopen_unchanged_repair_and_delete() {
    let rig = rig();
    let key = "k10-row-unreadable";
    restore(&rig, Table::Config, RowKey::text(key), Some(config_row(key, false)));
    let cloud = rig.host.cloud_runtime().unwrap();
    let record = name(&rig, codec::Kind::Config, key.as_bytes());
    assert!(cloud.entity_facts(&record).unwrap().is_none(), "a failed row has no placeholder payload");
    let (_, details) = cloud.row_classification_details().unwrap();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0].kind, codec::Kind::Config);
    assert_eq!(details[0].key, key.as_bytes());
    assert!(matches!(details[0].failure, Some(RowFailure::Unreadable { table, column }) if table=="config" && column=="mtime_secs"));
    assert_eq!(attention(&rig, Attention::Unreadable), 1);
    scan(&rig);
    assert!(cloud.row_classification_details().unwrap().0);
    assert_eq!(attention(&rig, Attention::Unreadable), 1, "an unchanged bad row stays flagged");
    rig.host.close().unwrap();
    let reopened = CoreHost::open(rig.config.clone()).unwrap();
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.row_classification_details().unwrap().1.len(), 1);
    reopened.close().unwrap();
    // Use another actual Host open for accepted repair/deletion.
    let host = Arc::new(CoreHost::open(rig.config.clone()).unwrap());
    let repaired = Rig { host, ..rig };
    restore(&repaired, Table::Config, RowKey::text(key), Some(config_row(key, true)));
    assert_eq!(attention(&repaired, Attention::Unreadable), 0);
    restore(&repaired, Table::Config, RowKey::text(key), Some(config_row(key, false)));
    assert_eq!(attention(&repaired, Attention::Unreadable), 1);
    restore(&repaired, Table::Config, RowKey::text(key), None);
    assert_eq!(attention(&repaired, Attention::Unreadable), 0);
    repaired.host.close().unwrap();
}

#[test]
fn k10_rows_newer_local_repair_and_failure_overtake_real_snapshot() {
    use meguri_sync::cloud::runtime::ScanPausePoint;
    for bad_before in [false, true] {
        let rig = rig();
        let key = "k10-row-scan-race";
        restore(&rig, Table::Config, RowKey::text(key), Some(config_row(key, !bad_before)));
        let cloud = rig.host.cloud_runtime().unwrap();
        let (reached, wait) = std::sync::mpsc::channel();
        let (resume, go) = std::sync::mpsc::channel();
        cloud.arm_note_scan_pause_for_testing(ScanPausePoint::BeforeRowMerge(codec::Kind::Config), reached, go).unwrap();
        std::thread::scope(|threads| {
            let scan = threads.spawn(|| scan(&rig));
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            restore(&rig, Table::Config, RowKey::text(key), Some(config_row(key, bad_before)));
            resume.send(()).unwrap();
            scan.join().unwrap();
        });
        assert_eq!(attention(&rig, Attention::Unreadable), u64::from(!bad_before));
        rig.host.close().unwrap();
    }
}

#[test]
fn k10_rows_failed_observer_commit_keeps_stop_and_real_scan_recovers_lost_flag() {
    let rig = rig();
    let cloud = rig.host.cloud_runtime().unwrap();
    cloud.arm_capture_failure_for_testing(1).unwrap();
    let key = "k10-row-failed-commit";
    restore(&rig, Table::Config, RowKey::text(key), Some(config_row(key, false)));
    assert!(cloud.status().unwrap().tracking_stopped);
    assert!(cloud.status().unwrap().scan_required);
    assert!(cloud.row_classification_details().unwrap().1.is_empty());
    // The real collection commit stands. No flag/zero claim came from its
    // failed tracking transaction; a held-snapshot scan discovers the row.
    scan(&rig);
    assert_eq!(attention(&rig, Attention::Unreadable), 1);
    assert!(!cloud.status().unwrap().tracking_stopped);
    rig.host.close().unwrap();
}

#[test]
fn k10_rows_old_handed_ack_cannot_clear_new_failure_and_unrelated_send_progresses() {
    let rig = rig();
    let cloud = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    let key = "k10-row-flight";
    restore(&rig, Table::Config, RowKey::text(key), Some(config_row(key, true)));
    let record = name(&rig, codec::Kind::Config, key.as_bytes());
    let batch = cloud.build_collection_batch(&rig.sdk, &collection, &[record.clone()].into()).unwrap();
    assert_eq!(batch.saves.len(), 1);
    let prior = cloud.entity_facts(&record).unwrap().unwrap();
    assert_eq!(prior.in_flight.len(), 1);
    let saved = save_at_service(&rig, &batch);
    restore(&rig, Table::Config, RowKey::text(key), Some(config_row(key, false)));
    let flagged = cloud.entity_facts(&record).unwrap().unwrap();
    assert_eq!(flagged.payload, prior.payload);
    assert!(flagged.base_payload == prior.base_payload && flagged.in_flight == prior.in_flight);
    cloud.enter_sent_handler(&rig.sdk, batch.id).unwrap();
    cloud.sent_results(&rig.sdk, &collection, batch.id, &saved, &[], 1).unwrap();
    cloud.return_sent_handler(&rig.sdk).unwrap();
    assert_eq!(attention(&rig, Attention::Unreadable), 1);
    let after_ack=cloud.entity_facts(&record).unwrap().unwrap();
    assert!(after_ack.mark.is_some());
    assert!(after_ack.local_rev>prior.local_rev);
    assert!(after_ack.acked_rev<after_ack.local_rev);
    finish_bad_exchange(&rig,&record);
    assert!(cloud.build_collection_batch(&rig.sdk, &collection, &[record.clone()].into()).unwrap().saves.is_empty());
    let other = "k10-row-unrelated";
    restore(&rig, Table::Config, RowKey::text(other), Some(config_row(other, true)));
    let other_name = name(&rig, codec::Kind::Config, other.as_bytes());
    let next = cloud.build_collection_batch(&rig.sdk, &collection, &[record.clone(), other_name.clone()].into()).unwrap();
    assert_eq!(next.saves.iter().map(|r| &r.name).collect::<Vec<_>>(), vec![&other_name]);
    // Only actual readable repair can remove the later fact.
    restore(&rig, Table::Config, RowKey::text(key), Some(config_row(key, true)));
    assert_eq!(attention(&rig, Attention::Unreadable), 0);
    rig.host.close().unwrap();
}

#[test]
fn k10_rows_oversize_raw_observer_scan_reopen_shrink_never_sends_retained_payload() {
    let rig=rig();
    let key="k10-row-oversize";
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,true)));
    let cloud=rig.host.cloud_runtime().unwrap();
    let record=name(&rig,codec::Kind::Config,key.as_bytes());
    let before=cloud.entity_facts(&record).unwrap().unwrap();
    let mut raw=config_row(key,true);
    raw[3]=Cell::Blob(vec![0;codec::MAX_PAYLOAD_BYTES as usize]);
    restore(&rig,Table::Config,RowKey::text(key),Some(raw));
    assert_eq!(attention(&rig,Attention::Oversize),1);
    let (_,details)=cloud.row_classification_details().unwrap();
    assert_eq!(details[0].kind,codec::Kind::Config);
    assert_eq!(details[0].key,key.as_bytes());
    assert_eq!(details[0].failure,Some(RowFailure::Oversize));
    assert_eq!(cloud.entity_facts(&record).unwrap().unwrap().payload,before.payload);
    assert!(cloud.build_collection_batch(&rig.sdk,&rig.host.collection().unwrap(),&[record.clone()].into()).unwrap().saves.is_empty());
    scan(&rig);
    assert_eq!(attention(&rig,Attention::Oversize),1);
    rig.host.close().unwrap();
    let reopened=Arc::new(CoreHost::open(rig.config.clone()).unwrap());
    let rig=Rig { host:reopened,..rig };
    assert_eq!(attention(&rig,Attention::Oversize),1);
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,true)));
    assert_eq!(attention(&rig,Attention::Oversize),0);
    rig.host.close().unwrap();
}

#[test]
fn k10_rows_bad_note_holds_its_card_while_unrelated_config_sends() {
    let rig=rig_with_root(false);
    fetch_first_root(&rig);
    let cloud=rig.host.cloud_runtime().unwrap();
    let collection=rig.host.collection().unwrap();
    let note=cloud.entity_facts_page(codec::Kind::Note,None).unwrap().remove(0);
    let card=cloud.entity_facts_page(codec::Kind::Card,None).unwrap().into_iter().find(|e|
        matches!(&e.payload,Payload::Card(c) if codec::id_key(c.nid).as_slice()==note.key.as_slice())
    ).unwrap();
    let reader=collection.reader(ReaderRole::Query).unwrap();
    let codec::Payload::Note(n)=&note.payload else { panic!() };
    let codec::Payload::Card(c)=&card.payload else { panic!() };
    let mut raw_note=reader.row_image(Table::Notes,&RowKey::Id(n.id)).unwrap().unwrap();
    let mut raw_card=reader.row_image(Table::Cards,&RowKey::Id(c.id)).unwrap().unwrap();
    drop(reader);
    raw_note[6]=Cell::Blob(vec![0xff]);
    restore(&rig,Table::Notes,RowKey::Id(n.id),Some(raw_note));
    raw_card[8]=Cell::Integer(1234);
    restore(&rig,Table::Cards,RowKey::Id(c.id),Some(raw_card));
    let key="k10-card-independent";
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,true)));
    let independent=name(&rig,codec::Kind::Config,key.as_bytes());
    let batch=cloud.build_collection_batch(&rig.sdk,&collection,&[note.name.clone(),card.name.clone(),independent.clone()].into()).unwrap();
    assert_eq!(attention(&rig,Attention::Unreadable),1);
    assert_eq!(batch.saves.iter().map(|r| &r.name).collect::<Vec<_>>(),vec![&independent]);
    assert_eq!(cloud.entity_facts(&note.name).unwrap().unwrap().payload,note.payload);
    scan(&rig);
    assert_eq!(attention(&rig,Attention::Unreadable),1);
    rig.host.close().unwrap();
}

#[test]
fn k10_rows_v7_migration_unknown_requires_actual_complete_scan() {
    use meguri_sync::cloud::TRACKING_FILE;
    let rig=rig();
    rig.host.close().unwrap();
    let db=meguri_storage::rusqlite::Connection::open(rig.config.collection_directory.join(TRACKING_FILE)).unwrap();
    db.execute_batch("DROP TABLE cloud_row_classifications; DROP TABLE cloud_row_classification_coverage; UPDATE cloud_runtime_schema SET version=7;").unwrap();
    drop(db);
    let reopened=Arc::new(CoreHost::open(rig.config.clone()).unwrap());
    let rig=Rig{host:reopened,..rig};
    let cloud=rig.host.cloud_runtime().unwrap();
    // Open only migrates; neither retained whole coverage nor empty tables
    // supply this producer's classification coverage.
    assert!(!cloud.row_classification_details().unwrap().0);
    let key="k10-legacy-bad";
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,false)));
    assert!(!cloud.row_classification_details().unwrap().0);
    cloud.refresh_identity().unwrap();
    scan(&rig);
    assert!(cloud.row_classification_details().unwrap().0);
    assert_eq!(attention(&rig,Attention::Unreadable),1);
    rig.host.close().unwrap();
}

fn save_at_service(rig: &Rig, batch: &meguri_sync::cloud::workflow::BuiltBatch) -> Vec<Saved> {
    use meguri_testkit::cloud::{Caller, Field, Mutation, MutationSuccess, Operation, RecordInput, Reply, Scope, Service};
    let mut service = Service::open(rig._root.path(), meguri_testkit::cloud::Config::default(), meguri_model::rng::SeededRandom::new(3301)).unwrap();
    let scope = Scope { container: "synthetic-u114".into(), environment: "development".into(), account: "synthetic-account-A".into() };
    assert!(matches!(service.request(&scope, Caller::default(), Operation::CreateZone { zone: rig.zone.clone() }).unwrap().result, Ok(Reply::Done)));
    let result = service.request(&scope, Caller::default(), Operation::Modify {
        zone: rig.zone.clone(), atomic: false, changes: batch.saves.iter().map(|r| Mutation::Save {
            record: RecordInput::encrypted(&r.name, "MGEntity", r.envelope.clone()), if_unchanged: None,
        }).collect(),
    }).unwrap();
    let Ok(Reply::Modified(outcomes)) = result.result else { panic!("actual service save required"); };
    outcomes.into_iter().map(|outcome| {
        let Ok(MutationSuccess::Saved(record)) = outcome.result else { panic!("save required"); };
        let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else { panic!("envelope required"); };
        Saved { name: record.name.clone(), envelope: envelope.clone(), system_fields: record.system_fields.bytes().to_vec() }
    }).collect::<Vec<_>>()
}

fn finish_bad_exchange(rig: &Rig, record: &str) -> meguri_sync::cloud::adapter::CompletedExchange {
    let driver=rig.host.cloud_policy_driver().unwrap();
    let run=command(rig.host.cloud_policy_input(DeviceEvent::SyncNowRequested(Source::Command)).unwrap(),
        |k| matches!(k,ActionKind::RunExchange{..}));
    let ActionKind::RunExchange{exchange,..}=run.action.kind else { unreachable!() };
    let cloud=rig.host.cloud_runtime().unwrap();
    let native=driver.capture_exchange(&run,&rig.host.collection().unwrap()).unwrap();
    policy_fetch_snapshot(rig);
    let completed=cloud.adapter_complete_exchange(&rig.sdk,native,true,None).unwrap();
    assert!(completed.result.selected_names.contains(record));
    assert!(completed.result.blocked_names.contains(record));
    assert_eq!(completed.collection,meguri_sync::cloud::adapter::CollectionOutcome::Incomplete);
    let record=command(driver.complete_exchange(exchange,rig.host.engine().state(),rig.host.engine().generation()).unwrap(),
        |k| matches!(k,ActionKind::RecordExchange(_)));
    assert!(matches!(record.action.kind,ActionKind::RecordExchange(ref r) if matches!(r.outcome,ExchangeOutcome::Incomplete{..})));
    rig.host.execute_cloud_policy_core(&record,Duration::from_secs(10)).unwrap();
    assert_eq!(cloud.adapter_exchange_result(native).unwrap(),completed);
    completed
}

#[test]
fn k10_rows_new_payloadless_failure_is_in_original_exchange_selection_and_remains_incomplete() {
    let rig=rig();
    let key="k10-new-exchange";
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,false)));
    let cloud=rig.host.cloud_runtime().unwrap();
    let record=name(&rig,codec::Kind::Config,key.as_bytes());
    assert!(cloud.entity_facts(&record).unwrap().is_none());
    let original=finish_bad_exchange(&rig,&record);
    let original_names=original.result.selected_names.clone();
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,true)));
    assert_eq!(cloud.adapter_exchange_result(original.id).unwrap().result.selected_names,original_names);
    assert_eq!(cloud.adapter_exchange_result(original.id).unwrap().collection,meguri_sync::cloud::adapter::CollectionOutcome::Incomplete);
    rig.host.close().unwrap();
}

#[test]
fn k10_rows_previously_acked_failure_advances_revision_and_cannot_complete_exchange_as_clean() {
    let rig=rig();
    let key="k10-acked-exchange";
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,true)));
    let cloud=rig.host.cloud_runtime().unwrap();
    let collection=rig.host.collection().unwrap();
    let record=name(&rig,codec::Kind::Config,key.as_bytes());
    let batch=cloud.build_collection_batch(&rig.sdk,&collection,&[record.clone()].into()).unwrap();
    assert_eq!(batch.saves.len(),1);
    let saved=save_at_service(&rig,&batch);
    cloud.enter_sent_handler(&rig.sdk,batch.id).unwrap();
    cloud.sent_results(&rig.sdk,&collection,batch.id,&saved,&[],1).unwrap();
    cloud.return_sent_handler(&rig.sdk).unwrap();
    let acked=cloud.entity_facts(&record).unwrap().unwrap();
    assert!(acked.mark.is_none() && acked.server_erev>0);
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,false)));
    let failed=cloud.entity_facts(&record).unwrap().unwrap();
    assert_eq!(failed.local_rev,acked.local_rev+1);
    assert!(failed.mark.is_some());
    assert_eq!(failed.acked_rev,acked.acked_rev);
    assert_eq!(failed.payload,acked.payload);
    assert_eq!(failed.acked_payload,acked.acked_payload);
    scan(&rig);
    assert_eq!(cloud.entity_facts(&record).unwrap().unwrap().local_rev,failed.local_rev);
    finish_bad_exchange(&rig,&record);
    rig.host.close().unwrap();
}

#[test]
fn k10_rows_real_notetype_part_commit_classifies_and_repairs_parent() {
    for table in [Table::Fields,Table::Templates] {
        let rig=rig();
        let cloud=rig.host.cloud_runtime().unwrap();
        let collection=rig.host.collection().unwrap();
        let nt=cloud.entity_facts_page(codec::Kind::Notetype,None).unwrap().remove(0);
        let id=i64::from_be_bytes(nt.key.as_slice().try_into().unwrap());
        let reader=collection.reader(ReaderRole::Query).unwrap();
        let key=RowKey::Pair(id,0);
        let row=reader.row_image(table,&key).unwrap().unwrap();
        drop(reader);
        let mut bad=row.clone();
        bad[if table==Table::Fields {3} else {5}]=Cell::Blob(vec![0x80]);
        restore(&rig,table,key.clone(),Some(bad));
        let details=cloud.row_classification_details().unwrap().1;
        assert!(details.iter().any(|f| f.kind==codec::Kind::Notetype && f.key==nt.key && matches!(f.failure,
            Some(RowFailure::Unreadable{table:t,column:"config"}) if t==table.name())));
        assert_eq!(cloud.entity_facts(&nt.name).unwrap().unwrap().payload,nt.payload);
        assert!(cloud.build_collection_batch(&rig.sdk,&collection,&[nt.name.clone()].into()).unwrap().saves.is_empty());
        scan(&rig);
        assert_eq!(attention(&rig,Attention::Unreadable),1);
        restore(&rig,table,key,Some(row));
        assert_eq!(attention(&rig,Attention::Unreadable),0);
        rig.host.close().unwrap();
    }
}

#[test]
fn k10_rows_partial_scan_classification_failure_never_certifies_zero() {
    let rig=rig();
    let cloud=rig.host.cloud_runtime().unwrap();
    cloud.arm_capture_failure_for_testing(1).unwrap();
    let key="k10-scan-commit";
    restore(&rig,Table::Config,RowKey::text(key),Some(config_row(key,false)));
    assert!(cloud.status().unwrap().tracking_stopped);
    let db=meguri_storage::rusqlite::Connection::open(rig.config.collection_directory.join(meguri_sync::cloud::TRACKING_FILE)).unwrap();
    db.execute_batch("CREATE TRIGGER reject_scan_failure BEFORE INSERT ON cloud_row_classifications WHEN NEW.failure!=0 BEGIN SELECT RAISE(FAIL,'classification fault'); END;").unwrap();
    drop(db);
    assert!(cloud.reconcile_binding(&rig.host.collection().unwrap(),&rig.host.media().unwrap(),Duration::from_secs(10)).is_err());
    assert!(cloud.status().unwrap().tracking_stopped && cloud.status().unwrap().scan_required);
    assert!(!cloud.row_classification_details().unwrap().0);
    let db=meguri_storage::rusqlite::Connection::open(rig.config.collection_directory.join(meguri_sync::cloud::TRACKING_FILE)).unwrap();
    db.execute_batch("DROP TRIGGER reject_scan_failure").unwrap();
    drop(db);
    rig.host.close().unwrap();
}

#[test]
fn k10_rows_real_field_ordinal_bytes_survive_observer_scan_reopen_and_repair() {
    raw_ordinal_case(Table::Fields, meguri_storage::raw::RawValue::text("invalid ordinal"));
}

#[test]
fn k10_rows_real_template_ordinal_bytes_survive_observer_scan_reopen_and_repair() {
    raw_ordinal_case(Table::Templates, meguri_storage::raw::RawValue::Blob(vec![0, 1]));
}

fn raw_ordinal_case(table: Table, invalid: meguri_storage::raw::RawValue) {
    use meguri_storage::raw::{PlainDatabase, RawValue};
    let initial = rig_with_root(false);
    fetch_first_root(&initial);
    let original = initial.host.cloud_runtime().unwrap().entity_facts_page(codec::Kind::Notetype, None).unwrap().remove(0);
    let codec::Payload::Notetype(layout) = &original.payload else { panic!() };
    let id = layout.id;
    let path = initial.config.collection_directory.join("collection.anki2");
    initial.host.close().unwrap();
    let read_parts = || {
        let db = PlainDatabase::open(&path).unwrap();
        let mut rows = Vec::new();
        db.for_each(&format!("SELECT * FROM {} WHERE ntid=?1 ORDER BY ord", table.name()),
            &[RawValue::Integer(id)], |row| rows.push(row)).unwrap();
        db.close().unwrap();
        rows
    };
    let valid_parts = read_parts();
    let db = PlainDatabase::open(&path).unwrap();
    assert_eq!(db.execute(&format!("UPDATE {} SET ord=?1 WHERE ntid=?2 AND ord=0", table.name()),
        &[invalid.clone(), RawValue::Integer(id)]).unwrap(), 1);
    db.close().unwrap();
    let damaged_parts = read_parts();
    assert!(damaged_parts.iter().any(|row| row[1] == invalid));

    let host = Arc::new(CoreHost::open(initial.config.clone()).unwrap());
    let observed = Rig { host, ..initial };
    let cloud = observed.host.cloud_runtime().unwrap();
    cloud.refresh_identity().unwrap();
    let mut head = observed.host.collection().unwrap().reader(ReaderRole::Query).unwrap()
        .row_image(Table::Notetypes, &RowKey::Id(id)).unwrap().unwrap();
    let Cell::Integer(usn) = &mut head[3] else { panic!() };
    *usn += 1;
    restore(&observed, Table::Notetypes, RowKey::Id(id), Some(head));
    let failures = cloud.row_classification_details().unwrap().1;
    assert_eq!(failures.len(), 1);
    let failure = failures[0].clone();
    assert_eq!(failure.kind, codec::Kind::Notetype);
    assert_eq!(failure.key, codec::id_key(id));
    assert!(matches!(failure.failure, Some(RowFailure::Unreadable { table: t, column: "ord" }) if t == table.name()));
    assert!(failure.raw_digest.is_some() && failure.logical_digest.is_some());
    assert!(!cloud.status().unwrap().tracking_stopped);
    assert_eq!(attention(&observed, Attention::Unreadable), 1);
    let retained = cloud.entity_facts(&original.name).unwrap().unwrap();
    assert_eq!(retained.payload, original.payload);
    assert_eq!(retained.acked_payload, original.acked_payload);
    assert_eq!(retained.in_flight, original.in_flight);
    scan(&observed);
    let scanned = cloud.row_classification_details().unwrap().1.remove(0);
    assert_eq!(scanned.raw_digest, failure.raw_digest);
    assert_eq!(scanned.logical_digest, failure.logical_digest);
    let revision = cloud.entity_facts(&original.name).unwrap().unwrap().local_rev;
    cloud.accept_root_fetch(&cloud.instance().unwrap(), Some(&observed.active), true, 1).unwrap();
    let independent_key = "ordinal-repair-progress";
    restore(&observed, Table::Config, RowKey::text(independent_key), Some(config_row(independent_key, true)));
    let independent_name = name(&observed, codec::Kind::Config, independent_key.as_bytes());
    let batch = cloud.build_collection_batch(&cloud.instance().unwrap(), &observed.host.collection().unwrap(),
        &[original.name.clone(), independent_name.clone()].into()).unwrap();
    assert_eq!(batch.saves.iter().map(|row| &row.name).collect::<Vec<_>>(), vec![&independent_name]);
    observed.host.close().unwrap();
    assert_eq!(read_parts(), damaged_parts);

    let host = Arc::new(CoreHost::open(observed.config.clone()).unwrap());
    let reopened = Rig { host, ..observed };
    let cloud = reopened.host.cloud_runtime().unwrap();
    cloud.refresh_identity().unwrap();
    assert_eq!(attention(&reopened, Attention::Unreadable), 1);
    scan(&reopened);
    let durable = cloud.row_classification_details().unwrap().1.remove(0);
    assert_eq!(durable.raw_digest, failure.raw_digest);
    assert_eq!(durable.logical_digest, failure.logical_digest);
    assert_eq!(cloud.entity_facts(&original.name).unwrap().unwrap().local_rev, revision);
    reopened.host.close().unwrap();
    assert_eq!(read_parts(), damaged_parts);

    let db = PlainDatabase::open(&path).unwrap();
    assert_eq!(db.execute(&format!("UPDATE {} SET ord=0 WHERE ntid=?1 AND ord=?2", table.name()),
        &[RawValue::Integer(id), invalid]).unwrap(), 1);
    db.close().unwrap();
    assert_eq!(read_parts(), valid_parts);
    let host = Arc::new(CoreHost::open(reopened.config.clone()).unwrap());
    let repaired = Rig { host, ..reopened };
    let cloud = repaired.host.cloud_runtime().unwrap();
    cloud.refresh_identity().unwrap();
    assert_eq!(attention(&repaired, Attention::Unreadable), 1);
    scan(&repaired);
    assert_eq!(attention(&repaired, Attention::Unreadable), 0);
    let accepted = cloud.entity_facts(&original.name).unwrap().unwrap();
    assert_eq!(accepted.payload, original.payload);
    assert_eq!(accepted.acked_payload, original.acked_payload);
    assert_eq!(accepted.in_flight, original.in_flight);
    scan(&repaired);
    assert_eq!(cloud.entity_facts(&original.name).unwrap().unwrap().local_rev, accepted.local_rev);
    assert_eq!(attention(&repaired, Attention::Unreadable), 0);
    repaired.host.close().unwrap();
}

#[test]
fn k10_rows_adapter_commits_and_selected_observations_are_bounded_at_150k() {
    use meguri_sync::cloud::testing::take_row_read_work;
    let rig = rig();
    let runtime = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    let selected = runtime.selected_control_for_testing(&collection).unwrap();
    // One real collection commit creates payload-independent classifications.
    // Every row is inspected by the production observer, with no tracking seed.
    let committed = collection.run(
        OpSpec::named("large raw classification commit", OpSource::SyncIncoming).stamp(StampMode::Raw),
        |ctx| {
            for id in 1..=150_000 {
                let key = format!("bounded-row-{id:06}");
                ctx.scope().restore(Table::Config, &RowKey::text(&key), Some(&config_row(&key, false)))?;
            }
            Ok(())
        },
    ).unwrap();
    assert!(runtime.capture_proof(committed.seq, committed.generation).unwrap().persisted);
    assert!(!runtime.status().unwrap().tracking_stopped);
    let binding = runtime.status().unwrap().binding.unwrap();
    meguri_sync::cloud::runtime::with_tracking_access(|| {
        let read = meguri_storage::rusqlite::Connection::open_with_flags(
            rig.config.collection_directory.join(meguri_sync::cloud::TRACKING_FILE),
            meguri_storage::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).unwrap();
        assert_eq!(read.query_row("SELECT count(*) FROM cloud_row_classifications WHERE binding=?1 AND kind=8 AND failure=2",
            [binding.id.0.as_slice()], |r| r.get::<_, i64>(0)).unwrap(), 150_000);
    });
    for serialization in [b"cursor-one".as_slice(), b"cursor-two", b"cursor-three"] {
        take_row_read_work();
        runtime.adapter_state_update(&rig.sdk, serialization).unwrap();
        let (queries, steps, owners, semantic) = take_row_read_work();
        assert!(queries > 0 && queries <= 8, "adapter row queries: {queries}");
        assert!(steps < 50_000, "adapter row query instructions: {steps}");
        assert!(owners > 0 && owners <= 8, "adapter owner decodes: {owners}");
        assert_eq!(semantic, 0, "adapter commit must not decode the classification table");
        assert_eq!(runtime.adapter_snapshot().unwrap().serialization.as_deref(), Some(serialization));

        take_row_read_work();
        let observation = runtime.selected_control_for_testing(&collection).unwrap();
        let (queries, steps, owners, semantic) = take_row_read_work();
        assert_eq!(observation, selected);
        assert!(queries > 0 && queries <= 8, "selected row queries: {queries}");
        assert!(steps < 50_000, "selected row query instructions: {steps}");
        assert!(owners > 0 && owners <= 8, "selected owner decodes: {owners}");
        assert_eq!(semantic, 0, "selected observation must not decode the classification table");
    }
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert_eq!(attention(&rig, Attention::Unreadable), 150_000);
    rig.host.close().unwrap();
}
