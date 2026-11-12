//! Service transport for issued media work; Runtime retains queue decisions.
use super::*;
use cloud::{
    media::{BlobRead, BlobReadResult, Reply as MediaReply, Request, Work},
    runtime::RuntimeError,
    workflow::{PendingChange, SendError},
};
use meguri_testkit::cloud::{Asset, ErrorCode, ServiceError};
use std::io::{Read, Write};

pub fn flush_collection(
    host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str, remaining: &mut usize,
) {
    let runtime = host.cloud_runtime().unwrap();
    for _ in 0..=*remaining {
        let batch = runtime.build_collection_batch(sdk, &host.collection().unwrap(), &entity_names(runtime)).unwrap();
        if batch.saves.is_empty() { return; }
        consume_step(remaining);
        assert!(send_actual_batch_with_instance(host, service, &batch, sdk.into()).is_empty());
        let names = batch.saves.iter().map(|save| save.name.clone()).collect::<Vec<_>>();
        runtime.sdk_saved_removed(sdk, batch.id, &names).unwrap();
    }
    panic!("collection setup exceeded protocol budget");
}

fn consume_step(remaining: &mut usize) {
    *remaining = remaining.checked_sub(1).expect("media setup exceeded protocol budget");
}

pub fn prepare_upload(
    host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str, remaining: &mut usize,
) -> (u64, Work) {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    while *remaining > 0 {
        consume_step(remaining);
        let work = runtime.cloud_media_work(&collection, &host.media().unwrap()).unwrap().expect("queued setup work");
        // ACK collection metadata only for an issued outgoing blob batch;
        // reads and incoming resolution must retain local unacknowledged edits.
        if matches!(work.request, Request::Upload { .. } | Request::Delete { .. }) {
            flush_collection(host, service, sdk, remaining);
        }
        if matches!(work.request, Request::Upload { .. }) {
            return prepare_batch(host, sdk, &work);
        }
        deliver(host, service, sdk, &work).unwrap();
    }
    panic!("upload setup exceeded protocol budget");
}

pub fn prepare_download(
    host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str, remaining: &mut usize,
) -> Work {
    while *remaining > 0 {
        consume_step(remaining);
        let work = host.cloud_runtime().unwrap().cloud_media_work(&host.collection().unwrap(), &host.media().unwrap())
            .unwrap().expect("queued download work");
        if matches!(work.request, Request::Download { .. }) { return work; }
        if matches!(work.request, Request::Upload { .. } | Request::Delete { .. }) {
            flush_collection(host, service, sdk, remaining);
        }
        deliver(host, service, sdk, &work).unwrap();
    }
    panic!("download setup exceeded protocol budget");
}

fn prepare_batch(host: &CoreHost, sdk: &str, work: &Work) -> (u64, Work) {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let (name, action) = match &work.request {
        Request::Upload { name, .. } => {
            runtime.queue_cloud_media_upload(sdk, &collection, work).unwrap();
            (name, PendingChange::Save)
        }
        Request::Delete { name } => (name, PendingChange::Delete),
        _ => panic!("issued media mutation required"),
    };
    let batch = runtime.build_cloud_media_batch(sdk, &collection, &BTreeMap::from([(name.clone(), action)]))
        .unwrap().expect("collection setup must be ACKed before media send");
    assert_eq!(batch.1.request, work.request);
    if matches!(work.request, Request::Upload { .. }) {
        assert_eq!(&batch.1, work);
    }
    batch
}

pub fn send_batch(host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str, id: u64, work: &Work) {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    assert_eq!(runtime.validate_cloud_media_batch(sdk, &collection, id).unwrap(), *work);
    let mutation = match &work.request {
        Request::Upload { name, envelope, staged } => {
            let mut bytes = Vec::new();
            std::fs::File::open(staged).unwrap().take(cloud::media::MAX_ASSET_BYTES + 1)
                .read_to_end(&mut bytes).unwrap();
            assert!(bytes.len() as u64 <= cloud::media::MAX_ASSET_BYTES);
            let mut record = RecordInput::encrypted(name, "MGBlob", envelope.clone());
            record.fields.insert("a".into(), Field::Asset(Asset::synthetic(bytes)));
            Mutation::Save { record, if_unchanged: None }
        }
        Request::Delete { name } => Mutation::Delete { name: name.clone() },
        _ => panic!("original media mutation required"),
    };
    let name = match &mutation {
        Mutation::Save { record, .. } => record.name.clone(),
        Mutation::Delete { name } => name.clone(),
    };
    let response = service.request(&scope(), Caller::default(), Operation::Modify {
        zone: "meguri-media".into(), atomic: false, changes: vec![mutation],
    }).unwrap();
    let mut saved = Vec::new();
    let mut conflicts = Vec::new();
    let mut failed = Vec::new();
    let mut deleted = Vec::new();
    let result = match response.result {
        Ok(Reply::Modified(mut items)) => {
            assert_eq!(items.len(), 1);
            let item = items.remove(0);
            assert_eq!(item.name, name);
            item.result
        }
        Err(error) => Err(error),
        _ => panic!("actual media mutation result required"),
    };
    match result {
        Ok(MutationSuccess::Saved(record)) => saved.push(saved_record(record)),
        Ok(MutationSuccess::Deleted) => deleted.push(name.clone()),
        Err(ServiceError { code: ErrorCode::ServerRecordChanged, server_record: Some(record), .. }) => {
            conflicts.push(saved_record(*record));
        }
        Err(error) => failed.push((name, match error.code {
            ErrorCode::LimitExceeded => SendError::LimitExceeded,
            ErrorCode::BatchRequestFailed => SendError::BatchRequestFailed,
            ErrorCode::ZoneNotFound => SendError::ZoneMissing,
            ErrorCode::UnknownItem => SendError::UnknownItem,
            _ => SendError::Other,
        })),
    }
    let removed = saved.iter().map(|record| record.name.clone()).chain(deleted.iter().cloned()).collect::<Vec<_>>();
    runtime.enter_sent_handler(sdk, id).unwrap();
    let (failed_save, failed_delete) = if matches!(work.request, Request::Delete { .. }) {
        (Vec::new(), failed)
    } else { (failed, Vec::new()) };
    assert!(runtime.cloud_media_sdk_sent(sdk, &collection, id, &saved, &conflicts, &failed_save,
        &deleted, &failed_delete, &[]).unwrap());
    assert_eq!(runtime.return_sent_handler(sdk).unwrap(), id);
    runtime.sdk_saved_removed(sdk, id, &removed).unwrap();
    assert!(failed_save.is_empty() && failed_delete.is_empty(), "media setup request failed");
    if matches!(work.request, Request::Upload { .. }) {
        assert_eq!(saved.len(), 1, "setup upload must receive an actual Saved result");
    } else {
        assert_eq!(deleted.len(), 1, "setup delete must receive an actual Deleted result");
    }
}

