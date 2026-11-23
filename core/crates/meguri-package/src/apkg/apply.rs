//! The apply of an import session (`package/apkg-import` I14): admission
//! and the long-operation slot, the plan on the maintenance snapshot, the
//! merge confirmation, media staging, the safety backup when the plan needs
//! one, and the one write op "Import".
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meguri_engine::executor::catalog;
use meguri_engine::media::MediaService;
use meguri_engine::undo::entry::StepKind;
use meguri_engine::{
    CancelToken, Collection, Engine, EngineError, LongOp, OpSource, OpSpec, Plan, Precondition, ProgressPhase,
    ProgressSink, plan_stage_and_commit,
};
use meguri_storage::{ReadRows, ReaderRole, StampMode, Table};

use super::commit::{Committed, Fault, commit};
use super::gather;
use super::plan::{self, ImportPlan};
use super::session::ImportSession;
use super::{
    ApkgImportError, ConfirmationToken, ImportOptions, ImportOutcome, ImportResult, UpdateCondition,
};

/// The committed write phase's value and its safety backup's name, kept
/// outside the op for results the engine reports as errors after a commit.
type Kept = Arc<Mutex<Option<(Committed, Option<String>)>>>;

/// I14.2: a write phase above this many rows declares its length.
const LONG_WRITE_ROWS: usize = 5000;

/// I5: the five option values as the open collection stores them, missing
/// or unreadable keys reading as their defaults. A pure read.
pub fn read_options(collection: &Collection) -> Result<ImportOptions, EngineError> {
    let lease = collection.reader(ReaderRole::Query)?;
    let rows = lease.snapshot()?;
    let flag = |key: &str| -> Result<bool, EngineError> {
        Ok(matches!(rows.config_value(key)?, Some(meguri_storage::ConfigValue::Bool(true))))
    };
    let rule = |key: &str| -> Result<UpdateCondition, EngineError> {
        Ok(UpdateCondition::from_stored(
            rows.config_value(key)?.as_ref().and_then(|value| value.as_str()),
        ))
    };
    Ok(ImportOptions {
        merge_notetypes: flag("mergeNotetypes")?,
        update_notes: rule("updateNotes")?,
        update_notetypes: rule("updateNotetypes")?,
        with_scheduling: flag("withScheduling")?,
        with_deck_configs: flag("withDeckConfigs")?,
    })
}

fn stash(fault: &Fault, error: ApkgImportError) -> EngineError {
    match error {
        ApkgImportError::Cancelled => EngineError::Cancelled,
        ApkgImportError::Engine(error) => *error,
        other => {
            *fault.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(other);
            EngineError::InvalidArgument {
                message: "the import was refused".into(),
            }
        }
    }
}

