//! Deck and preset adapters (conformance/deck-ops K1-K15).
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{CollapseScope, ObservationRequest, Setup, Step};
use meguri_engine::{EngineError, deck_config, decks};
use meguri_model::ids::{DeckConfigId, DeckId};
use meguri_storage::ReadRows;
use serde_json::{Value, json};

pub(super) fn register(r: &mut Registry) {
    for (op, rejects) in [
        ("add_deck", &["filtered_parent", "too_deep"][..]),
        ("rename_deck", &["filtered_parent", "too_deep", "under_descendant"][..]),
        ("reparent_decks", &["filtered_parent"][..]),
        ("delete_decks", &[][..]),
        ("set_deck_collapsed", &[][..]),
        ("set_current_deck", &[][..]),
        ("add_preset", &[][..]),
        ("clone_preset", &[][..]),
        ("rename_preset", &[][..]),
        ("remove_preset", &["default_preset"][..]),
        ("set_deck_preset", &["filtered_deck"][..]),
    ] {
        r.op_rejecting(op, rejects, run);
    }
    r.setup("decks", setup_decks);
    r.setup("presets", setup_presets);
    r.observation("decks", observe_decks);
    r.observation("deck_tree", observe_tree);
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}
fn unknown(s: impl Into<String>) -> ExecError {
    ExecError::new("unknown_ref", s)
}
fn writer<'a>(s: &'a Session<'_>) -> Result<super::storage::SessionReader<'a>, ExecError> {
    s.storage
        .collection()
        .ok_or_else(|| core("no collection open"))?
        .reader()
}

fn deck(rows: &impl ReadRows, name: &str) -> Result<DeckId, ExecError> {
    rows.deck_rows()
        .map_err(core)?
        .iter()
        .find(|d| d.human_name() == name)
        .map(|d| d.id)
        .ok_or_else(|| unknown(format!("no deck {name:?}")))
}
fn preset(rows: &impl ReadRows, name: &str) -> Result<DeckConfigId, ExecError> {
    let found = rows
        .deck_config_rows()
        .map_err(core)?
        .into_iter()
        .filter(|p| p.name == name)
        .collect::<Vec<_>>();
    match found.as_slice() {
        [one] => Ok(one.id),
        [] => Err(unknown(format!("no preset {name:?}"))),
        _ => Err(ExecError::new("invalid_scenario", format!("ambiguous preset {name:?}"))),
    }
}
fn deck_id(s: &Session<'_>, name: &str) -> Result<DeckId, ExecError> {
    deck(&*writer(s)?, name)
}
fn preset_id(s: &Session<'_>, name: &str) -> Result<DeckConfigId, ExecError> {
    preset(&*writer(s)?, name)
}

fn ids(rows: &impl ReadRows, names: &[String]) -> Result<Vec<DeckId>, ExecError> {
    names.iter().map(|n| deck(rows, n)).collect()
}
fn run(s: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let c = s.engine_collection()?;
    let result = match step {
        Step::AddDeck(a) => {
            decks::add(&c, a.name.clone()).map(|d| OpResult::Output(json!({"name": d.value.human_name()})))
        }
        Step::RenameDeck(a) => decks::rename(&c, deck_id(s, &a.deck)?, a.name.clone())
            .map(|d| OpResult::Output(json!({"name": d.value.human_name()}))),
        Step::ReparentDecks(a) => {
            let selected = ids(&*writer(s)?, &a.decks)?;
            let parent = if a.parent.is_empty() {
                None
            } else {
                Some(deck_id(s, &a.parent)?)
            };
            decks::move_decks(&c, selected, parent).map(|r| OpResult::Output(json!({"count": r.value})))
        }
        Step::DeleteDecks(a) => {
            let selected = ids(&*writer(s)?, &a.decks)?;
            // K5: the counts read at the step, right before submitting. The
            // reader is released first: the delete plans and commits on its own.
            let confirmed = {
                let rows = writer(s)?;
                decks::deletion_summary(&*rows, &selected)
            };
            let backup = s.storage.host()?.backup();
            confirmed
                .and_then(|confirmed| decks::delete(&c, selected, decks::ConfirmedLosses::Counts(confirmed), backup))
                .map(|r| OpResult::Output(json!({"count": r.value.cards})))
        }
        Step::SetDeckCollapsed(a) => {
            let selected = ids(&*writer(s)?, &a.decks)?;
            let which = match a.scope {
                CollapseScope::Study => decks::CollapseScope::Study,
                CollapseScope::Browser => decks::CollapseScope::Browser,
            };
            decks::collapse(&c, selected, which, a.collapsed).map(|_| OpResult::Done)
        }
        Step::SetCurrentDeck(a) => decks::select(&c, deck_id(s, &a.deck)?).map(|_| OpResult::Done),
        Step::AddPreset(a) => deck_config::add(&c, a.name.clone()).map(|_| OpResult::Done),
        Step::ClonePreset(a) => {
            deck_config::clone(&c, preset_id(s, &a.preset)?, a.name.clone()).map(|_| OpResult::Done)
        }
        Step::RenamePreset(a) => {
            deck_config::rename(&c, preset_id(s, &a.preset)?, a.name.clone()).map(|_| OpResult::Done)
        }
        Step::RemovePreset(a) => {
            deck_config::remove(&c, preset_id(s, &a.preset)?, preset_id(s, &a.fallback)?).map(|_| OpResult::Done)
        }
        Step::SetDeckPreset(a) => {
            deck_config::assign(&c, deck_id(s, &a.deck)?, preset_id(s, &a.preset)?, a.subtree).map(|_| OpResult::Done)
        }
        _ => return Err(core("deck adapter called for another op")),
    };
    match result {
        Ok(value) => Ok(value),
        Err(EngineError::DeckRefused(refusal)) => Ok(OpResult::Rejected(refusal.kind())),
        Err(error) => Err(core(error)),
    }
}

