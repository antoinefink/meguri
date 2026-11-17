//! Production incremental reconciliation (N1-N15, H1-H18). Entry requires
//! separately disclosed local/remote authority; Settings directional transfers
//! do not route here. No lifecycle hook, timer, or media follow-up starts it.
mod commit;
mod holdback;
mod outgoing;
#[cfg(all(test, feature = "test-hooks"))]
mod repeated;
mod sanity;
mod stage;
#[cfg(feature = "test-hooks")]
pub mod test_hooks;
use crate::wire::{
    changes::decode_changes,
    chunk::decode_chunk,
    graves::{decode_graves, encode_start},
    sanity::{SanityAnswer, decode_finish, decode_sanity_answer, encode_sanity_check},
};
use crate::{
    ErrorKind, HostKey, Job, JobError, JobOptions, Method, Payload, SyncTransport,
    status::{self, CheckResult, StatusMemory, SyncStatus},
};
pub use commit::MergeOutcome;
use meguri_engine::{
    Collection, Engine, EngineError, FailedReason, JobHandle, JobKind, LifecycleState, OpSource, OpSpec, ProgressPhase,
    ProgressSink,
};
use meguri_model::{clock::Clock, rng::RandomSource, time::EpochMillis};
use meguri_storage::{ReadRows, ReaderRole, StampMode, StorageError, normal_stage::NormalStage};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
pub(super) type Result<T> = std::result::Result<T, JobError>;

