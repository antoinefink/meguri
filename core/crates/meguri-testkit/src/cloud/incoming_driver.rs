//! IPC stays on the command thread; workers own only Send engine handles.
use super::*;
use std::sync::{Condvar, PoisonError, mpsc};

#[derive(Default)]
struct LatchState {
    armed: BTreeSet<IncomingStage>,
    held: BTreeSet<IncomingStage>,
    events: Option<mpsc::Sender<WorkEvent>>,
    receive_events: Option<mpsc::Sender<WorkEvent>>,
}
#[derive(Default)]
pub(super) struct Latches {
    state: Mutex<LatchState>,
    released: Condvar,
}
impl Latches {
    pub(super) fn hold(&self, stage: IncomingStage) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .armed
            .insert(stage);
    }
    pub(super) fn release(&self, stage: IncomingStage) {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        s.armed.remove(&stage);
        s.held.remove(&stage);
        self.released.notify_all();
    }
    pub(super) fn hit(&self, stage: IncomingStage) {
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !s.armed.contains(&stage) {
            return;
        }
        s.held.insert(stage);
        let events = if matches!(stage, IncomingStage::FetchBeforeInbox | IncomingStage::FetchAfterInbox) {
            s.receive_events.as_ref().or(s.events.as_ref())
        } else {
            s.events.as_ref()
        };
        if let Some(events) = events {
            let _ = events.send(WorkEvent::Paused(stage));
        }
        while s.held.contains(&stage) && s.armed.contains(&stage) {
            s = self.released.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
    }
}
enum WorkEvent {
    Paused(IncomingStage),
    Completed(std::result::Result<serde_json::Value, ChildFailure>),
}
pub(super) struct Work {
    events: mpsc::Receiver<WorkEvent>,
    thread: std::thread::JoinHandle<()>,
    notified_pause: Option<IncomingStage>,
    previous: Option<Box<Work>>,
}
fn advance(device: &mut Device, stream: &mut UnixStream) -> Result<()> {
    let latches = device
        .incoming_latches
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let paused = device.incoming_worker.as_ref().and_then(|work| work.notified_pause);
    if let Some(stage) = paused.filter(|stage| latches.held.contains(stage)) {
        return write_frame(stream, &ChildMessage::Paused(stage));
    }
    drop(latches);
    let event = device
        .incoming_worker
        .as_ref()
        .ok_or_else(failure)?
        .events
        .recv_timeout(IPC_TIMEOUT)?;
    match event {
        WorkEvent::Paused(stage) => {
            device.incoming_worker.as_mut().ok_or_else(failure)?.notified_pause = Some(stage);
            write_frame(stream, &ChildMessage::Paused(stage))
        }
        WorkEvent::Completed(reply) => {
            let mut work = device.incoming_worker.take().ok_or_else(failure)?;
            work.thread.join().map_err(|_| failure())?;
            let mut latches = device
                .incoming_latches
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            device.incoming_worker = work.previous.take().map(|work| *work);
            if device.incoming_worker.is_some() {
                latches.receive_events = None;
            } else {
                latches.events = None;
            }
            drop(latches);
            write_frame(stream, &ChildMessage::Reply(reply))
        }
    }
}
/// A pause is a separate protocol frame. The one original reply is emitted only
/// after release/continuation; queries have their own independent reply frames.
pub(super) fn dispatch(device: &mut Device, input: Input, stream: &mut UnixStream) -> Result<Option<Input>> {
    if matches!(input, Input::ContinueWork) {
        advance(device, stream)?;
        return Ok(None);
    }
    if !matches!(
        input,
        Input::Fetched(..)
            | Input::DeliverCollection(..)
            | Input::DrainIncoming(..)
            | Input::Reconcile
            | Input::Build(..)
            | Input::MediaReply(..)
            | Input::MediaWork
    ) {
        return Ok(Some(input));
    }
    // Receive-only work cannot grant a second writer. The journalled apply's
    // AfterIntent hook holds neither runtime nor tracking locks.
    let nested_receive = matches!(input, Input::DeliverCollection(..))
        && device.incoming_worker.as_ref().is_some_and(|work| {
            work.previous.is_none() && work.notified_pause == Some(IncomingStage::ApplyAfterIntent)
        });
    if device.incoming_worker.is_some() && !nested_receive {
        write_frame(
            stream,
            &ChildMessage::Reply(Err(ChildFailure::Engine(EngineFailure::Busy))),
        )?;
        return Ok(None);
    }
    let runtime = device.host.cloud_runtime().cloned().ok_or_else(failure)?;
    let collection = device.host.collection()?;
    let media = device.host.media()?;
    let unbound = device.stable.is_none();
    let last_fetch = device.last_fetch_engine_state.clone();
    let submission = device.incoming_submission.clone();
    let (events, receiver) = mpsc::channel();
    let mut latches = device
        .incoming_latches
        .state
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if nested_receive {
        latches.receive_events = Some(events.clone());
    } else {
        latches.events = Some(events.clone());
    }
    drop(latches);
    let thread = std::thread::spawn(move || {
        let run = || -> Result<serde_json::Value> {
            match input {
                Input::Fetched(instance, records, zone, serialization, exchange) => {
                    let callback = || -> Result<()> {
                        if runtime.incoming_sdk_instance_for_testing().as_deref() == Some(&instance) {
                            let projection = super::super::adapter::collection_cursor_projection(&serialization)?;
                            *last_fetch.lock().unwrap_or_else(PoisonError::into_inner) = projection;
                            runtime.adapter_fetched(&instance, &zone, &records, &[])?;
                            // The state event follows the durable inbox handler and
                            // precedes any writer grant. Store exactly SDK bytes.
                            runtime.adapter_state_update(&instance, &serialization)?;
                            meguri_model::faultpoint!("cloud.fetch.after_cursor")?;
                            runtime.incoming_stage_for_testing(IncomingStage::FetchAfterCursor);
                        } else {
                            // Legacy explicit full-fetch proof fixtures have no SDK
                            // authority and make no native cursor-resume claim.
                            runtime.receive_collection_records(&instance, &records)?;
                        }
                        runtime.drain_incoming(&collection, IPC_TIMEOUT)?;
                        Ok(())
                    };
                    if let Err(error) = callback() {
                        if let Some(id) = exchange {
                            let cancelled = matches!(
                                error.downcast_ref::<runtime::RuntimeError>(),
                                Some(runtime::RuntimeError::Engine(meguri_engine::EngineError::Cancelled))
                            );
                            let outcome = if cancelled {
                                core::adapter::CollectionOutcome::Cancelled
                            } else {
                                core::adapter::CollectionOutcome::Failed
                            };
                            if let Err(completion) =
                                runtime.adapter_complete_exchange(&instance, id, false, Some(outcome))
                            {
                                return Err(Box::new(ChildFailure::FetchCompletion(
                                    Box::new(ChildFailure::classify(error)),
                                    Box::new(ChildFailure::classify(Box::new(completion))),
                                )));
                            }
                        }
                        return Err(error);
                    }
                    Ok(serde_json::Value::Null)
                }
                Input::DeliverCollection(records) => {
                    runtime.receive_collection_records(&runtime.instance()?, &records)?;
                    Ok(serde_json::Value::Null)
                }
                Input::DrainIncoming(steps) => {
                    let mut count = 0;
                    for _ in 0..steps {
                        let applied = runtime.drain_incoming(&collection, IPC_TIMEOUT)?;
                        count += applied;
                        if applied == 0 {
                            break;
                        }
                    }
                    Ok(serde_json::to_value(count)?)
                }
                Input::MediaWork => Ok(serde_json::to_value(runtime.cloud_media_work_with_space(
                    &collection,
                    &media,
                    &meguri_storage::space::StatvfsSpace,
                )?)?),
                Input::MediaReply(work, reply) => {
                    runtime.cloud_media_reply(&collection, &media, &work, reply)?;
                    Ok(serde_json::Value::Null)
                }
                Input::Reconcile => {
                    if unbound {
                        media.store().register_full(|_, _| true).map_err(|_| failure())?;
                    }
                    runtime.reconcile_binding(&collection, &media, IPC_TIMEOUT)?;
                    Ok(serde_json::Value::Null)
                }
                Input::Build(instance, names) => {
                    let batch = runtime.build_collection_batch(&instance, &collection, &names)?;
                    if !batch.saves.is_empty() {
                        let binding = runtime.status()?.binding.ok_or_else(failure)?;
                        let request = super::super::coordinator::HeldRequest {
                            caller: super::super::Caller {
                                instance: Some(instance.clone()),
                                ..Default::default()
                            },
                            operation: super::super::Operation::Modify {
                                zone: core::adapter::collection_zone(binding.facts.epoch),
                                atomic: false,
                                changes: batch
                                    .saves
                                    .iter()
                                    .map(|save| super::super::Mutation::Save {
                                        record: super::super::RecordInput::encrypted(
                                            &save.name,
                                            "MGEntity",
                                            save.envelope.clone(),
                                        ),
                                        if_unchanged: save
                                            .system_fields
                                            .clone()
                                            .map(super::super::SystemFields::from_bytes),
                                    })
                                    .collect(),
                            },
                        };
                        // Projection of the original built request; building it
                        // persisted the actual flights. No Service call here.
                        *submission.lock().unwrap_or_else(PoisonError::into_inner) = Some(request);
                        runtime.incoming_stage_for_testing(IncomingStage::BeforeCollectionSave);
                        runtime.validate_caller(&instance)?;
                        *submission.lock().unwrap_or_else(PoisonError::into_inner) = None;
                    }
                    Ok(serde_json::to_value(batch)?)
                }
                _ => Err(failure()),
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run))
            .unwrap_or_else(|_| Err(failure()))
            .map_err(ChildFailure::classify);
        let _ = events.send(WorkEvent::Completed(result));
    });
    let previous = device.incoming_worker.take().map(Box::new);
    device.incoming_worker = Some(Work {
        events: receiver,
        thread,
        notified_pause: None,
        previous,
    });
    advance(device, stream)?;
    Ok(None)
}

