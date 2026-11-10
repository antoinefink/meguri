//! Opaque original Host/work authority. Native requests are projections only.
use crate::cloud_policy::policy_jobs::{Cell, Completion};
use crate::{
    CoreError, callback,
    sync::{CloudWorkflow, cloud_error},
};
use meguri_sync::cloud::media as m;
use meguri_sync::cloud::{policy::PolicyWork, runtime::RuntimeError};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, uniffi::Enum)]
pub enum CloudMediaRequest {
    Delete {
        name: String,
    },
    Root,
    Read {
        name: String,
    },
    Reads {
        names: Vec<String>,
    },
    Download {
        name: String,
    },
    Upload {
        name: String,
        envelope: Vec<u8>,
        staged_path: String,
    },
}
impl From<&m::Request> for CloudMediaRequest {
    fn from(request: &m::Request) -> Self {
        match request {
            m::Request::Delete { name } => Self::Delete { name: name.clone() },
            m::Request::Root => Self::Root,
            m::Request::Read { name } => Self::Read { name: name.clone() },
            m::Request::Reads { names } => Self::Reads { names: names.clone() },
            m::Request::Download { name } => Self::Download { name: name.clone() },
            m::Request::Upload { name, envelope, staged } => Self::Upload {
                name: name.clone(),
                envelope: envelope.clone(),
                staged_path: staged.to_string_lossy().into_owned(),
            },
        }
    }
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudMediaBlobResult {
    Present { envelope: Vec<u8> },
    UnknownItem,
    ZoneMissing,
    Failed { code: i64 },
}
#[derive(Clone, uniffi::Record)]
pub struct CloudMediaBlobRead {
    pub name: String,
    pub result: CloudMediaBlobResult,
}
/// Own record-ID reads cannot acknowledge uploads. Saved enters only through
/// CloudWorkflow.sent with its original SDK batch and retained backend Work.
#[derive(Clone, uniffi::Enum)]
pub enum CloudMediaReply {
    BlobReads {
        results: Vec<CloudMediaBlobRead>,
    },
    Record {
        name: String,
        record_type: String,
        envelope: Vec<u8>,
        asset_path: Option<String>,
    },
    UnknownItem,
    ZoneMissing,
    Failed {
        code: i64,
    },
}
impl From<CloudMediaReply> for m::Reply {
    fn from(reply: CloudMediaReply) -> Self {
        match reply {
            CloudMediaReply::BlobReads { results } => Self::BlobReads {
                results: results
                    .into_iter()
                    .map(|r| m::BlobRead {
                        name: r.name,
                        result: match r.result {
                            CloudMediaBlobResult::Present { envelope } => m::BlobReadResult::Present { envelope },
                            CloudMediaBlobResult::UnknownItem => m::BlobReadResult::UnknownItem,
                            CloudMediaBlobResult::ZoneMissing => m::BlobReadResult::ZoneMissing,
                            CloudMediaBlobResult::Failed { code } => m::BlobReadResult::Failed { code },
                        },
                    })
                    .collect(),
            },
            CloudMediaReply::Record {
                name,
                record_type,
                envelope,
                asset_path,
            } => Self::Record {
                name,
                record_type,
                envelope,
                asset: asset_path.map(Into::into),
            },
            CloudMediaReply::UnknownItem => Self::UnknownItem,
            CloudMediaReply::ZoneMissing => Self::ZoneMissing,
            CloudMediaReply::Failed { code } => Self::Failed { code },
        }
    }
}
#[uniffi::export(foreign)]
pub trait CloudMediaQueue: Send + Sync {
    fn enqueue(&self) -> bool;
}

#[derive(uniffi::Object)]
pub struct CloudMedia {
    host: Arc<meguri_host::CoreHost>,
    generation: u64,
}
#[derive(uniffi::Object)]
pub struct CloudMediaWork {
    owner: Arc<CloudMedia>,
    sdk: String,
    work: m::Work,
}
#[derive(Clone, uniffi::Record)]
pub struct CloudMediaIssuedWork {
    pub handle: Arc<CloudMediaWork>,
    pub request: CloudMediaRequest,
}
// Pending is the outer Option; a completed NoWork is a value with work=None.
#[derive(Clone, uniffi::Record)]
pub struct CloudMediaPreparedWork {
    pub work: Option<CloudMediaIssuedWork>,
}
#[derive(uniffi::Object)]
pub struct CloudMediaNextWorkResult {
    cell: Arc<Cell<CloudMediaPreparedWork>>,
}
#[uniffi::export]
impl CloudMediaNextWorkResult {
    pub fn result(&self) -> Result<Option<CloudMediaPreparedWork>, CoreError> {
        callback::enter("CloudMediaNextWorkResult.result");
        self.cell
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .transpose()
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CloudMediaNextWorkJob {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<CloudMediaNextWorkResult>,
}
fn preparation_error(error: RuntimeError) -> CoreError {
    match error {
        RuntimeError::Engine(meguri_engine::EngineError::Cancelled) => CoreError::Cancelled,
        error => cloud_error(error),
    }
}
impl CloudMedia {
    fn collection(&self) -> Result<meguri_engine::Collection, CoreError> {
        let collection = self.host.cloud_connection_collection()?;
        if collection.generation().0 != self.generation || !collection.is_current() {
            return Err(cloud_error("stale media generation"));
        }
        Ok(collection)
    }
    fn runtime(&self) -> Result<&meguri_sync::cloud::runtime::Runtime, CoreError> {
        self.host
            .cloud_runtime()
            .map(Arc::as_ref)
            .ok_or_else(|| cloud_error("cloud media unavailable"))
    }
    fn owns(&self, work: &CloudMediaWork) -> Result<meguri_engine::Collection, CoreError> {
        if !std::ptr::eq(self, Arc::as_ptr(&work.owner)) {
            return Err(cloud_error("wrong media owner"));
        }
        let collection = self.collection()?;
        let runtime = self.runtime()?;
        runtime
            .validate_cloud_media_admission(&work.sdk, &collection, &work.work)
            .map_err(cloud_error)?;
        Ok(collection)
    }
}
#[uniffi::export]
impl CloudWorkflow {
    pub fn media(&self) -> Result<Arc<CloudMedia>, CoreError> {
        callback::enter("CloudWorkflow.media");
        let generation = self.host.cloud_connection_generation()?.0;
        Ok(Arc::new(CloudMedia {
            host: self.host.clone(),
            generation,
        }))
    }
    /// Final native enqueue seam. The SDK owner supplies its already prepared
    /// batch queue; a batch DTO/id can select only an existing original Work.
    pub fn submit_media_batch(
        &self,
        sdk: String,
        batch: u64,
        queue: Arc<dyn CloudMediaQueue>,
    ) -> Result<bool, CoreError> {
        callback::enter("CloudWorkflow.submit_media_batch");
        self.core
            .validate_cloud_media_batch(&sdk, &self.host.collection()?, batch)
            .map_err(cloud_error)?;
        Ok(callback::invoke("CloudMediaQueue.enqueue", || queue.enqueue()))
    }
}
#[uniffi::export]
impl CloudMedia {
    pub fn start_next_work(self: Arc<Self>, sdk: String) -> Result<CloudMediaNextWorkJob, CoreError> {
        callback::enter("CloudMedia.start_next_work");
        let cell = Arc::new(Cell(Mutex::new(None)));
        let completion = Completion(cell.clone());
        let owner = self.clone();
        let generation = meguri_engine::Generation(self.generation);
        // Admission touches only the retained Host's lifecycle/registry, never
        // Runtime/Driver, media, a provider or a fresh process owner.
        let admission = self
            .host
            .start_cloud_connection_job(generation, "CloudMedia.next_work", move |job| {
                let mut authority =
                    PolicyWork::new(&job.cancel).with_generation(owner.host.engine().downgrade(), generation);
                let cancellation = job.cancel.check().map_err(CoreError::from);
                #[cfg(feature = "test-hooks")]
                let cancellation = cancellation.inspect_err(|_| {
                    eprintln!("cloudMedia.refused site=cancelCheck");
                });
                let value = cancellation
                    .and_then(|()| {
                        let collection = owner.collection();
                        #[cfg(feature = "test-hooks")]
                        let collection = collection.inspect_err(|_| {
                            eprintln!("cloudMedia.refused site=ownerCollection");
                        });
                        collection
                    })
                    .and_then(|collection| {
                        let prepared = owner
                            .host
                            .next_cloud_media_work_with_work(&collection, &sdk, &mut authority);
                        #[cfg(feature = "test-hooks")]
                        let prepared = prepared.inspect_err(|error| {
                            let (kind, variant) = error.refusal_tags_for_testing();
                            eprintln!("cloudMedia.refused site=runtimePreparation errorKind={kind} variant={variant}");
                        });
                        prepared.map_err(preparation_error)
                    })
                    .and_then(|work| {
                        let checked = authority.check();
                        #[cfg(feature = "test-hooks")]
                        let checked = checked.inspect_err(|error| {
                            let (kind, variant) = error.refusal_tags_for_testing();
                            eprintln!("cloudMedia.refused site=authorityCheck errorKind={kind} variant={variant}");
                        });
                        checked.map_err(preparation_error)?;
                        Ok(CloudMediaPreparedWork {
                            work: work.map(|work| CloudMediaIssuedWork {
                                request: (&work.request).into(),
                                handle: Arc::new(CloudMediaWork { owner, sdk, work }),
                            }),
                        })
                    });
                completion.finish(value)
            });
        #[cfg(feature = "test-hooks")]
        let admission = admission.inspect_err(|_| {
            eprintln!("cloudMedia.refused site=admission");
        });
        let handle = admission?;
        Ok(CloudMediaNextWorkJob {
            handle: crate::jobs::wrap(handle),
            result: Arc::new(CloudMediaNextWorkResult { cell }),
        })
    }
    pub fn next_work(self: Arc<Self>, sdk: String) -> Result<Option<CloudMediaIssuedWork>, CoreError> {
        callback::enter("CloudMedia.next_work");
        let collection = self.collection()?;
        let Some(work) = self
            .host
            .next_cloud_media_work_with_work(&collection, &sdk, &mut PolicyWork::default())
            .map_err(cloud_error)?
        else {
            return Ok(None);
        };
        let request = (&work.request).into();
        Ok(Some(CloudMediaIssuedWork {
            handle: Arc::new(CloudMediaWork { owner: self, sdk, work }),
            request,
        }))
    }
    pub fn downloads_blocked(&self, sdk: String) -> Result<bool, CoreError> {
        callback::enter("CloudMedia.downloads_blocked");
        self.collection()?;
        self.runtime()?.validate_sdk(&sdk).map_err(cloud_error)?;
        Ok(self.runtime()?.cloud_media_downloads_blocked())
    }
    /// The only callback is the prepared operation's synchronous database add.
    /// No state/tracking lock survives validation or encloses foreign code.
    pub fn submit_read(&self, work: Arc<CloudMediaWork>, queue: Arc<dyn CloudMediaQueue>) -> Result<bool, CoreError> {
        callback::enter("CloudMedia.submit_read");
        if matches!(work.work.request, m::Request::Upload { .. } | m::Request::Delete { .. }) {
            return Err(cloud_error("media upload requires the registered sync engine"));
        }
        self.owns(&work)?;
        if matches!(work.work.request, m::Request::Download { .. }) && self.runtime()?.cloud_media_downloads_blocked() {
            return Ok(false);
        }
        Ok(callback::invoke("CloudMediaQueue.enqueue", || queue.enqueue()))
    }
    pub fn queue_upload(&self, work: Arc<CloudMediaWork>) -> Result<(), CoreError> {
        callback::enter("CloudMedia.queue_upload");
        let collection = self.owns(&work)?;
        self.runtime()?
            .queue_cloud_media_upload(&work.sdk, &collection, &work.work)
            .map_err(cloud_error)
    }
    pub fn reply(&self, work: Arc<CloudMediaWork>, reply: CloudMediaReply) -> Result<(), CoreError> {
        callback::enter("CloudMedia.reply");
        self.owns(&work)?;
        if matches!(work.work.request, m::Request::Upload { .. } | m::Request::Delete { .. }) {
            return Err(cloud_error("media Saved requires the original sync engine callback"));
        }
        self.host
            .cloud_media_reply(&work.work, reply.into())
            .map_err(cloud_error)
    }
}
#[cfg(test)]
impl CloudMediaWork {
    pub(crate) fn retained_for_test(&self) -> &m::Work {
        &self.work
    }
}