fn saved_record(record: meguri_testkit::cloud::Record) -> Saved {
    assert_eq!(record.record_type, "MGBlob");
    let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else { panic!("encrypted blob required"); };
    Saved { name: record.name, envelope: envelope.clone(), system_fields: record.system_fields.bytes().to_vec() }
}

fn failed(error: ServiceError) -> MediaReply {
    match error.code {
        ErrorCode::UnknownItem => MediaReply::UnknownItem,
        ErrorCode::ZoneNotFound => MediaReply::ZoneMissing,
        code => MediaReply::Failed { code: code as i64 },
    }
}

pub fn deliver(host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str, work: &Work) -> Result<(), RuntimeError> {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.validate_cloud_media_work(&collection, work)?;
    if matches!(work.request, Request::Upload { .. } | Request::Delete { .. }) {
        let (id, original) = prepare_batch(host, sdk, work);
        send_batch(host, service, sdk, id, &original);
        return Ok(());
    }
    let mut asset_owner = None;
    let reply = match &work.request {
        Request::Reads { names } => {
            assert!(!names.is_empty() && names.len() <= 200);
            let mut results = Vec::new();
            for name in names {
                runtime.validate_cloud_media_work(&collection, work)?;
                let response = service.request(&scope(), Caller::default(), Operation::Read {
                    zone: "meguri-media".into(), name: name.clone(), fields: RequestedFields::Named(vec!["e".into()]),
                }).unwrap();
                let result = match response.result {
                    Ok(Reply::Record(record)) => {
                        assert_eq!(record.name, *name);
                        assert_eq!(record.record_type, "MGBlob");
                        assert!(!record.fields.contains_key("a"));
                        let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else { panic!("encrypted blob metadata required"); };
                        BlobReadResult::Present { envelope: envelope.clone() }
                    }
                    Err(error) => match error.code {
                        ErrorCode::UnknownItem => BlobReadResult::UnknownItem,
                        ErrorCode::ZoneNotFound => BlobReadResult::ZoneMissing,
                        code => BlobReadResult::Failed { code: code as i64 },
                    },
                    _ => panic!("actual blob metadata required"),
                };
                results.push(BlobRead { name: name.clone(), result });
            }
            MediaReply::BlobReads { results }
        }
        Request::Root | Request::Read { .. } | Request::Download { .. } => {
            let (name, fields) = match &work.request {
                Request::Root => ("root", vec!["e".into()]),
                Request::Read { name } => (name.as_str(), vec!["e".into()]),
                Request::Download { name } => (name.as_str(), vec!["e".into(), "a".into()]),
                _ => unreachable!(),
            };
            let response = service.request(&scope(), Caller::default(), Operation::Read {
                zone: "meguri-media".into(), name: name.into(), fields: RequestedFields::Named(fields),
            }).unwrap();
            match response.result {
                Ok(Reply::Record(record)) => {
                    assert_eq!(record.name, name);
                    assert_eq!(record.record_type, if matches!(work.request, Request::Root) { "MGMediaRoot" } else { "MGBlob" });
                    let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else { panic!("encrypted media required"); };
                    if matches!(work.request, Request::Download { .. }) {
                        let Some(Field::Asset(asset)) = record.fields.get("a") else { panic!("actual asset required"); };
                        let mut file = tempfile::NamedTempFile::new().unwrap();
                        file.write_all(asset.bytes()).unwrap();
                        file.as_file().sync_all().unwrap();
                        asset_owner = Some(file);
                    }
                    MediaReply::Record { name: record.name, record_type: record.record_type, envelope: envelope.clone(),
                        asset: asset_owner.as_ref().map(|file| file.path().to_owned()) }
                }
                Err(error) => failed(error),
                _ => panic!("actual media read required"),
            }
        }
        Request::Upload { .. } | Request::Delete { .. } => unreachable!(),
    };
    let result = runtime.cloud_media_reply(&collection, &host.media().unwrap(), work, reply);
    drop(asset_owner);
    result
}