#[derive(Debug)]
pub struct Outcome {
    pub check: status::CheckOutcome,
    pub merge: Option<MergeOutcome>,
}
pub struct Task {
    pub handle: JobHandle,
    result: mpsc::Receiver<Result<Outcome>>,
    finished: meguri_engine::Subscription,
    finish_sent: Arc<AtomicBool>,
}
impl Task {
    pub fn finish_sent(&self) -> bool { self.finish_sent.load(Ordering::SeqCst) }
    pub fn wait(self) -> Result<Outcome> {
        let result = self.result.recv().unwrap_or_else(|_| {
            Err(JobError::new(
                if self.handle.cancel.stop_cause() == Some(meguri_engine::StopCause::User) {
                    ErrorKind::Cancelled
                } else {
                    ErrorKind::Interrupted
                },
                None,
                "normal job ended before delivering its result",
            ))
        });
        while let Some(event) = self.finished.recv() {
            if matches!(event.kind,meguri_engine::EventKind::JobFinished{id,..} if id==self.handle.id) {
                break;
            }
        }
        result
    }
}
/// A real core maintenance job, over the same transport used by every sync
/// component. Captures the original collection handle; never reacquires a
/// replacement generation. The caller owns its collection's status memory.
#[allow(clippy::too_many_arguments)]
pub fn start(
    engine: &Engine,
    collection: Collection,
    options: JobOptions,
    key: HostKey,
    clock: Arc<dyn Clock>,
    random: Arc<dyn RandomSource>,
    memory: Arc<Mutex<StatusMemory>>,
    transport: Option<Box<dyn SyncTransport>>,
) -> std::result::Result<Task, EngineError> {
    start_owned(engine, collection, options, key, clock, random, memory, transport, None)
}
/// Additive provider entry; the returned task owns the actual NormalSync job.
#[allow(clippy::too_many_arguments)]
pub fn start_admitted(
    engine: &Engine, collection: Collection, options: JobOptions, key: HostKey,
    clock: Arc<dyn Clock>, random: Arc<dyn RandomSource>, memory: Arc<Mutex<StatusMemory>>,
    transport: Option<Box<dyn SyncTransport>>, admission: crate::provider::MeguriRoundAdmission,
) -> std::result::Result<Task, EngineError> {
    start_owned(engine, collection, options, key, clock, random, memory, transport, Some(admission))
}
#[allow(clippy::too_many_arguments)]
fn start_owned(
    engine: &Engine, collection: Collection, options: JobOptions, key: HostKey,
    clock: Arc<dyn Clock>, random: Arc<dyn RandomSource>, memory: Arc<Mutex<StatusMemory>>,
    transport: Option<Box<dyn SyncTransport>>, admission: Option<crate::provider::MeguriRoundAdmission>,
) -> std::result::Result<Task, EngineError> {
    if collection.generation() != engine.generation() {
        return Err(EngineError::StaleGeneration {
            planned: collection.generation(),
            current: engine.generation(),
        });
    }
    let engine = engine.clone();
    let owner = engine.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    let finished = engine.stream().subscribe();
    let finish_sent = Arc::new(AtomicBool::new(false));
    let sent = finish_sent.clone();
    let handle = engine.start_job(JobKind::NormalSync, move |ctx| {
        let options = options.cancel(ctx.cancel.clone());
        let created = match transport {
            Some(t) => Job::with_transport(&options, Some(key), random.as_ref(), t),
            None => Job::new(&options, Some(key), random.as_ref()),
        };
        let mut job = match created {
            Ok(job) => job,
            Err(e) => {
                let _ = tx.send(Err(JobError::from_transport(e, None, options.kind)));
                return Err(EngineError::Internal {
                    message: "normal transport initialization".into(),
                });
            }
        };
        if let Some(admission) = admission { job.admit_meguri(admission); }
        let result = run(
            &owner,
            &collection,
            &mut job,
            clock.as_ref(),
            &memory,
            ctx.progress.clone(),
            sent,
        );
        let error = result.as_ref().err().map(|e| {
            if e.kind == ErrorKind::Cancelled {
                EngineError::Cancelled
            } else {
                EngineError::Internal {
                    message: format!("normal sync: {}", e.kind.id()),
                }
            }
        });
        let _ = tx.send(result);
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    })?;
    Ok(Task {
        handle,
        result: rx,
        finished,
        finish_sent,
    })
}
/// Blocking production execution used by the core job and focused mechanism
/// callers. It owns the state through cancellation instead of dropping it.
fn run(
    engine: &Engine,
    collection: &Collection,
    job: &mut Job,
    clock: &dyn Clock,
    memory: &Mutex<StatusMemory>,
    progress: ProgressSink,
    finish: Arc<AtomicBool>,
) -> Result<Outcome> {
    let result = crate::transport::runtime::drive(engine.threads(), async {
        run_inner(engine, collection, job, clock, memory, progress, finish).await
    })
    .map_err(|e| JobError::from_transport(e, None, job.kind()))?;
    result.map_err(|error| {
        if error.kind == ErrorKind::Cancelled
            && !job.admission_refused()
            && job.cancellation().stop_cause() == Some(meguri_engine::StopCause::System)
        {
            stopped(job)
        } else {
            error
        }
    })
}
fn checkpoint(engine: &Engine, collection: &Collection, job: &Job) -> Result<()> {
    if matches!(
        engine.state(),
        LifecycleState::Failed {
            reason: FailedReason::Corrupt { .. },
            ..
        }
    ) {
        return Err(JobError::new(
            ErrorKind::CollectionCorrupt,
            None,
            "collection lifecycle is corrupt",
        ));
    }
    if !collection.is_current() {
        return Err(JobError::new(
            ErrorKind::Interrupted,
            None,
            "original collection generation ended",
        ));
    }
    if job.cancellation().is_cancelled() {
        return Err(stopped(job));
    }
    job.check_admission()
}
async fn run_inner(
    engine: &Engine,
    collection: &Collection,
    job: &mut Job,
    clock: &dyn Clock,
    memory: &Mutex<StatusMemory>,
    progress: ProgressSink,
    finish: Arc<AtomicBool>,
) -> Result<Outcome> {
    checkpoint(engine, collection, job)?;
    progress.set_phase(ProgressPhase::Named("connecting"));
    collection.check_rollover().map_err(engine_error)?;
    let mut reader = collection.reader(ReaderRole::Sync).map_err(engine_error)?;
    let local = status::LocalRecord::read(&*reader)?;
    checkpoint(engine, collection, job)?;
    let mut check = status::check(job, &local, clock).await?;
    if !matches!(check.result, CheckResult::Normal { .. }) {
        memory.lock().unwrap().remember(
            job.base(),
            job.host_key(),
            if matches!(check.result, CheckResult::NoChanges) {
                SyncStatus::NoChanges
            } else {
                SyncStatus::FullSync
            },
            clock,
        );
        return Ok(Outcome { check, merge: None });
    }
    // This guard also clears flags if staging fails or a caller unwinds.
    struct Flags<'a> {
        collection: &'a Collection,
        finish: Arc<AtomicBool>,
    }
    impl Drop for Flags<'_> {
        fn drop(&mut self) {
            if !self.finish.load(Ordering::SeqCst) {
                let _ = self.collection.sync_failed_before_finish();
            }
        }
    }
    let _flags = Flags {
        collection,
        finish: finish.clone(),
    };
    let (snapshot, hold) = reader.capture_sync(collection).map_err(engine_error)?;
    #[cfg(feature = "test-hooks")]
    test_hooks::reach(test_hooks::Point::Snapshot(hold.snapshot));
    let captured = status::LocalRecord::read(&snapshot)?;
    let schema = captured.schema_modified;
    checkpoint(engine, collection, job)?;
    if schema != local.schema_modified {
        // Preserve the schema admission made before meta, even if mod now equals the server's.
        check.result = CheckResult::OneWay {
            upload: captured.has_cards || check.server.empty,
            download: !check.server.empty || !captured.has_cards,
        };
        memory
            .lock()
            .unwrap()
            .remember(job.base(), job.host_key(), SyncStatus::FullSync, clock);
        return Ok(Outcome { check, merge: None });
    }
    let budget = std::fs::metadata(collection.path())
        .map_err(io_error)?
        .len()
        .saturating_mul(3)
        .saturating_add(16 * 1024 * 1024);
    collection.require_space(budget).map_err(engine_error)?;
    let disk = NormalStage::create(collection.path()).map_err(storage_error)?;
    holdback::build(&disk, &snapshot, &hold, clock, &job.cancellation())?;
    let newer = matches!(check.result, CheckResult::Normal { local_is_newer: true });
    let usn = check.server.usn;
    let mut started = false;
    let result: Result<MergeOutcome> = async {
        checkpoint(engine, collection, job)?;
        progress.set_phase(ProgressPhase::Named("syncing"));
        started = true;
        let graves =
            decode_graves(&request(job, Method::Start, Payload::Json(encode_start(check.local_usn, newer))).await?)?;
        stage::record_graves(&disk, &graves, usn, clock)?;
        let mut received = graves.len() as u64;
        let mut sent = 0;
        progress.report_objects(sent, received);
        checkpoint(engine, collection, job)?;
        let mut cursor = outgoing::GravesCursor::new();
        while let Some((body, count)) = cursor.next(&disk)? {
            drain(job, Method::ApplyGraves, Payload::Json(body)).await?;
            sent += count;
            progress.report_objects(sent, received);
        }
        stage::graves(&disk, &graves, usn, clock)?;
        let (body, count) = outgoing::changes(&disk, newer, usn)?;
        let changes = decode_changes(&request(job, Method::ApplyChanges, Payload::Json(body)).await?)?;
        sent += count;
        received +=
            (changes.notetypes.len() + changes.decks.len() + changes.deck_configs.len() + changes.tags.len()) as u64;
        let meta = stage::objects(&disk, changes, usn, clock)?;
        progress.report_objects(sent, received);
        loop {
            let chunk = decode_chunk(&request(job, Method::Chunk, Payload::empty()).await?)?;
            let done = chunk.done;
            received += stage::chunk(&disk, chunk, clock)?;
            progress.report_objects(sent, received);
            if done {
                break;
            }
        }
        checkpoint(engine, collection, job)?;
        let mut chunks = outgoing::Chunks::new();
        loop {
            checkpoint(engine, collection, job)?;
            let (body, count, done) = chunks.next(&disk, usn)?;
            drain(job, Method::ApplyChunk, Payload::Json(body)).await?;
            sent += count;
            progress.report_objects(sent, received);
            if done {
                break;
            }
        }
        progress.set_phase(ProgressPhase::Named("finalising"));
        let counts = sanity::counts(&disk)?;
        if let SanityAnswer::Bad { client, server } = decode_sanity_answer(
            &request(job, Method::SanityCheck2, Payload::Json(encode_sanity_check(&counts))).await?,
        )? {
            return Err(JobError::new(
                ErrorKind::SanityCheckFailed { client, server },
                Some(Method::SanityCheck2),
                "merged counts refused",
            ));
        }
        checkpoint(engine, collection, job)?;
        let sent_flag = finish.clone();
        let original = collection.clone();
        job.on_finish_sent(move || {
            sent_flag.store(true, Ordering::SeqCst);
            let _ = original.sync_finish_sent();
        });
        let time = decode_finish(&request(job, Method::Finish, Payload::empty()).await?)?;
        if !finish.load(Ordering::SeqCst) {
            return Err(JobError::new(
                ErrorKind::Interrupted,
                Some(Method::Finish),
                "finish body was not consumed",
            ));
        }
        checkpoint(engine, collection, job)?;
        let input = commit::MergeInput { hold, schema, usn, time, meta };
        match job.meguri_admission() {
            Some(admission) => commit::apply_admitted(collection, &disk, input, job.cancellation(), progress.clone(), Some(admission)),
            None => commit::apply(collection, &disk, input, job.cancellation(), progress.clone()),
        }
    }
    .await;
    drop(snapshot);
    drop(reader);
    match result {
        Ok(merge) => {
            job.check_admission()?;
            collection.check_rollover().map_err(engine_error)?;
            memory.lock().unwrap().remember(
                job.base(),
                job.host_key(),
                if merge.one_way {
                    SyncStatus::FullSync
                } else {
                    SyncStatus::NoChanges
                },
                clock,
            );
            Ok(Outcome {
                check,
                merge: Some(merge),
            })
        }
        Err(mut error) => {
            // Drop the stage before the raw scm-only maintenance operation.
            drop(disk);
            if started {
                job.abort();
            }
            if finish.load(Ordering::SeqCst) {
                if !matches!(error.kind, ErrorKind::DiskFull { .. } | ErrorKind::LocalStorage) {
                    error = JobError::new(ErrorKind::Interrupted, error.method, "finish may have committed");
                }
            } else if job.cancellation().is_cancelled() && !job.admission_refused()
                && (job.meguri_admission().is_none() || error.kind == ErrorKind::Cancelled) {
                error = stopped(job);
            }
            if matches!(error.kind, ErrorKind::SanityCheckFailed { .. }) {
                let now = EpochMillis::from_clock(clock);
                collection
                    .run(
                        OpSpec::named("sync_sanity_failed", OpSource::Maintenance).stamp(StampMode::Raw),
                        move |ctx| {
                            let mut col = ctx.scope().col()?;
                            col.schema_modified = now;
                            ctx.scope().put_col(&col)?;
                            Ok(())
                        },
                    )
                    .map_err(engine_error)?;
            }
            Err(error)
        }
    }
}
async fn request(job: &mut Job, method: Method, payload: Payload) -> Result<Vec<u8>> {
    match job.request(method, payload).await {
        Ok(body) => body.bytes().await,
        Err(e) => Err(e),
    }
    .map_err(|e| job.error_for(method, e))
}
async fn drain(job: &mut Job, method: Method, payload: Payload) -> Result<()> {
    let mut body = job
        .request(method, payload)
        .await
        .map_err(|e| job.error_for(method, e))?;
    while body.chunk().await.map_err(|e| job.error_for(method, e))?.is_some() {}
    Ok(())
}
fn stopped(job: &Job) -> JobError {
    JobError::new(
        if job.cancellation().stop_cause() == Some(meguri_engine::StopCause::System) {
            ErrorKind::Interrupted
        } else {
            ErrorKind::Cancelled
        },
        None,
        "normal job stopped",
    )
}
pub(super) fn missing_outgoing() -> JobError {
    JobError::new(
        ErrorKind::Internal,
        None,
        "outgoing view contains an unsent pending key",
    )
}
pub(super) fn storage_error(error: StorageError) -> JobError {
    if let StorageError::InsufficientSpace { required, available } = &error {
        return JobError::new(
            ErrorKind::DiskFull {
                needed_bytes: Some(*required),
                available_bytes: Some(*available),
            },
            None,
            "normal staging capacity",
        );
    }
    let mut error = JobError::from_storage(&error);
    error.detail = "normal sync local storage".into();
    error
}
fn io_error(error: std::io::Error) -> JobError {
    JobError::new(
        if error.kind() == std::io::ErrorKind::StorageFull {
            ErrorKind::DiskFull {
                needed_bytes: None,
                available_bytes: None,
            }
        } else {
            ErrorKind::LocalStorage
        },
        None,
        "normal staging file",
    )
}
pub(super) fn engine_error(error: EngineError) -> JobError {
    match error {
        EngineError::SyncNote(fault) => stage::note_fault(fault),
        EngineError::Storage(e) => storage_error(e),
        EngineError::Cancelled => JobError::new(ErrorKind::Cancelled, None, "normal job cancelled"),
        EngineError::Busy { .. } => JobError::new(ErrorKind::ExclusiveBusy, None, "normal job already active"),
        _ => JobError::new(ErrorKind::LocalStorage, None, "normal engine operation"),
    }
}
