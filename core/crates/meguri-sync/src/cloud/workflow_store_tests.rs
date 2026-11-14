use super::*;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};

fn fixture() -> (tempfile::TempDir, TrackingStore, BindingId, u64) {
    let directory = tempfile::tempdir().unwrap();
    let scope = BuildScope {
        container: ContainerId::new("synthetic-scan-provenance").unwrap(),
        environment: Environment::Development,
    };
    let OpenOutcome::Missing(mut store) = TrackingStore::open(directory.path(), scope).unwrap() else {
        panic!("new store");
    };
    store.install_runtime().unwrap();
    let binding = store
        .create_binding(
            BindingFacts {
                account: OpaqueBytes(vec![1]),
                epoch: EpochId([2; 16]),
                record_key: RecordKey([3; 32]),
                publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
                state: BindingState::Active,
            },
            &FixedClock::new(1000, UtcOffset::UTC),
            &SeededRandom::new(1916),
        )
        .unwrap();
    let state = store
        .open_runtime_binding(
            binding.id,
            runtime::NativeFileIdentity {
                device: 1,
                inode: 2,
                created_seconds: 1,
                created_nanos: 0,
            },
            runtime::DurableFileIdentity {
                version: 1,
                identity: if cfg!(target_vendor = "apple") {
                    meguri_storage::persistent_identity::PersistentIdentity::Darwin {
                        volume_uuid: [3; 16],
                        inode: 2,
                    }
                } else {
                    meguri_storage::persistent_identity::PersistentIdentity::NonDarwin { device: 1, inode: 2 }
                },
                created_seconds: 1,
                created_nanos: 0,
            },
        )
        .unwrap();
    (directory, store, binding.id, state.open_counter)
}

fn scan_classification(e: &EntityFacts, open: u64, seq: u64, generation: u64) -> super::super::RowClassification {
    let rows = row_payload::payload_rows(&e.payload).unwrap();
    super::super::RowClassification {
        kind: e.kind, key: e.key.clone(), open, seq, generation,
        file: runtime::NativeFileIdentity { device: 1, inode: 2, created_seconds: 1, created_nanos: 0 },
        raw_digest: Some(meguri_storage::journal::digest(&rows[0].2)),
        logical_digest: Some(row_payload::logical_row_digest(rows[0].0, &rows[0].2)), failure: None,
    }
}

// Cover both the note adapter and generic entity storage with actual store APIs.
fn payloads() -> [codec::Payload; 2] {
    [
        codec::Payload::Note(codec::Note {
            id: 11,
            mid: 12,
            nt_struct_gen: 1,
            flds: b"front\x1fback".to_vec(),
            ..Default::default()
        }),
        codec::Payload::Config(codec::Config {
            key: b"scan-provenance".to_vec(),
            val: b"1".to_vec(),
            ..Default::default()
        }),
    ]
}