fn setup_decks(s: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    for entry in setup.decks.iter().flatten() {
        let c = s.engine_collection()?;
        let preset = entry.preset().map(|name| preset_id(s, name)).transpose()?;
        let name = entry.name().to_owned();
        let deck = c
            .run(
                meguri_engine::OpSpec::named("scenario_setup", meguri_engine::OpSource::UserNonUndoable).skip_undo(),
                move |ctx| {
                    let scope = ctx.scope();
                    let deck = decks::add_in(scope, &name)?;
                    if let Some(preset) = preset {
                        deck_config::assign_in(scope, deck.id, preset, false)?;
                    }
                    Ok(deck)
                },
            )
            .map_err(core)?
            .value;
        s.decks.insert(entry.name().to_owned(), deck.id.0);
    }
    Ok(())
}

fn setup_presets(s: &mut Session<'_>, setup: &Setup) -> Result<(), ExecError> {
    for entry in setup.presets.iter().flatten() {
        let c = s.engine_collection()?;
        let entry = entry.clone();
        c.run(
            meguri_engine::OpSpec::named("scenario_setup", meguri_engine::OpSource::UserNonUndoable).skip_undo(),
            move |ctx| {
                let scope = ctx.scope();
                let id = if entry.name == "Default" {
                    DeckConfigId(1)
                } else {
                    // Setup starts from built-in defaults, without P4's FSRS-6
                    // list; only the add_preset operation uses the menu defaults.
                    let mut p = deck_config::defaults();
                    p.name = entry.name.clone();
                    scope.add_deck_config(&p)?
                };
                let mut p = scope.deck_config(id)?.ok_or_else(|| EngineError::Internal {
                    message: "preset disappeared".into(),
                })?;
                macro_rules! field {
                    ($from:ident, $to:ident, $kind:ty) => {
                        if let Some(v) = entry.$from {
                            p.config.$to = v as $kind;
                        }
                    };
                }
                field!(graduating_interval_days, graduating_interval_good, u32);
                field!(easy_interval_days, graduating_interval_easy, u32);
                field!(new_per_day, new_per_day, u32);
                field!(reviews_per_day, reviews_per_day, u32);
                field!(max_interval_days, maximum_review_interval, u32);
                field!(min_lapse_interval_days, minimum_lapse_interval, u32);
                field!(starting_ease, initial_ease, f32);
                field!(easy_bonus, easy_multiplier, f32);
                field!(hard_interval, hard_multiplier, f32);
                field!(interval_modifier, interval_multiplier, f32);
                field!(lapse_new_interval, lapse_multiplier, f32);
                field!(leech_threshold, leech_threshold, u32);
                field!(desired_retention, desired_retention, f32);
                if let Some(steps) = &entry.learn_steps {
                    p.config.learn_steps = steps.iter().map(|d| d.seconds() as f32 / 60.0).collect();
                }
                if let Some(steps) = &entry.relearn_steps {
                    p.config.relearn_steps = steps.iter().map(|d| d.seconds() as f32 / 60.0).collect();
                }
                if let Some(v) = entry.leech_action {
                    p.config.leech_action = match v {
                        crate::scenario::LeechAction::Suspend => 0,
                        crate::scenario::LeechAction::TagOnly => 1,
                    };
                }
                if let Some(v) = entry.bury_new {
                    p.config.bury_new = v;
                }
                if let Some(v) = entry.bury_reviews {
                    p.config.bury_reviews = v;
                }
                if let Some(v) = entry.bury_interday_learning {
                    p.config.bury_interday_learning = v;
                }
                if let Some(v) = &entry.fsrs_params {
                    p.config.set_fsrs_params_6(v.iter().map(|v| *v as f32).collect());
                }
                scope.put(&p)?;
                Ok(())
            },
        )
        .map_err(core)?;
    }
    Ok(())
}
fn observe_decks(s: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    let mut out = serde_json::Map::new();
    for d in writer(s)?.deck_rows().map_err(core)? {
        let config = match &d.kind.kind {
            meguri_model::proto::deck::DeckKind::Normal(n) => Some(n.config_id),
            _ => None,
        };
        out.insert(d.human_name(), json!({"id": d.id.0, "filtered": decks::is_filtered(&d), "config_id": config,
            "study_collapsed": d.common.study_collapsed, "browser_collapsed": d.common.browser_collapsed, "mtime_secs": d.mtime.0, "usn": d.usn.0}));
    }
    Ok(out.into())
}
fn observe_tree(s: &mut Session<'_>, _: &ObservationRequest) -> Result<Value, ExecError> {
    fn node(n: decks::tree::Node) -> Value {
        json!({"name": n.name, "children": n.children.into_iter().map(node).collect::<Vec<_>>()})
    }
    Ok(decks::tree::tree(&*writer(s)?)
        .map_err(core)?
        .into_iter()
        .map(node)
        .collect())
}