pub(super) fn observe(device: &mut Device, binding: core::BindingId) -> Result<IncomingObservation> {
    use meguri_engine::changes::stream::EventKind;
    while let Some(event) = device.incoming_events.try_recv() {
        match &event.kind {
            EventKind::Changes(_) => device.change_events.push(event.seq.0),
            EventKind::RolledOver { .. } => device.rollovers.push(RolloverObservation {
                seq: event.cause_seq.unwrap_or(event.seq).0,
            }),
            _ => {}
        }
    }
    let collection = device.host.collection()?;
    let (seq, generation, native_file, col_mod) = {
        let lease = collection.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        let file = runtime::FileIdentityProvider::identity(&devices::NativeFiles, collection.path())?;
        if snapshot.wal_identity().native_file() != (file.device, file.inode) {
            return Err(failure());
        }
        (
            snapshot.seq().0,
            snapshot.generation().0,
            file,
            snapshot.col()?.modified.0,
        )
    };
    let runtime = device.runtime()?;
    let serialization = runtime.incoming_engine_state_for_testing(binding)?;
    let engine_state = serialization
        .as_deref()
        .map(super::super::adapter::collection_cursor_projection)
        .transpose()?
        .flatten();
    let runs = runtime.incoming_runs();
    let wal_receipts = runtime
        .incoming_wal_receipts()
        .iter()
        .map(|receipt| {
            let identity = receipt.identity();
            let file = runs
                .iter()
                .find(|run| {
                    run.generation == identity.generation().0
                        && (run.native_file.device, run.native_file.inode) == identity.native_file()
                })
                .map(|run| run.native_file)
                .or_else(|| {
                    (generation == identity.generation().0
                        && (native_file.device, native_file.inode) == identity.native_file())
                    .then_some(native_file)
                })
                .ok_or_else(failure)?;
            Ok(Receipt {
                through: receipt.through().0,
                generation: identity.generation().0,
                native_file: file,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(IncomingObservation {
        seq,
        generation,
        native_file,
        col_mod,
        undo_depth: collection.undo_depth_for_testing()?,
        change_events: device.change_events.clone(),
        engine_instance: runtime.incoming_sdk_instance_for_testing(),
        engine_state,
        last_fetch_engine_state: device
            .last_fetch_engine_state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone(),
        runs,
        wal_receipts,
        rollovers: device.rollovers.clone(),
    })
}
