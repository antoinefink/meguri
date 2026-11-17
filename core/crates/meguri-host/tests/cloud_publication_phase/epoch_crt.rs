//! P6 producer step only: actual publication and fetch transactions, no policy seeding.
use super::*;

pub(super) fn setup() -> (tempfile::TempDir, std::path::PathBuf, CoreHost, Service<SeededRandom>) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_prior_icloud_host(&dir, keys());
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let p = preview(&host, &mut service);
    host.cloud_runtime().unwrap().confirm_connection_publication(&p, &host.collection().unwrap()).unwrap();
    (temp, dir, host, service)
}

#[test]
fn epoch_crt_publication_requires_actual_epoch_ack_and_survives_reopen() {
    let (_temp, dir, host, mut service) = setup();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), None);
    let crt = runtime.publication_op().unwrap().unwrap().crt;
    let mut epoch_acked = false;
    let mut reached_scan = false;
    // Same finite publication-state budget as own_until_scan.
    for _ in 0..16 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) => {
                let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
                runtime.publication_result(&request, result, &collection).unwrap();
                epoch_acked |= request.kind == OwnKind::SavePublishingEpoch;
                assert_eq!(runtime.epoch_crt_for_testing().unwrap(), epoch_acked.then_some(crt));
            }
            PublicationWork::Scan => { reached_scan = true; break; }
            other => panic!("unexpected publication work: {other:?}"),
        }
    }
    assert!(epoch_acked && reached_scan);
    host.close().unwrap();
    drop(host);
    let reopened = open_prior_icloud_host(&dir, keys());
    assert_eq!(reopened.cloud_runtime().unwrap().epoch_crt_for_testing().unwrap(), Some(crt));
    reopened.close().unwrap();
}

pub(super) fn save_complete_epoch(
    service: &mut Service<SeededRandom>,
    zone: &str,
    change: impl FnOnce(&mut Epoch),
) -> Saved {
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: zone.into(), name: "epoch".into(), fields: RequestedFields::All,
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("published epoch required"); };
    let control = publication_control_record(&record).unwrap();
    let DecodeOutcome::Supported(mut epoch) = Epoch::decode(control.encrypted.as_deref().unwrap()).unwrap() else {
        panic!("supported epoch required");
    };
    epoch.state = EpochState::Complete;
    change(&mut epoch);
    let envelope = epoch.encode().unwrap();
    let response = service.request(&scope(), Caller::default(), Operation::Modify {
        zone: zone.into(), atomic: false,
        changes: vec![Mutation::Save {
            record: RecordInput::encrypted("epoch", "MGEpoch", envelope.clone()),
            if_unchanged: Some(record.system_fields),
        }],
    }).unwrap();
    let Ok(Reply::Modified(mut results)) = response.result else { panic!("epoch save required"); };
    let Ok(MutationSuccess::Saved(saved)) = results.remove(0).result else { panic!("conditional epoch save required"); };
    Saved { name: saved.name, envelope, system_fields: saved.system_fields.bytes().to_vec() }
}

#[test]
fn epoch_crt_fetched_complete_updates_both_runtime_intakes_and_survives_reopen() {
    let (_temp, dir, host, mut service) = setup();
    own_until_scan(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    let (sdk, _, zone) = runtime.register_sdk(false).unwrap();
    let published_crt = runtime.epoch_crt_for_testing().unwrap().unwrap();
    let changed = save_complete_epoch(&mut service, &zone, |epoch| epoch.crt += 1);
    runtime.adapter_fetched(&sdk, &zone, &[changed], &[]).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(published_crt + 1));
    // The non-SDK production intake has the same atomic producer.
    let restored = save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = published_crt);
    runtime.receive_collection_records(&runtime.instance().unwrap(), &[restored]).unwrap();
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), Some(published_crt));
    host.close().unwrap();
    drop(host);
    let reopened = open_prior_icloud_host(&dir, keys());
    assert_eq!(reopened.cloud_runtime().unwrap().epoch_crt_for_testing().unwrap(), Some(published_crt));
    reopened.close().unwrap();
}
