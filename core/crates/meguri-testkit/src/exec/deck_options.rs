//! E1/E4: actual U49 drafts and saves, including U46's hidden-option seam.
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{InsertionOrder, LeechAction, Step};
use meguri_engine::{EngineError, sched::deck_options as options};
use meguri_sched::options::{Field, Value};
use meguri_storage::ReadRows;

pub(super) fn register(registry: &mut Registry) {
    registry.op_rejecting("save_deck_options", &["default_preset"], save);
    registry.op("set_scheduling_prefs", preferences);
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}
fn invalid(e: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", e)
}
fn save(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::SaveDeckOptions(args) = step else {
        return Err(invalid("expected save_deck_options"));
    };
    let collection = session.engine_collection()?;
    let id = {
        let rows = collection.reader(meguri_storage::ReaderRole::Query).map_err(core)?;
        rows.deck_rows()
            .map_err(core)?
            .into_iter()
            .find(|d| d.human_name() == args.deck)
            .map(|d| d.id)
            .ok_or_else(|| invalid("unknown target deck"))?
    };
    let mut draft = options::Draft::new(options::snapshot(&collection, id).map_err(core)?);
    let find = |name: &str| -> Result<i64, ExecError> {
        let found = draft
            .presets
            .iter()
            .filter(|p| p.name == name)
            .map(|p| p.key)
            .collect::<Vec<_>>();
        if let [key] = found.as_slice() {
            Ok(*key)
        } else {
            Err(invalid("unknown or ambiguous preset"))
        }
    };
    let selected = args
        .preset
        .as_ref()
        .map(|p| find(p))
        .transpose()?
        .unwrap_or(draft.selected);
    let removals = args
        .remove
        .iter()
        .flatten()
        .map(|p| find(p))
        .collect::<Result<Vec<_>, _>>()?;
    if removals.contains(&selected) {
        return Err(invalid("selected preset cannot be removed"));
    }
    draft.select(selected).map_err(core)?;
    for key in removals {
        if key == 1 {
            return Ok(OpResult::Rejected("default_preset"));
        }
        draft.remove(key).map_err(core)?;
    }
    draft.select(selected).map_err(core)?;
    draft.subtree = args.subtree;
    if let Some(options) = &args.options {
        macro_rules! numeric {
            ($from:ident,$field:ident) => {
                if let Some(v) = options.$from {
                    draft
                        .edit(Some(selected), Field::$field, Value::Number(v as f64))
                        .map_err(core)?;
                }
            };
        }
        numeric!(new_per_day, NewPerDay);
        numeric!(reviews_per_day, ReviewsPerDay);
        numeric!(max_interval_days, MaximumReviewInterval);
        numeric!(min_lapse_interval_days, MinimumLapseInterval);
        numeric!(leech_threshold, LeechThreshold);
        numeric!(desired_retention, DesiredRetention);
        numeric!(historical_retention, HistoricalRetention);
        for (field, value) in [
            (Field::BuryNew, options.bury_new),
            (Field::BuryReviews, options.bury_reviews),
            (Field::BuryInterdayLearning, options.bury_interday_learning),
        ] {
            if let Some(value) = value {
                draft.edit(Some(selected), field, Value::Boolean(value)).map_err(core)?;
            }
        }
        for (field, steps) in [
            (Field::LearnSteps, &options.learn_steps),
            (Field::RelearnSteps, &options.relearn_steps),
        ] {
            if let Some(steps) = steps {
                draft
                    .edit(
                        Some(selected),
                        field,
                        Value::Steps(steps.iter().map(|d| d.seconds() as f32 / 60.0).collect()),
                    )
                    .map_err(core)?;
            }
        }
        if let Some(action) = options.leech_action {
            draft
                .edit(
                    Some(selected),
                    Field::LeechAction,
                    Value::Number(match action {
                        LeechAction::Suspend => 0.0,
                        LeechAction::TagOnly => 1.0,
                    }),
                )
                .map_err(core)?;
        }
        if let Some(order) = options.insertion_order {
            draft
                .edit(
                    Some(selected),
                    Field::NewCardInsertOrder,
                    Value::Number(match order {
                        InsertionOrder::Sequential => 0.0,
                        InsertionOrder::Random => 1.0,
                    }),
                )
                .map_err(core)?;
        }
        if let Some(date) = &options.ignore_reviews_before {
            draft
                .edit(
                    Some(selected),
                    Field::IgnoreRevlogsBeforeDate,
                    Value::Text(date.clone()),
                )
                .map_err(core)?;
        }
    }
    if let Some(value) = args.deck_desired_retention {
        draft
            .edit(
                None,
                Field::DeckDesiredRetention,
                value.map_or(Value::None, Value::Number),
            )
            .map_err(core)?;
    }
    if let Some(limits) = &args.limits {
        for (field, value) in [
            (Field::NewLimit, limits.new),
            (Field::ReviewLimit, limits.review),
            (Field::NewLimitToday, limits.new_today),
            (Field::ReviewLimitToday, limits.review_today),
        ] {
            if let Some(value) = value {
                draft
                    .edit(None, field, value.map_or(Value::None, |n| Value::Number(n as f64)))
                    .map_err(core)?;
            }
        }
    }
    if let Some(globals) = &args.globals {
        for (field, value) in [
            (Field::NewCardsIgnoreReviewLimit, globals.new_cards_ignore_review_limit),
            (Field::ApplyAllParentLimits, globals.apply_all_parent_limits),
        ] {
            if let Some(value) = value {
                draft.edit(None, field, Value::Boolean(value)).map_err(core)?;
            }
        }
    }
    let hidden = args.options.clone();
    options::save_with(&collection, draft, &Default::default(), move |scope| {
        if let Some(edit) = hidden {
            let mut row = scope
                .deck_config(meguri_model::ids::DeckConfigId(selected))?
                .ok_or_else(|| EngineError::InvalidArgument {
                    message: "missing preset".into(),
                })?;
            macro_rules! patch {
                ($from:ident,$to:ident,$ty:ty) => {
                    if let Some(v) = edit.$from {
                        row.config.$to = v as $ty;
                    }
                };
            }
            patch!(graduating_interval_days, graduating_interval_good, u32);
            patch!(easy_interval_days, graduating_interval_easy, u32);
            patch!(starting_ease, initial_ease, f32);
            patch!(easy_bonus, easy_multiplier, f32);
            patch!(hard_interval, hard_multiplier, f32);
            patch!(interval_modifier, interval_multiplier, f32);
            patch!(lapse_new_interval, lapse_multiplier, f32);
            if let Some(params) = edit.fsrs_params {
                row.config.set_fsrs_params_6(params.iter().map(|v| *v as f32).collect());
            }
            scope.put(&row)?;
        }
        Ok(())
    })
    .map_err(core)?;
    Ok(OpResult::Done)
}
fn preferences(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::SetSchedulingPrefs(a) = step else {
        return Err(invalid("expected set_scheduling_prefs"));
    };
    let collection = session.engine_collection()?;
    options::save_preferences(
        &collection,
        options::PreferencesChange {
            rollover: a.rollover.map(|v| v as f64),
            learn_ahead_minutes: a.learn_ahead_minutes.map(|v| v as f64),
            load_balancer: a.load_balancer,
        },
        &Default::default(),
    )
    .map_err(core)?;
    // The compatibility observation cache is separate from native U40.
    session.day.clear();
    Ok(OpResult::Done)
}
