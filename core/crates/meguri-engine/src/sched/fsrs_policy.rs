//! Explicit FSRS enable and device-local seen-on state (sched/fsrs-policy).
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError},
    time::Duration,
};

use super::memory::{self, RecomputeReport, Target};
use crate::{
    Collection, EngineError, OpResult, OpSpec,
    executor::{
        catalog,
        plan_commit::{Plan, plan_and_commit_with},
        preconditions::Precondition,
    },
    tags::RunOptions,
};
use meguri_model::{
    changes::OpSource,
    ids::{DeckConfigId, DeckId},
    proto::deck::DeckKind,
};
pub use meguri_sched::compat::fsrs_policy::PolicyStatus;
use meguri_sched::{
    compat::{fsrs_policy::status, params_support, sched_version},
    model::{preset::PresetView, sched_config::SchedConfig},
    states::fsrs_glue::FsrsCache,
};
use meguri_storage::{
    ConfigValue, ReadRows, ReaderRole, Table,
    atomic_file::{AtomicFile, Purpose},
};

#[derive(Default)]
struct Mark {
    seen: bool,
    durable: bool,
}

/// Collection-place state, shared across generations. U43's admission path
/// must reserve/release review_active on the writer, after policy validation.
pub(crate) struct PolicyState {
    path: PathBuf,
    mark: Mutex<Mark>,
    pub(crate) review_active: Mutex<bool>,
}
impl PolicyState {
    fn mark_path(collection: &Path) -> PathBuf {
        let mut path = collection.as_os_str().to_owned();
        path.push(".meguri-fsrs-seen");
        PathBuf::from(path)
    }
    pub(crate) fn load(collection: &Path) -> Self {
        let path = Self::mark_path(collection);
        let seen = match std::fs::read(&path) {
            Ok(bytes) if bytes == b"1\n" => true,
            Ok(_) => {
                tracing::warn!("invalid FSRS seen-on mark; treating it as lost");
                false
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                tracing::warn!(%error, "could not read FSRS seen-on mark");
                false
            }
        };
        Self {
            path,
            mark: Mutex::new(Mark { seen, durable: seen }),
            review_active: Mutex::new(false),
        }
    }
    pub(crate) fn seen(&self) -> bool {
        self.mark.lock().unwrap_or_else(PoisonError::into_inner).seen
    }
    pub(crate) fn observe_on(&self) {
        let mut mark = self.mark.lock().unwrap_or_else(PoisonError::into_inner);
        mark.seen = true;
        if mark.durable {
            return;
        }
        let write = || -> Result<(), meguri_storage::StorageError> {
            let mut file = AtomicFile::new(&self.path, Purpose::Sidecar)?;
            file.file_mut().write_all(b"1\n")?;
            file.publish(|_| Ok(()))
        };
        match write() {
            Ok(()) => mark.durable = true,
            Err(error) => tracing::warn!(%error, "could not persist FSRS seen-on mark; will retry"),
        }
    }
    fn require_no_session(&self) -> Result<(), EngineError> {
        if *self.review_active.lock().unwrap_or_else(PoisonError::into_inner) {
            Err(EngineError::SessionActive)
        } else {
            Ok(())
        }
    }
}

pub fn policy_status(collection: &Collection) -> Result<PolicyStatus, EngineError> {
    let rows = collection.reader(ReaderRole::Interactive)?;
    Ok(status(
        SchedConfig::new(&*rows).fsrs()?,
        collection.core.exec.fsrs_policy.seen(),
    ))
}

/// Every real engine open, before Ready; replacements intentionally do not
/// normalize sched2021 until the next open (V2).
pub(crate) fn normalize_at_open(collection: &Collection) -> Result<(), EngineError> {
    let policy = collection.core.exec.fsrs_policy.clone();
    collection
        .run(
            OpSpec::named("enable_current_scheduler", OpSource::Rollover),
            move |ctx| {
                if sched_version::needs_current_scheduler(ctx.scope())? {
                    ctx.scope().set_config("sched2021", &ConfigValue::Bool(true))?;
                }
                if SchedConfig::new(ctx.scope()).fsrs()? {
                    ctx.on_commit(move |_| policy.observe_on());
                }
                Ok(())
            },
        )
        .map(drop)
}