/// I14: imports the session's package into the open collection with
/// `options`, as one undoable step. `token` is the confirmation a previous
/// call's `ConfirmationRequired` returned (I8). Blocks until the import
/// ends; `cancel` stops it before its write phase.
///
/// A failure or cancel before the commit leaves the collection, its media
/// folder and its journal as they were and the session open for another
/// apply. After a commit the session never applies again.
pub fn apply(
    engine: &Engine,
    session: &ImportSession,
    options: ImportOptions,
    token: Option<&ConfirmationToken>,
    cancel: &Arc<CancelToken>,
    progress: &ProgressSink,
) -> ImportOutcome<ImportResult> {
    {
        let state = session.lock();
        if state.closed || state.result.is_some() {
            return Err(ApkgImportError::SessionClosed);
        }
    }
    let slot = engine.acquire_slot(LongOp::ApkgApply)?;
    // The handle of the collection the slot was taken on, never one opened
    // since; and the session's state again, now that no other apply runs.
    let collection = slot.collection()?;
    // I2: a session belongs to the engine whose collection directory holds
    // its staging; it never applies to another engine's collection.
    if collection.path().parent() != Some(session.context.collection_directory.as_path()) {
        return Err(ApkgImportError::SessionClosed);
    }
    {
        let state = session.lock();
        if state.closed || state.result.is_some() {
            return Err(ApkgImportError::SessionClosed);
        }
    }
    let context = session.context.clone();
    progress.set_phase(ProgressPhase::Planning);
    let check = || -> ImportOutcome<()> { cancel.check().map_err(Into::into) };
    let raw = Arc::new(gather::read(&session.collection_path(), &context, options.with_scheduling, &check)?);
    let taken = Arc::new(gather::take(&raw, &options));
    let service = MediaService::open(&collection, context.clock.clone())?;

    let fault: Fault = Arc::new(Mutex::new(None));
    let kept: Kept = Arc::new(Mutex::new(None));
    let spec = OpSpec::new(catalog::APKG_APPLY, OpSource::UserUndoable)
        .stamp(StampMode::KeepMtime)
        .undo_step(meguri_engine::undo::names::IMPORT, StepKind::Other)
        .cancel(Arc::clone(cancel))
        .progress(progress.clone());

    let outcome = plan_stage_and_commit(
        &collection,
        ReaderRole::Maintenance,
        spec,
        meguri_engine::executor::plan_commit::REPLANS,
        |planning| {
            // T by the pure day calculation on the snapshot: the write phase
            // makes the same computation on the live rows (I14.2).
            // One instant for both days of I11.2.
            let at = context.clock.now_ms();
            let today = super::commit::today(&**planning.snapshot(), context.clock.as_ref(), at)
                .map_err(|error| stash(&fault, error.into()))?;
            let now_s = at.div_euclid(1000);
            let built = plan::build(
                &**planning.snapshot(),
                &raw,
                &taken,
                &options,
                today,
                gather::package_today(&raw, &context, at),
                now_s,
                cancel,
                progress,
            )
            .map_err(|error| stash(&fault, error))?;
            // I8: a preview needs a confirmation that names this session,
            // these options and this preview.
            if !built.stage.preview.is_empty() {
                let digest = built.stage.preview.digest();
                let confirmed = token
                    .is_some_and(|token| token.session == session.id && token.options == options && token.digest == digest);
                if !confirmed {
                    return Err(stash(
                        &fault,
                        ApkgImportError::ConfirmationRequired {
                            preview: Box::new(built.stage.preview.clone()),
                            token: Box::new(ConfirmationToken {
                                session: session.id,
                                options,
                                digest,
                                snapshot_seq: planning.seq().0,
                            }),
                        },
                    ));
                }
            }
            let preconditions = [
                Table::Notes,
                Table::Notetypes,
                Table::Fields,
                Table::Templates,
                Table::DeckConfig,
                Table::Tags,
            ]
            .into_iter()
            .map(|table| Precondition::TableUntouched {
                table,
                since: planning.seq(),
            })
            .collect();
            Ok(Plan {
                value: built,
                preconditions,
            })
        },
        |mut built: ImportPlan, _| {
            super::media::stage(session, &service, &mut built, cancel, progress).map_err(|error| stash(&fault, error))?;
            if !built.stage.reasons.is_empty() || !built.stage.preview.is_empty() {
                progress.set_phase(ProgressPhase::Named("backup"));
            }
            Ok(built)
        },
        |spec, built| {
            let mut spec = spec.clone().schema_changes(built.stage.reasons.iter().copied());
            if !built.stage.preview.is_empty() {
                spec = spec.require_safety_backup();
            }
            if built.rows() > LONG_WRITE_ROWS {
                spec = spec.declare_long_write(Duration::from_secs(1));
            }
            spec
        },
        {
            let fault = Arc::clone(&fault);
            let kept = Arc::clone(&kept);
            let clock = context.clock.clone();
            move |ctx, built| {
                let backup = ctx.safety_backup_name().map(str::to_owned);
                let committed = commit(ctx, built, clock.as_ref(), &fault)?;
                *kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some((committed.clone(), backup));
                Ok(committed)
            }
        },
    );

    let take_kept = || kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    let finish = |seq: u64, generation: u64, undo: Option<&'static str>, pending: bool| -> Option<ImportResult> {
        let (committed, backup) = take_kept()?;
        let mut counts = committed.counts;
        if !pending {
            let (written, skipped) =
                super::media::write_statics(session, &service, &committed.statics, committed.decoded);
            counts.media_written += written;
            counts.media_skipped += skipped;
        } else {
            counts.media_skipped += committed.statics.len() as u64;
        }
        let result = ImportResult {
            seq,
            generation,
            undo_step: undo,
            log: committed.log,
            counts,
            safety_backup: backup,
            media_pending: pending,
        };
        session.lock().result = Some(result.clone());
        Some(result)
    };
    // The slot is held until the session has its result and the static
    // media are written: no second apply and no collection swap in between.
    let _held = slot;
    match outcome {
        Ok(op) => finish(op.seq.0, op.generation.0, op.undo.map(|undo| undo.name), false).ok_or_else(|| {
            ApkgImportError::Io {
                detail: "the import committed without a result".into(),
            }
        }),
        Err(EngineError::CommitCompleted { seq, message }) => {
            finish(seq.0, collection.generation().0, Some(meguri_engine::undo::names::IMPORT), false)
                .ok_or_else(|| ApkgImportError::Engine(Box::new(EngineError::CommitCompleted { seq, message })))
        }
        Err(EngineError::MediaCommitPending { seq, message }) => {
            match finish(seq.0, collection.generation().0, Some(meguri_engine::undo::names::IMPORT), true) {
                Some(result) => Err(ApkgImportError::MediaCommitPending(Box::new(result))),
                None => Err(ApkgImportError::Engine(Box::new(EngineError::MediaCommitPending { seq, message }))),
            }
        }
        Err(error) => {
            let refused = fault.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
            Err(refused.unwrap_or_else(|| error.into()))
        }
    }
}
