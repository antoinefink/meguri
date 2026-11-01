//! Thin transport for typed Runtime media work. No conflict, feed or adoption
//! decisions belong here. Assets are actual Service bytes and actual files.
use super::*;
use cloud::media::{Reply as MediaReply, Request as MediaRequest, Work as MediaWork};
use std::io::{Read, Write};
impl SDKRig {
    pub(in crate::cloud) fn drive_cloud_media(
        &mut self,
        service: &mut Service<SeededRandom>,
        remaining: &mut usize,
    ) -> Result<bool> {
        self.updates()?;
        while *remaining > 0 {
            let facts: cloud::testing::PublicationControl = self.driver.call(Input::PublicationControl)?;
            if !facts
                .current
                .as_ref()
                .is_some_and(|b| b.facts.state == cloud::BindingState::Active)
            {
                return Ok(false);
            }
            let work: Option<MediaWork> = self.driver.call(Input::MediaWork)?;
            if self.updates()? {
                return Ok(true);
            }
            let Some(work) = work else {
                *remaining -= 1;
                return self.send_pending_media(service);
            };
            *remaining -= 1;
            if matches!(work.request, MediaRequest::Delete { .. }) {
                return self.send_pending_media(service);
            }
            if matches!(work.request, MediaRequest::Upload { .. }) {
                if self.driver.call::<bool>(Input::MediaQueue(work.clone()))? {
                    self.updates()?;
                    if self.send_pending_media(service)? {
                        continue;
                    }
                    return Ok(false);
                }
            }
            self.driver.call::<()>(Input::MediaValidate(work.clone()))?;
            let operation = match &work.request {
                MediaRequest::Delete { .. } => return Err(failure()),
                MediaRequest::Reads { names } => {
                    if names.is_empty() || names.len() > 200 {
                        return Err(failure());
                    }
                    let mut results = Vec::new();
                    for name in names {
                        self.driver.call::<()>(Input::MediaValidate(work.clone()))?;
                        let response = service.request(
                            &self.files.scope,
                            Caller::default(),
                            Operation::Read {
                                zone: "meguri-media".into(),
                                name: name.clone(),
                                fields: RequestedFields::Named(vec!["e".into()]),
                            },
                        )?;
                        let result = match response.result {
                            Ok(Reply::Record(record)) => {
                                let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                                    return Err(failure());
                                };
                                if record.name != *name
                                    || record.record_type != "MGBlob"
                                    || record.fields.contains_key("a")
                                {
                                    return Err(failure());
                                }
                                cloud::media::BlobReadResult::Present {
                                    envelope: envelope.clone(),
                                }
                            }
                            Err(error) => match error.code {
                                ErrorCode::UnknownItem => cloud::media::BlobReadResult::UnknownItem,
                                ErrorCode::ZoneNotFound => cloud::media::BlobReadResult::ZoneMissing,
                                code => cloud::media::BlobReadResult::Failed { code: code as i64 },
                            },
                            _ => return Err(failure()),
                        };
                        results.push(cloud::media::BlobRead {
                            name: name.clone(),
                            result,
                        });
                    }
                    self.driver
                        .call::<()>(Input::MediaReply(work.clone(), MediaReply::BlobReads { results }))?;
                    if self.updates()? {
                        return Ok(true);
                    }
                    continue;
                }
                MediaRequest::Root => Operation::Read {
                    zone: "meguri-media".into(),
                    name: "root".into(),
                    fields: RequestedFields::Named(vec!["e".into()]),
                },
                MediaRequest::Read { name } => Operation::Read {
                    zone: "meguri-media".into(),
                    name: name.clone(),
                    fields: RequestedFields::Named(vec!["e".into()]),
                },
                MediaRequest::Download { name } => Operation::Read {
                    zone: "meguri-media".into(),
                    name: name.clone(),
                    fields: RequestedFields::Named(vec!["e".into(), "a".into()]),
                },
                MediaRequest::Upload { name, envelope, staged } => {
                    let file = std::fs::File::open(staged)?;
                    let mut bytes = Vec::new();
                    file.take(cloud::media::MAX_ASSET_BYTES + 1).read_to_end(&mut bytes)?;
                    if bytes.len() as u64 > cloud::media::MAX_ASSET_BYTES {
                        return Err(failure());
                    }
                    let mut record = RecordInput::encrypted(name, "MGBlob", envelope.clone());
                    record.fields.insert("a".into(), Field::Asset(Asset::synthetic(bytes)));
                    Operation::Modify {
                        zone: "meguri-media".into(),
                        atomic: false,
                        changes: vec![Mutation::Save {
                            record,
                            if_unchanged: None,
                        }],
                    }
                }
            };
            let response = service.request(
                &self.files.scope,
                Caller {
                    own_operation: Some("media-runtime".into()),
                    ..Default::default()
                },
                operation,
            )?;
            let mut asset_owner = None;
            let reply = match response.result {
                Ok(Reply::Record(record)) => {
                    let asset = if let Some(Field::Asset(asset)) = record.fields.get("a") {
                        let mut file = tempfile::NamedTempFile::new()?;
                        file.write_all(asset.bytes())?;
                        file.as_file().sync_all()?;
                        let path = file.path().to_path_buf();
                        asset_owner = Some(file);
                        Some(path)
                    } else {
                        None
                    };
                    let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                        return Err(failure());
                    };
                    MediaReply::Record {
                        name: record.name,
                        record_type: record.record_type,
                        envelope: envelope.clone(),
                        asset,
                    }
                }
                Ok(Reply::Modified(mut results)) if results.len() == 1 => match results.remove(0).result {
                    Ok(MutationSuccess::Saved(record)) => saved(record)?,
                    Err(ServiceError {
                        code: ErrorCode::ServerRecordChanged,
                        server_record: Some(record),
                        ..
                    }) => saved(*record)?,
                    Err(error) => failed(error),
                    _ => return Err(failure()),
                },
                Err(error) => failed(error),
                _ => return Err(failure()),
            };
            self.driver.call::<()>(Input::MediaReply(work, reply))?;
            drop(asset_owner);
            if self.updates()? {
                return Ok(true);
            }
        }
        Err(failure())
    }
}
impl PublicationRig {
    /// Injection reaches the next owned staged-file read. Releasing a latch
    /// resumes the original work, whose real intent survives that I/O failure.
    pub fn fail_held_io(&mut self, point: Point) -> Result<()> {
        if !self.is_held(point)
            || !matches!(
                point,
                Point::ResolutionMoveBeforeRewrite | Point::MediaTrashBeforeInstall
            )
        {
            return Err(failure());
        }
        self.sdk.get_mut().driver.call::<()>(Input::FailMediaIo)?;
        self.release(point)?;
        self.sdk.get_mut().driver.complete_pending_media_io_failure()
    }
}
fn saved(record: Record) -> Result<MediaReply> {
    let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
        return Err(failure());
    };
    Ok(MediaReply::Saved {
        name: record.name,
        envelope: envelope.clone(),
    })
}
fn failed(error: ServiceError) -> MediaReply {
    match error.code {
        ErrorCode::UnknownItem => MediaReply::UnknownItem,
        ErrorCode::ZoneNotFound => MediaReply::ZoneMissing,
        _ => MediaReply::Failed {
            code: error.code as i64,
        },
    }
}