/// All memory writes and the global switch share one transaction. Snapshot
/// guards include config, so an intervening sync forces a fresh plan.
pub fn enable_fsrs(collection: &Collection, options: &RunOptions) -> Result<OpResult<RecomputeReport>, EngineError> {
    let mut spec = OpSpec::new(catalog::ENABLE_FSRS, OpSource::UserNonUndoable);
    if let Some(cancel) = &options.cancel {
        spec = spec.cancel(cancel.clone());
    }
    if let Some(progress) = &options.progress {
        spec = spec.progress(progress.clone());
    }
    let enabled = {
        let rows = collection.reader(ReaderRole::Interactive)?;
        SchedConfig::new(&*rows).fsrs()?
    };
    // The normal already-on path performs no memory/day computation. If a
    // sync changes it before the writer runs, use the guarded planner below.
    if enabled {
        let policy = collection.core.exec.fsrs_policy.clone();
        let attempt = collection.run(spec.clone(), move |ctx| {
            policy.require_no_session()?;
            if !SchedConfig::new(ctx.scope()).fsrs()? {
                return Err(EngineError::Stale {
                    table: Table::Config,
                    key: "fsrs".into(),
                });
            }
            ctx.on_commit(move |_| policy.observe_on());
            Ok(RecomputeReport::default())
        });
        if !matches!(&attempt, Err(EngineError::Stale { .. })) {
            return attempt;
        }
    }
    let next_day_at = collection.next_day_at()?;
    let policy = collection.core.exec.fsrs_policy.clone();
    plan_and_commit_with(
        collection,
        ReaderRole::Maintenance,
        spec,
        |context| {
            let rows = &**context.snapshot();
            let value = if SchedConfig::new(rows).fsrs()? {
                None
            } else {
                Some(memory::plan(
                    rows,
                    &Target::All,
                    next_day_at,
                    &|| context.check_cancel(),
                    context.progress(),
                )?)
            };
            let preconditions = [
                Table::Config,
                Table::Cards,
                Table::Revlog,
                Table::Decks,
                Table::DeckConfig,
            ]
            .map(|table| Precondition::TableUntouched {
                table,
                since: context.seq(),
            })
            .to_vec();
            Ok(Plan { value, preconditions })
        },
        |spec, _| spec.clone().declare_long_write(Duration::from_secs(1)),
        move |ctx, planned| {
            policy.require_no_session()?;
            ctx.check_cancel()?;
            let report = if let Some(planned) = planned {
                ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
                memory::apply_in(ctx, planned)?
            } else {
                RecomputeReport::default()
            };
            let policy = policy.clone();
            ctx.on_commit(move |_| policy.observe_on());
            Ok(report)
        },
    )
}

/// U43 passes the chosen deck and descendants; this checks the cards' home
/// presets, including filtered cards. Browsing/counting never calls it.
pub fn unsupported_home_presets(rows: &impl ReadRows, decks: &[DeckId]) -> Result<Vec<DeckConfigId>, EngineError> {
    let mut presets = std::collections::BTreeSet::new();
    let mut cache = FsrsCache::default();
    let deck_rows = rows.deck_rows()?;
    let homes = rows.card_home_decks_in(decks)?;
    let mut checked = std::collections::BTreeSet::new();
    for home in homes {
        let Some(deck) = deck_rows.iter().find(|deck| deck.id == home) else {
            continue;
        };
        let DeckKind::Normal(normal) = &deck.kind.kind else {
            continue;
        };
        let id = DeckConfigId(normal.config_id);
        if !checked.insert(id) {
            continue;
        }
        let preset = PresetView::for_config(rows, id)?;
        if !params_support::supported(&mut cache, &preset) {
            presets.insert(id);
        }
    }
    Ok(presets.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::{
        clock::{StepClock, UtcOffset},
        rng::SeededRandom,
    };
    use std::sync::Arc;

    #[test]
    fn writer_owned_active_admission_refuses_enable_without_losing_undo() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("collection.anki2");
        let clock = Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC"));
        crate::bootstrap::create(&path, clock.as_ref(), &SeededRandom::new(7)).unwrap();
        let engine = crate::Engine::new(crate::EngineConfig::default()).unwrap();
        let c = engine
            .open_raw(&path, crate::OpenConfig::new(clock, Arc::new(SeededRandom::new(7))))
            .unwrap();
        c.run(OpSpec::named("user_setting", OpSource::UserUndoable), |ctx| {
            ctx.scope().set_config("userSetting", &ConfigValue::Bool(true))?;
            Ok(())
        })
        .unwrap();
        let before = c.peek_undo(crate::UndoScope::Global).unwrap();
        let policy = c.core.exec.fsrs_policy.clone();
        // U43 owns real session acquisition/release. This tests the U50
        // writer guard directly, without claiming an answer-session path.
        c.run(OpSpec::named("test_admission", OpSource::Rollover), move |_| {
            *policy.review_active.lock().unwrap() = true;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            enable_fsrs(&c, &Default::default()).unwrap_err(),
            EngineError::SessionActive
        );
        assert_eq!(policy_status(&c).unwrap(), PolicyStatus::NeedsEnable);
        assert_eq!(c.peek_undo(crate::UndoScope::Global).unwrap(), before);
        assert!(
            c.reader(ReaderRole::Query)
                .unwrap()
                .config_entry("fsrs")
                .unwrap()
                .is_none()
        );
        engine.close().unwrap();
    }
}
