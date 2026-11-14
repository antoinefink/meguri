//! Source fixtures for the original writer boundary and restrictive local choices.
use super::*;
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate};
use meguri_storage::raw::{PlainDatabase, RawValue};

fn ordinary_native_save_failure_case(failure: &str) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated_legacy_reversed(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(461)).unwrap();
    publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let source = source_type(&host);
    let original = nt(&source);
    let before = bookkeeping(&host);
    let notes = runtime.entity_facts_page(codec::Kind::Note, None).unwrap();
    let cards = runtime.entity_facts_page(codec::Kind::Card, None).unwrap();
    collection.pause_automatic_wal_flush_for_testing(true);
    if failure == "wal" { collection.fail_wal_sync_after_next_commit_for_testing(); }
    if failure == "journal" { runtime.fail_next_capture_journal_reads_for_testing(); }
    if failure == "foreign" {
        let path = collection.path().to_owned();
        collection.after_next_commit(move |_| {
            let db = PlainDatabase::open(&path).unwrap();
            db.execute_batch("BEGIN IMMEDIATE").unwrap();
            db.execute("UPDATE templates SET ord=-ord-1 WHERE ntid=?1", &[RawValue::Integer(original.id)]).unwrap();
            db.execute("UPDATE templates SET ord=ord+2 WHERE ntid=?1", &[RawValue::Integer(original.id)]).unwrap();
            db.execute_batch("COMMIT").unwrap();
            db.close().unwrap();
        });
    }
    let base = collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(original.id)).unwrap().unwrap();
    let mut draft = notetypes::Draft::of(base);
    let first = draft.templates[0].name.clone();
    draft.templates[0].name = draft.templates[1].name.clone();
    draft.templates[1].name = first;
    draft.config.css = "Owned commit styling".into();
    // Observer failure does not roll back or repeat the successful collection COMMIT.
    let saved = notetypes::save(&collection, draft, Default::default()).unwrap();
    assert_ne!(current_type(&host, &original), source.payload);
    assert_eq!(bookkeeping(&host), before);
    if failure == "control" {
        let captured = runtime.entity_facts(&source.name).unwrap().unwrap();
        assert_eq!(captured.payload, current_type(&host, &original));
        assert_eq!(captured.acked_payload, source.acked_payload);
        assert_eq!(captured.base_payload, Some(source.payload));
        assert!(!runtime.status().unwrap().tracking_stopped);
        let identity = {
            let lease = collection.reader(ReaderRole::Cloud).unwrap();
            let snapshot = lease.snapshot().unwrap();
            snapshot.wal_identity()
        };
        let meguri_engine::WalCoverage::Covered(receipt) = collection.wait_wal_covered(
            identity, saved.seq, Duration::ZERO).unwrap() else { panic!("authority preceded WAL coverage") };
        assert_eq!(receipt.identity(), identity);
        assert!(receipt.through() >= saved.seq);
    } else {
        assert_eq!(runtime.entity_facts(&source.name).unwrap().unwrap(), source);
        assert!(runtime.status().unwrap().tracking_stopped);
        assert!(runtime.require_collection_sending().is_err());
        assert!(runtime.register_sdk(true).is_err());
        let projection = runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap();
        assert_ne!(projection.facts.fence, cloud::policy::Fence::ChooseCollection);
        assert!(matches!(host.begin_cloud_connection_preview(), Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::Database))));
        collection.flush_wal(Duration::from_secs(30)).unwrap();
        assert_eq!(runtime.entity_facts(&source.name).unwrap().unwrap(), source);
    }
    assert_eq!(runtime.entity_facts_page(codec::Kind::Note, None).unwrap(), notes);
    assert_eq!(runtime.entity_facts_page(codec::Kind::Card, None).unwrap(), cards);
    collection.pause_automatic_wal_flush_for_testing(false);
    host.close().unwrap();
}

#[test]
fn ordinary_notetype_native_authority_requires_current_open_wal_coverage() {
    ordinary_native_save_failure_case("control");
}
#[test]
fn ordinary_notetype_native_wal_failure_preserves_tracking_images_and_error() {
    ordinary_native_save_failure_case("wal");
}
#[test]
fn ordinary_notetype_native_journal_failure_is_not_collection_choice() {
    ordinary_native_save_failure_case("journal");
}
#[test]
fn ordinary_notetype_foreign_post_commit_reorder_cannot_use_native_identity_report() {
    ordinary_native_save_failure_case("foreign");
}

#[test]
fn ordinary_notetype_native_add_and_clone_after_publication_bootstrap_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(462)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let collection = host.collection().unwrap();
    let created = notetypes::add(&collection, NewNotetype {
        name: "Native published addition".into(), fields: vec!["Prompt".into(), "Reply".into()],
        templates: vec![NewTemplate { name: "Question".into(), front: "{{Prompt}}".into(), back: "{{Reply}}".into(), ..Default::default() }],
        ..Default::default()
    }).unwrap().value.id;
    let cloned = notetypes::clone_notetype(&collection, created, Some("Native published clone".into())).unwrap().value.id;
    for id in [created, cloned] {
        meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
            notetype_id: id, fields: vec!["New local prompt".into(), "New local reply".into()],
            tags: vec![], deck_id: meguri_model::ids::DeckId(1), guid: None, update_add_defaults: false,
        }, meguri_model::changes::OpSource::UserUndoable).unwrap();
    }
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let mut retained = Vec::new();
    for id in [created, cloned] {
        let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Notetype, &codec::id_key(id.0)).unwrap();
        let entity = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(nt(&entity).struct_gen, 1);
        assert_eq!(entity.acked_rev, 0);
        assert!(entity.acked_payload.is_none() && entity.base_payload.is_none() && entity.mark.is_some());
        retained.push(entity);
    }
    assert_ne!(runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap().facts.fence,
        cloud::policy::Fence::ChooseCollection);
    collection.flush_wal(Duration::from_secs(30)).unwrap();
    host.close().unwrap();
    drop(collection);
    drop(host);
    let host = open_host(&dir, keys());
    prove_reopened_binding(&host, &binding);
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    for entity in retained {
        let current = host.cloud_runtime().unwrap().entity_facts(&entity.name).unwrap().unwrap();
        assert_eq!(current.payload, entity.payload);
        assert_eq!(current.acked_rev, 0);
    }
    host.close().unwrap();
}
