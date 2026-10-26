//! Real actor transport: login, the production normal job, status memory, and
//! existing manual one-way bootstrap. This module never implements a round.
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{Credentials, ReviewSession, Step, SyncKind, SyncSpec};
use meguri_sync::{Endpoint, ErrorKind, HostKey, JobError, JobOptions, ServerKind, StatusMemory};
use serde_json::json;
use std::sync::{Arc, Mutex};
#[derive(Default)]
pub(super) struct State {
    key: Option<HostKey>,
    memory: Arc<Mutex<StatusMemory>>,
    generation: Option<meguri_engine::Generation>,
}
impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").finish_non_exhaustive()
    }
}
fn error(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("sync_error", e.to_string())
}
pub(super) fn register(registry: &mut Registry) {
    registry.actor(super::storage::actor_dump);
    registry.op_rejecting("sync", &SyncSpec::REJECTIONS, sync);
    registry.op("review_session", review_session);
}
fn review_session(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::ReviewSession(action) = step else {
        return Err(error("expected review_session"));
    };
    session.engine_collection()?;
    match action {
        ReviewSession::Begin => {
            if session.review_session.is_some() {
                return Err(error("review session already active"));
            }
            session.review_session = Some(
                session
                    .storage
                    .host()?
                    .review()
                    .begin_unqueued_session()
                    .map_err(error)?,
            );
        }
        ReviewSession::End => session
            .review_session
            .take()
            .ok_or_else(|| error("review session absent"))?
            .end()
            .map_err(error)?,
    }
    Ok(OpResult::Done)
}
fn sync(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Sync(spec) = step else {
        return Err(error("expected sync"));
    };
    let seat = session
        .actor
        .clone()
        .ok_or_else(|| error("sync requires an actor seat"))?;
    let endpoint = seat
        .endpoint
        .as_deref()
        .ok_or_else(|| error("Meguri actor edge is unavailable"))?;
    super::sync_guard::check_endpoint(endpoint).map_err(error)?;
    let endpoint = Endpoint::parse(endpoint).map_err(|rule| error(rule.as_str()))?;
    let options = || JobOptions::new(endpoint.clone(), ServerKind::SelfHosted);
    let collection = session.engine_collection()?;
    if session.sync.generation != Some(collection.generation()) {
        session.sync.memory.lock().unwrap().forget();
        session.sync.generation = Some(collection.generation());
    }
    let host = session.storage.host()?;
    let result: Result<&'static str, JobError> = match spec.kind {
        SyncKind::Login => {
            let password = if spec.credentials == Credentials::WrongPassword {
                format!("{}-wrong", seat.account.password)
            } else {
                seat.account.password.clone()
            };
            meguri_sync::login_blocking(
                host.engine().threads(),
                &options(),
                session.random.as_ref(),
                &seat.account.username,
                &password,
            )
            .map(|key| {
                session.sync.key = Some(key);
                "ok"
            })
            .map_err(|e| JobError::from_transport(e, Some(meguri_sync::Method::HostKey), ServerKind::SelfHosted))
        }
        SyncKind::Normal => {
            let key = session.sync.key.clone().ok_or_else(|| error("actor has no host key"))?;
            let task = meguri_sync::normal::start(
                host.engine(),
                collection,
                options(),
                key,
                session.clock.clone(),
                session.random.clone(),
                session.sync.memory.clone(),
                None,
            )
            .map_err(error)?;
            task.wait().map(|outcome| {
                if outcome.merge.as_ref().is_some_and(|m| m.one_way) {
                    return "full_sync";
                }
                match outcome.check.result {
                    meguri_sync::CheckResult::NoChanges | meguri_sync::CheckResult::Normal { .. } => "no_changes",
                    meguri_sync::CheckResult::OneWay {
                        upload: true,
                        download: true,
                    } => "full_sync",
                    meguri_sync::CheckResult::OneWay {
                        upload: true,
                        download: false,
                    } => "full_upload",
                    meguri_sync::CheckResult::OneWay { .. } => "full_download",
                }
            })
        }
        SyncKind::Status => {
            let key = session.sync.key.clone().ok_or_else(|| error("actor has no host key"))?;
            let mut job = meguri_sync::Job::new(&options(), Some(key), session.random.as_ref()).map_err(error)?;
            let reader = collection.reader(meguri_storage::ReaderRole::Sync).map_err(error)?;
            let local = meguri_sync::LocalRecord::read(&*reader).map_err(error)?;
            // Actor steps are serial; Task::wait also waits for JobFinished,
            // after the normal job has released its captured memory reference.
            let memory = Arc::get_mut(&mut session.sync.memory)
                .ok_or_else(|| error("actor status memory ownership invariant: unexpected shared ownership"))?
                .get_mut()
                .unwrap();
            meguri_sync::transport::runtime::block_on(host.engine().threads(), options().cancel_token(), async {
                Ok(memory.status(&mut job, &local, session.clock.as_ref()).await)
            })
            .map_err(error)?
            .map(|o| match o.status {
                meguri_sync::SyncStatus::NoChanges => "no_changes",
                meguri_sync::SyncStatus::NormalSync => "normal_sync",
                meguri_sync::SyncStatus::FullSync => "full_sync",
            })
        }
        SyncKind::FullDownload | SyncKind::FullUpload => {
            if session.review_session.is_some() {
                return Err(error("one-way operation during actor review"));
            }
            let key = session.sync.key.clone().ok_or_else(|| error("actor has no host key"))?;
            let access = || {
                meguri_sync::oneway::TransferAccess::new(
                    endpoint.clone(),
                    &seat.account.username,
                    key.clone(),
                    options(),
                )
            };
            let direction = if spec.kind == SyncKind::FullDownload {
                meguri_sync::policy::Direction::Import
            } else {
                meguri_sync::policy::Direction::Export
            };
            let events = host.engine().stream().subscribe();
            let prep = host
                .prepare_manual_transfer(
                    access().map_err(error)?,
                    direction,
                    meguri_sync::policy::Scope::CollectionOnly,
                )
                .map_err(error)?;
            let prep = complete(events, prep).map_err(error)?;
            let events = host.engine().stream().subscribe();
            let task = host
                .confirm_manual_transfer(access().map_err(error)?, prep.confirmation)
                .map_err(error)?;
            complete(events, task).map_err(error)?;
            let generation = host.collection().map_err(error)?.generation();
            session.sync.generation = Some(generation);
            session.sync.memory.lock().unwrap().remember(
                &endpoint,
                Some(&key),
                meguri_sync::SyncStatus::NoChanges,
                session.clock.as_ref(),
            );
            Ok("ok")
        }
    };
    match result {
        Ok(result) => Ok(OpResult::Output(json!({"result":result}))),
        Err(e) => {
            let kind = match e.kind {
                ErrorKind::AuthFailed => {
                    session.sync.key = None;
                    "auth_failed"
                }
                ErrorKind::ClockIncorrect { .. } => "clock_incorrect",
                ErrorKind::Network { head_received: false } | ErrorKind::Offline | ErrorKind::Timeout => "network",
                ErrorKind::SanityCheckFailed { .. } => "sanity_check_failed",
                ErrorKind::DbCheckRequired { .. } => "db_check_required",
                _ => return Err(error(e.kind.id())),
            };
            Ok(OpResult::Rejected(kind))
        }
    }
}
fn complete<T>(
    events: meguri_engine::Subscription,
    task: meguri_host::ManualTransferTask<T>,
) -> Result<T, meguri_sync::oneway::TransferError> {
    while let Some(event) = events.recv() {
        if matches!(event.kind,meguri_engine::EventKind::JobFinished{id,..} if id==task.job.id) {
            break;
        }
    }
    task.take_result()
        .ok_or(meguri_sync::oneway::TransferError::Interrupted)?
}
