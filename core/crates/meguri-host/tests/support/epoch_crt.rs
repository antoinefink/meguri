//! Stable-binding fixtures establish P6 through a Service-saved complete epoch
//! and production intake. No tracking setter or local/media fallback is used.
use meguri_host::CoreHost;
use meguri_model::rng::SeededRandom;
use meguri_storage::{ReadRows, ReaderRole};
use meguri_sync::cloud::{self, workflow::Saved};
use meguri_testkit::cloud::{Caller, Field, Mutation, MutationSuccess, Operation, RecordInput, Reply, Scope, Service};

pub fn accept(host: &CoreHost, service: &mut Service<SeededRandom>, scope: &Scope) {
    let runtime = host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    assert_eq!(binding.scope.container.as_str(), scope.container);
    assert_eq!(binding.facts.account.0, scope.account.as_bytes());
    let collection = host.collection().unwrap();
    let crt = collection.reader(ReaderRole::Query).unwrap().col().unwrap().created.0;
    let mut counts = Vec::new();
    for kind in cloud::row_payload::KINDS {
        let mut after = None;
        let mut count = 0;
        loop {
            let page = runtime.entity_facts_page(kind, after.as_deref()).unwrap();
            if page.is_empty() { break; }
            count += page.iter().filter(|entity| entity.tombstone_at.is_none()).count() as u64;
            after = page.last().map(|entity| entity.key.clone());
        }
        counts.push((kind, count));
    }
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    assert!(matches!(service.request(scope, Caller::default(), Operation::CreateZone { zone: zone.clone() }).unwrap().result, Ok(Reply::Done)));
    let envelope = cloud::control_codec::epoch(binding.facts.epoch.0, binding.facts.record_key.0, crt, 1_894_701_600_000, &counts).unwrap();
    let response = service.request(scope, Caller::default(), Operation::Modify {
        zone, atomic: true, changes: vec![Mutation::Save {
            record: RecordInput::encrypted("epoch", "MGEpoch", envelope), if_unchanged: None,
        }],
    }).unwrap();
    let Ok(Reply::Modified(mut outcomes)) = response.result else { panic!("epoch save required"); };
    let Ok(MutationSuccess::Saved(record)) = outcomes.remove(0).result else { panic!("accepted epoch required"); };
    let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else { panic!("complete epoch required"); };
    runtime.receive_collection_records(&runtime.instance().unwrap(), &[Saved {
        name: record.name.clone(), envelope: envelope.clone(), system_fields: record.system_fields.bytes().to_vec(),
    }]).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(crt));
}