#[test]
fn exchange_blocks_follow_pending_references_and_stop_at_clean_images() {
    let (_directory, mut store, binding, open) = fixture();
    let type_payload = codec::Payload::Notetype(codec::Notetype {
        id: 12,
        ..Default::default()
    });
    let note_payload = payloads()[0].clone();
    let card_payload = codec::Payload::Card(codec::Card {
        id: 13,
        nid: 11,
        ..Default::default()
    });
    let key = store.binding(binding).unwrap().unwrap().facts.record_key;
    let mut state = store.runtime_binding(binding).unwrap();
    let rows = [type_payload, note_payload, card_payload]
        .into_iter()
        .map(|payload| {
            new_pending_entity(
                &key,
                payload,
                [5; 16],
                DirtySource::Snapshot,
                &mut state.change_counter,
                (open, 10, 7),
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let tx = store.connection.transaction().unwrap();
    for row in &rows {
        put(&tx, binding, row).unwrap();
    }
    put_binding(&tx, binding, &state).unwrap();
    tx.commit().unwrap();
    let names = rows.iter().map(|row| row.name.clone()).collect();
    assert!(store.blocked_exchange_names(binding, &names).unwrap().is_empty());
    let refused = super::super::incoming::RefusedEntry {
        record_name: rows[0].name.clone(),
        reason: "synthetic unsupported format".into(),
        envelope: OpaqueBytes(vec![1]),
        origin: None,
        erev: None,
        seen_at_ms: 1000,
    };
    store
        .receive_records(binding, &[], std::slice::from_ref(&refused), None, None)
        .unwrap();
    assert_eq!(store.blocked_exchange_names(binding, &names).unwrap(), names);

    // A supported clean note already satisfies its card's dependency. A later
    // problem with the note type does not turn that note into pending work.
    let mut clean = rows[1].clone();
    clean.mark = None;
    clean.server_erev = 1;
    clean.acked_rev = clean.local_rev;
    clean.acked_row_digest = clean.payload.row_digest().unwrap();
    clean.acked_payload_digest = clean.payload.payload_digest().unwrap();
    clean.acked_payload = Some(clean.payload.clone());
    clean.acked_system_fields = vec![1];
    let tx = store.connection.transaction().unwrap();
    put(&tx, binding, &clean).unwrap();
    tx.commit().unwrap();
    let selected = [rows[2].name.clone()].into();
    assert!(store.blocked_exchange_names(binding, &selected).unwrap().is_empty());
    // A refusal of the note itself still prevents sending its card.
    let note_refusal = super::super::incoming::RefusedEntry {
        record_name: clean.name,
        ..refused
    };
    store.receive_records(binding, &[], &[note_refusal], None, None).unwrap();
    assert_eq!(store.blocked_exchange_names(binding, &selected).unwrap(), selected);
}

#[test]
fn guarded_scan_equal_payload_preserves_pending_facts_and_retained_flight() {
    let (_directory, mut store, binding, open) = fixture();
    for payload in payloads() {
        let row = store
            .insert_workflow_entity(binding, payload.clone(), [5; 16], (open, 10, 7))
            .unwrap();
        store
            .persist_flight(
                binding,
                &row.name,
                Flight {
                    local_rev: row.local_rev,
                    payload_digest: payload.payload_digest().unwrap(),
                    row_digest: payload.row_digest().unwrap(),
                    payload: payload.into(),
                    erev: 1,
                    image_seq: 10,
                    held: false,
                    tombstone: false,
                    tombstone_envelope_digest: None,
                },
            )
            .unwrap();
        let before = store.entity_facts(binding, &row.name).unwrap().unwrap();
        assert!(before.sent && !before.in_flight.is_empty() && before.mark.is_some());
        let counter = store.runtime_binding(binding).unwrap().change_counter;
        store.workflow_merge(binding, open, 20, 8, before.clone(), &scan_classification(&before, open, 20, 8)).unwrap();
        let after = store.entity_facts(binding, &row.name).unwrap().unwrap();
        let mut expected = before;
        expected.observer_open = open;
        expected.observed_seq = 20;
        expected.observed_generation = 8;
        assert!(after == expected, "only actual scan provenance may change");
        assert_eq!(store.runtime_binding(binding).unwrap().change_counter, counter);
    }
}

#[test]
fn guarded_changed_scan_marks_snapshot_revision_and_rejects_older_or_invalid_open() {
    let (_directory, mut store, binding, open) = fixture();
    for payload in payloads() {
        let before = store
            .insert_workflow_entity(binding, payload, [6; 16], (open, 10, 7))
            .unwrap();
        let counter = store.runtime_binding(binding).unwrap().change_counter;
        let mut scanned = before.clone();
        match &mut scanned.payload {
            codec::Payload::Note(note) => note.flds = b"front\x1fchanged".to_vec(),
            codec::Payload::Config(config) => config.val = b"2".to_vec(),
            _ => unreachable!(),
        }
        store.workflow_merge(binding, open, 20, 7, scanned.clone(), &scan_classification(&scanned, open, 20, 7)).unwrap();
        let committed = store.entity_facts(binding, &before.name).unwrap().unwrap();
        assert_eq!(committed.payload, scanned.payload);
        assert_eq!(committed.local_rev, before.local_rev + 1);
        let mark = committed.mark.as_ref().unwrap();
        assert_eq!(mark.source, DirtySource::Snapshot);
        assert_eq!(mark.change_seq, counter + 1);
        assert_eq!(mark.payload_digest, committed.payload.payload_digest().unwrap());
        assert_eq!(
            (
                committed.observer_open,
                committed.observed_seq,
                committed.observed_generation
            ),
            (open, 20, 7)
        );
        assert_eq!(store.runtime_binding(binding).unwrap().change_counter, counter + 1);
        // The old image must not replace a later commit from this open/generation.
        store.workflow_merge(binding, open, 19, 7, before.clone(), &scan_classification(&before, open, 19, 7)).unwrap();
        assert!(store.entity_facts(binding, &before.name).unwrap().unwrap() == committed);
        for invalid_open in [open - 1, open + 1] {
            assert_eq!(
                store.workflow_merge(binding, invalid_open, 30, 8, before.clone(), &scan_classification(&before, invalid_open, 30, 8)),
                Err(Error::InvalidData)
            );
            assert!(store.entity_facts(binding, &before.name).unwrap().unwrap() == committed);
        }
        assert_eq!(store.runtime_binding(binding).unwrap().change_counter, counter + 1);
    }
}

#[test]
fn k10_rows_storage_stale_failure_repair_deletion_and_failed_transaction_preserve_newer_fact() {
    use super::super::{RowClassification, row_payload::RowFailure};
    let (_dir,mut store,b,open)=fixture();
    let payload=payloads()[1].clone();
    let e=store.insert_workflow_entity(b,payload,[1;16],(open,1,7)).unwrap();
    let mut failed=scan_classification(&e,open,10,7);
    failed.failure=Some(RowFailure::Oversize);
    assert!(store.classify_row(b,&failed).unwrap());
    let repair=RowClassification{seq:11,failure:None,..failed.clone()};
    store.workflow_merge(b,open,repair.seq,repair.generation,e.clone(),&repair).unwrap();
    assert!(!store.classify_row(b,&failed).unwrap());
    assert!(store.row_classifications(b,true).unwrap().is_empty());
    let mut later=failed.clone(); later.seq=12;
    assert!(store.classify_row(b,&later).unwrap());
    let deletion=RowClassification{seq:13,raw_digest:None,logical_digest:None,failure:None,..later.clone()};
    store.connection.execute_batch("CREATE TRIGGER reject_classification BEFORE INSERT ON cloud_row_classifications BEGIN SELECT RAISE(FAIL,'classification fault'); END;").unwrap();
    assert!(store.classify_row(b,&deletion).is_err());
    assert_eq!(store.row_classifications(b,true).unwrap(),vec![later.clone()]);
    store.connection.execute_batch("DROP TRIGGER reject_classification").unwrap();
    assert!(store.classify_row(b,&deletion).unwrap());
    assert!(!store.classify_row(b,&later).unwrap());
    for invalid in [RowClassification{open:open+1,..later.clone()},RowClassification{generation:8,..later.clone()},RowClassification{file:runtime::NativeFileIdentity{inode:99,..later.file},..later.clone()}] {
        assert!(store.classify_row(b,&invalid).is_err());
    }
    assert!(store.row_classifications(b,true).unwrap().is_empty());
    assert!(!store.row_classification_coverage(b).unwrap(),"partial classification never certifies an empty ledger");
}

#[test]
fn k10_rows_v8_private_wal_admission_refuses_future_or_inconsistent_state_without_mutation() {
    let (dir,mut store,b,open)=fixture();
    let scope=store.scope.clone();
    let e=store.insert_workflow_entity(b,payloads()[1].clone(),[1;16],(open,1,7)).unwrap();
    let mut f=scan_classification(&e,open,2,7); f.failure=Some(row_payload::RowFailure::Oversize);
    store.classify_row(b,&f).unwrap();
    store.connection.execute_batch("PRAGMA wal_autocheckpoint=0; UPDATE cloud_runtime_schema SET version=9;").unwrap();
    let paths=[dir.path().join(TRACKING_FILE),dir.path().join(format!("{TRACKING_FILE}-wal"))];
    let bytes=paths.iter().map(|p| std::fs::read(p).unwrap()).collect::<Vec<_>>();
    assert!(matches!(TrackingStore::open(dir.path(),scope.clone()),Err(Error::RuntimeSchemaUnsupported)));
    assert_eq!(paths.iter().map(|p| std::fs::read(p).unwrap()).collect::<Vec<_>>(),bytes);
    store.connection.execute("UPDATE cloud_runtime_schema SET version=8",[]).unwrap();
    store.connection.execute("UPDATE cloud_row_classifications SET content=json_set(content,'$.open',999999)",[]).unwrap();
    let damaged=paths.iter().map(|p| std::fs::read(p).unwrap()).collect::<Vec<_>>();
    assert!(matches!(TrackingStore::open(dir.path(),scope.clone()).unwrap(),OpenOutcome::Damaged));
    assert_eq!(paths.iter().map(|p| std::fs::read(p).unwrap()).collect::<Vec<_>>(),damaged);
    let foreign=BuildScope{container:ContainerId::new("synthetic-classification-foreign").unwrap(),..scope.clone()};
    assert!(matches!(TrackingStore::open(dir.path(),foreign).unwrap(),OpenOutcome::Opened(_)));
    assert!(super::super::runtime_store::validate_bindings(&store.connection, &store.scope).is_err());
    store.connection.execute("UPDATE cloud_row_classifications SET content=?1",[encode(&f).unwrap()]).unwrap();
    store.connection.execute("UPDATE cloud_runtime_schema SET version=7",[]).unwrap();
    let before=store.connection.total_changes();
    assert!(matches!(store.install_runtime(),Err(Error::InvalidData)));
    assert_eq!(store.connection.total_changes(),before,"old marker plus new tables cannot trigger downgrade or migration");
    store.connection.execute("UPDATE cloud_runtime_schema SET version=8",[]).unwrap();
    store.connection.execute_batch("DROP TABLE cloud_row_classification_coverage").unwrap();
    assert!(matches!(super::super::runtime_store::check_version(&store.connection),Err(Error::InvalidData)));
}

fn classification_ledger(open: u64) -> CoverageLedger {
    CoverageLedger {
        open,
        cursor: runtime::NoteScanCursor { seq: 10, generation: 7, covered_through: 10,
            native_file: runtime::NativeFileIdentity { device: 1, inode: 2, created_seconds: 1, created_nanos: 0 },
            modified: 0, schema_modified: 0 },
        counts: row_payload::KINDS.into_iter().map(|k| (k,0)).collect(),
        retained_tombstones: Vec::new(), media_feed_id: [0;16], media_cursor: 0,
    }
}

#[test]
fn k10_rows_coverage_requires_exact_accepted_state_and_reopen_or_rebase_cannot_transfer_it() {
    let (_dir,mut store,b,open) = fixture();
    assert!(!store.row_classification_coverage(b).unwrap());
    let ledger = classification_ledger(open);
    store.finish_workflow_scan(b,ledger.clone()).unwrap();
    assert!(store.row_classification_coverage(b).unwrap());
    for variant in 0..3 {
        let mut inconsistent=ledger.clone();
        match variant {
            0 => inconsistent.cursor.seq -= 1,
            1 => inconsistent.cursor.generation += 1,
            _ => inconsistent.counts[0].1 += 1,
        }
        store.connection.execute("UPDATE cloud_row_classification_coverage SET content=?1 WHERE binding=?2",params![encode(&inconsistent).unwrap(),b.0.as_slice()]).unwrap();
        assert!(store.row_classification_coverage(b).is_err());
        assert!(matches!(store.stored_runtime_state(b), Err(Error::InvalidData)));
        assert!(super::super::row_classification::validate(&store.connection, &store.scope).is_err());
    }
    store.connection.execute("UPDATE cloud_row_classification_coverage SET content=?1 WHERE binding=?2",params![encode(&ledger).unwrap(),b.0.as_slice()]).unwrap();
    let original=store.runtime_binding(b).unwrap();
    let mut state=store.open_runtime_binding(b,original.file.unwrap(),original.durable_file.unwrap()).unwrap();
    assert!(!store.row_classification_coverage(b).unwrap());
    let file=runtime::NativeFileIdentity{inode:3,..state.file.unwrap()};
    let mut durable=state.durable_file.unwrap();
    match &mut durable.identity {
        meguri_storage::persistent_identity::PersistentIdentity::Darwin{inode,..}
        | meguri_storage::persistent_identity::PersistentIdentity::NonDarwin{inode,..} => *inode=3,
    }
    state.record_identity(file,durable).unwrap();
    let tx=store.connection.transaction().unwrap(); put_binding(&tx,b,&state).unwrap(); tx.commit().unwrap();
    assert!(!store.row_classification_coverage(b).unwrap());
    assert!(state.coverage.is_none());
}

#[test]
fn k10_rows_failure_metadata_is_atomic_idempotent_and_scoped_without_payload() {
    let (_dir,mut store,b,open)=fixture();
    let payload=payloads()[1].clone();
    let mut counter=0;
    let e=new_pending_entity(&store.binding(b).unwrap().unwrap().facts.record_key,payload,[5;16],DirtySource::Observer,&mut counter,(open,1,7)).unwrap();
    let mut failed=scan_classification(&e,open,2,7);
    failed.failure=Some(row_payload::RowFailure::Unreadable{table:"config",column:"mtime_secs"});
    let counter=store.runtime_binding(b).unwrap().change_counter;
    store.connection.execute_batch("CREATE TRIGGER fail_classification BEFORE INSERT ON cloud_row_classifications BEGIN SELECT RAISE(FAIL,'classification fault'); END;").unwrap();
    assert!(store.classify_row(b,&failed).is_err());
    assert_eq!(store.runtime_binding(b).unwrap().change_counter,counter);
    assert!(!store.row_failed(b,&e.name).unwrap());
    assert!(!store.row_classification_coverage(b).unwrap());
    store.connection.execute_batch("DROP TRIGGER fail_classification").unwrap();
    store.classify_row(b,&failed).unwrap();
    let revision=store.row_failure_revision(b,&e.name).unwrap().unwrap();
    assert_eq!(revision,(1,open,7,2));
    assert_eq!(store.runtime_binding(b).unwrap().change_counter,counter+1);
    assert!(store.entity_header(b,&e.name).unwrap().is_none());
    assert!(store.policy_pending_names(b).unwrap().contains(&e.name));
    assert_eq!(store.blocked_exchange_names(b,&[e.name.clone()].into()).unwrap(),[e.name.clone()].into());
    let own_scope=store.scope.clone();
    let own_state=store.runtime_binding(b).unwrap();
    store.scope=BuildScope{container:ContainerId::new("synthetic-other-classification").unwrap(),environment:Environment::Development};
    let other=store.create_binding(store.connection.query_row("SELECT facts FROM bindings WHERE id=?1",[b.0.as_slice()],|r|r.get::<_,String>(0)).map(|text|decode(&text).unwrap()).unwrap(),
        &FixedClock::new(1000,UtcOffset::UTC),&SeededRandom::new(1932)).unwrap();
    let other_state=store.open_runtime_binding(other.id,own_state.file.unwrap(),own_state.durable_file.unwrap()).unwrap();
    assert!(!store.row_failed(other.id,&e.name).unwrap());
    let other_failure=super::super::RowClassification{open:other_state.open_counter,..failed.clone()};
    store.classify_row(other.id,&other_failure).unwrap();
    store.scope=own_scope;
    assert!(store.classify_row(other.id,&other_failure).is_err());
    assert!(store.row_classifications(other.id,true).is_err());
    assert_eq!(store.row_failure_counts(b).unwrap(),vec![(2,1)]);
    super::super::row_classification::validate(&store.connection, &store.scope).unwrap();
    failed.seq+=1; failed.raw_digest=Some([19;32]); // raw-only (for example USN) changes are not logical edits
    store.classify_row(b,&failed).unwrap();
    assert_eq!(store.row_failure_revision(b,&e.name).unwrap().unwrap().0,revision.0);
    assert_eq!(store.runtime_binding(b).unwrap().change_counter,counter+1);
    failed.seq+=1; failed.logical_digest=Some([20;32]);
    store.classify_row(b,&failed).unwrap();
    assert_eq!(store.row_failure_revision(b,&e.name).unwrap().unwrap().0,revision.0+1);
    assert_eq!(store.runtime_binding(b).unwrap().change_counter,counter+2);
}

#[test]
fn k10_rows_indexed_observer_and_candidate_queries_are_bounded_at_150k() {
    use super::super::row_classification as rows;
    let (_dir,mut store,b,open)=fixture();
    let binding=store.binding(b).unwrap().unwrap();
    let mut state=store.runtime_binding(b).unwrap();
    let file=state.file.unwrap();
    let tx=store.connection.transaction().unwrap();
    let mut selected=Vec::new();
    // Real acceptance API installs valid classifications. No payload bodies,
    // policy counts or exchange results are seeded.
    for id in 1..=150_000_i64 {
        let f=super::super::RowClassification{kind:codec::Kind::Revlog,key:codec::id_key(id).to_vec(),
            open,generation:7,seq:1,file,raw_digest:Some([1;32]),logical_digest:Some([2;32]),failure:None};
        rows::accept(&tx,b,&f,&mut state,&binding.facts.record_key,DirtySource::Snapshot).unwrap();
        if id<=250 { selected.push(codec::entity_name(&binding.facts.record_key.0,f.kind,&f.key).unwrap()); }
    }
    let bad=super::super::RowClassification{kind:codec::Kind::Config,key:b"indexed-bad".to_vec(),
        open,generation:7,seq:1,file,raw_digest:Some([3;32]),logical_digest:Some([4;32]),
        failure:Some(row_payload::RowFailure::Unreadable{table:"config",column:"mtime_secs"})};
    rows::accept(&tx,b,&bad,&mut state,&binding.facts.record_key,DirtySource::Observer).unwrap();
    put_binding(&tx,b,&state).unwrap(); tx.commit().unwrap();
    rows::take_read_work();
    #[cfg(feature="test-hooks")] let bodies=body_reads();
    for _ in 0..3 { for name in &selected { assert!(!store.row_failed(b,name).unwrap()); } }
    assert_eq!(store.row_failure_counts(b).unwrap(),vec![(2,1)]);
    assert_eq!(store.row_failure_names(b).unwrap().len(),1);
    let (queries,steps,owners,semantic)=rows::take_read_work();
    assert_eq!(queries,752);
    assert!(steps<50_000,"indexed reads executed {steps} VM instructions");
    assert_eq!(owners,0);
    assert_eq!(semantic,0);
    #[cfg(feature="test-hooks")] assert_eq!(body_reads(),bodies);
    rows::validate(&store.connection, &store.scope).unwrap();
    let (_,_,owners,semantic)=rows::take_read_work();
    assert_eq!(owners,1,"one state decode for this owner at admission");
    assert_eq!(semantic,150_001,"admission validates every scoped classification");
    let first=store.row_classification_page(b,None).unwrap();
    assert_eq!(first.len(),250);
    let last=first.last().unwrap();
    let second=store.row_classification_page(b,Some((last.kind,&last.key))).unwrap();
    assert_eq!(second.len(),250);
    assert!(second.iter().all(|f| (f.kind as u8,&f.key)>(last.kind as u8,&last.key)));
}

#[test]
fn k10_rows_all_classified_write_guards_and_failed_scan_completion_leave_coverage_unknown() {
    let (_dir,mut store,b,open)=fixture();
    let payload=payloads()[1].clone();
    let e=store.insert_workflow_entity(b,payload.clone(),[5;16],(open,1,7)).unwrap();
    let good=scan_classification(&e,open,2,7);
    for invalid in [super::super::RowClassification{open:open+1,..good.clone()},
        super::super::RowClassification{file:runtime::NativeFileIdentity{inode:99,..good.file},..good.clone()}] {
        assert!(store.classify_row(b,&invalid).is_err());
        assert!(store.workflow_merge(b,invalid.open,invalid.seq,invalid.generation,e.clone(),&invalid).is_err());
        let mut new=invalid.clone(); new.key=b"new-guard".to_vec();
        let mut p=payload.clone(); let codec::Payload::Config(ref mut config)=p else {unreachable!()}; config.key=new.key.clone();
        assert!(store.insert_classified_scan_entity(b,p,[1;16],&new,false).is_err());
    }
    assert!(store.row_classifications(b,false).unwrap().is_empty());
    store.connection.execute_batch("CREATE TRIGGER fail_coverage BEFORE INSERT ON cloud_row_classification_coverage BEGIN SELECT RAISE(FAIL,'coverage fault'); END;").unwrap();
    assert!(store.finish_workflow_scan(b,classification_ledger(open)).is_err());
    assert!(store.runtime_binding(b).unwrap().coverage.is_none());
    assert!(!store.row_classification_coverage(b).unwrap());
}

#[test]
fn k10_rows_usn_is_raw_identity_only() {
    let mut raw=row_payload::payload_rows(&payloads()[0]).unwrap().remove(0).2;
    let before=meguri_storage::journal::digest(&raw);
    let logical=row_payload::logical_row_digest(Table::Notes,&raw);
    raw[Table::Notes.usn_column().unwrap()]=meguri_storage::Cell::Integer(100);
    assert_ne!(before,meguri_storage::journal::digest(&raw));
    assert_eq!(logical,row_payload::logical_row_digest(Table::Notes,&raw));
}

#[test]
fn k10_rows_v8_scope_admission_ignores_foreign_runtime_rows_and_coverage_without_writes() {
    for environment in [false, true] {
        let (dir, mut store, b, open) = fixture();
        let own_scope = store.scope.clone();
        let state = store.runtime_binding(b).unwrap();
        let facts = store.binding(b).unwrap().unwrap().facts;
        store.scope = if environment {
            BuildScope { environment: Environment::Production, ..own_scope.clone() }
        } else {
            BuildScope { container: ContainerId::new("synthetic-foreign-rows").unwrap(), ..own_scope.clone() }
        };
        let other_scope = store.scope.clone();
        let other = store.create_binding(facts, &FixedClock::new(1000, UtcOffset::UTC), &SeededRandom::new(1933)).unwrap().id;
        let other_open = store.open_runtime_binding(other, state.file.unwrap(), state.durable_file.unwrap()).unwrap().open_counter;
        let e = store.insert_workflow_entity(other, payloads()[1].clone(), [8;16], (other_open,1,7)).unwrap();
        let mut failed = scan_classification(&e, other_open, 2, 7);
        failed.failure = Some(row_payload::RowFailure::Oversize);
        store.classify_row(other, &failed).unwrap();
        store.finish_workflow_scan(other, classification_ledger(other_open)).unwrap();
        let own = store.insert_workflow_entity(b, payloads()[1].clone(), [1;16], (open,1,7));
        assert!(own.is_err(), "a foreign build cannot select the current owner");
        store.scope = own_scope.clone();
        let own = store.insert_workflow_entity(b, payloads()[1].clone(), [1;16], (open,1,7)).unwrap();
        let mut current = scan_classification(&own, open, 2, 7);
        current.failure = Some(row_payload::RowFailure::Oversize);
        store.classify_row(b, &current).unwrap();
        for table in ["cloud_runtime_bindings", "cloud_row_classifications", "cloud_row_classification_coverage"] {
            store.connection.execute(&format!("UPDATE {table} SET content='{{}}' WHERE binding=?1"), [other.0.as_slice()]).unwrap();
        }
        let foreign = || ["cloud_runtime_bindings", "cloud_row_classifications", "cloud_row_classification_coverage"].map(|table|
            store.connection.query_row(&format!("SELECT content FROM {table} WHERE binding=?1"), [other.0.as_slice()], |r| r.get::<_, String>(0)).unwrap());
        let before = foreign();
        let OpenOutcome::Opened(mut admitted) = TrackingStore::open(dir.path(), own_scope.clone()).unwrap() else { panic!("current scope remains usable"); };
        admitted.install_runtime().unwrap();
        assert!(admitted.row_failed(b, &own.name).unwrap());
        assert_eq!(foreign(), before);
        assert!(matches!(TrackingStore::open(dir.path(), other_scope).unwrap(), OpenOutcome::Damaged));
        drop(admitted);
        // Same corruption in the current owner must be refused by the private
        // probe, before even a WAL-open of the original file.
        store.connection.execute("UPDATE cloud_row_classifications SET content='{}' WHERE binding=?1", [b.0.as_slice()]).unwrap();
        let files = [dir.path().join(TRACKING_FILE), dir.path().join(format!("{TRACKING_FILE}-wal"))];
        let bytes = files.iter().map(|p| std::fs::read(p).unwrap()).collect::<Vec<_>>();
        assert!(matches!(TrackingStore::open(dir.path(), own_scope).unwrap(), OpenOutcome::Damaged));
        assert_eq!(files.iter().map(|p| std::fs::read(p).unwrap()).collect::<Vec<_>>(), bytes);
    }
}
