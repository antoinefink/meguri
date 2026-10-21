//! U48: one selection, one journal and one Other step. All compatibility
//! validation precedes row application; no operation owns a study queue.
use crate::{Collection, EngineError, OpResult, SessionId, sched::answer, tags::RunOptions, undo::names};
use meguri_model::{
    card::CardQueue,
    config::ConfigEntry,
    ids::{CardId, DeckConfigId, DeckId, NoteId, RevlogId},
    proto::deck::DeckKind,
    revlog::{RevlogEntry, RevlogKind},
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_sched::{
    answer::Rating,
    load_balancer::{Entry, LoadBalancer},
    model::{card_sched::CardSched, preset::PresetView, sched_config::SchedConfig},
    ops as values,
    states::{fsrs_glue::FsrsCache, fuzz},
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, StampMode, Table, WriteScope};
use std::collections::{BTreeMap, BTreeSet};

pub use values::set_due::{Days, InvalidDays};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    InvalidDays(InvalidDays),
    HomeDeckFiltered { deck: DeckId },
    InFilteredDeck { card: CardId },
    NotFiltered { deck: DeckId },
    PresetUnsupported { preset_ids: Vec<DeckConfigId> },
}
#[derive(Clone, Debug)]
pub enum Targets {
    Cards(Vec<CardId>),
    Notes(Vec<NoteId>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnburyMode {
    All,
    Manual,
    Siblings,
}
impl UnburyMode {
    fn includes(self, queue: CardQueue) -> bool {
        matches!(
            (self, queue),
            (Self::All | Self::Manual, CardQueue::BuriedByUser)
                | (Self::All | Self::Siblings, CardQueue::BuriedByScheduler)
        )
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Reviewer,
    Browser,
}
#[derive(Clone, Debug)]
pub enum Request {
    Bury(Targets),
    Suspend(Targets),
    Unsuspend(Vec<CardId>),
    UnburyDeck {
        deck: DeckId,
        mode: UnburyMode,
    },
    SetDue {
        cards: Vec<CardId>,
        days: String,
        context: Option<Context>,
    },
    Reset {
        cards: Vec<CardId>,
        restore_position: bool,
        reset_counts: bool,
        context: Option<Context>,
    },
    GradeNow {
        cards: Vec<CardId>,
        rating: Rating,
    },
    EmptyFiltered {
        deck: DeckId,
    },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuriedKinds {
    pub manual: bool,
    pub siblings: bool,
}
#[derive(Clone, Debug)]
pub struct ResultValue {
    pub count: usize,
    pub head_diff: Option<crate::session::HeadDiff>,
}
fn missing(table: Table, id: i64) -> EngineError {
    EngineError::NotFound {
        table,
        key: id.to_string(),
    }
}
fn refused(r: Refusal) -> EngineError {
    EngineError::SchedOp(r)
}
fn distinct<T: Ord + Copy>(ids: &[T]) -> Result<(), EngineError> {
    let mut seen = BTreeSet::new();
    if ids.iter().any(|id| !seen.insert(*id)) {
        return Err(EngineError::InvalidArgument {
            message: "duplicate scheduling target".into(),
        });
    }
    Ok(())
}
impl Request {
    pub(crate) fn validate(&self) -> Result<(), EngineError> {
        match self {
            Self::Bury(Targets::Cards(c))
            | Self::Suspend(Targets::Cards(c))
            | Self::Unsuspend(c)
            | Self::Reset { cards: c, .. }
            | Self::GradeNow { cards: c, .. } => distinct(c)?,
            Self::Bury(Targets::Notes(n)) | Self::Suspend(Targets::Notes(n)) => distinct(n)?,
            Self::SetDue { cards, days, .. } => {
                distinct(cards)?;
                Days::parse(days).map_err(|e| refused(Refusal::InvalidDays(e)))?;
            }
            _ => (),
        }
        Ok(())
    }
    fn step(&self) -> &'static str {
        match self {
            Self::Bury(_) => names::BURY,
            Self::Suspend(_) => names::SUSPEND,
            Self::Unsuspend(_) | Self::UnburyDeck { .. } => names::UNBURY_UNSUSPEND,
            Self::SetDue { .. } => names::SET_DUE_DATE,
            Self::Reset { .. } => names::RESET_CARD,
            Self::GradeNow { .. } => names::GRADE_NOW,
            Self::EmptyFiltered { .. } => names::EMPTY,
        }
    }
    fn cards(&self, rows: &impl ReadRows) -> Result<Vec<CardId>, EngineError> {
        let mut ids = match self {
            Self::Bury(Targets::Cards(c))
            | Self::Suspend(Targets::Cards(c))
            | Self::Unsuspend(c)
            | Self::Reset { cards: c, .. }
            | Self::SetDue { cards: c, .. }
            | Self::GradeNow { cards: c, .. } => c.clone(),
            Self::Bury(Targets::Notes(notes)) | Self::Suspend(Targets::Notes(notes)) => {
                let mut cards = Vec::new();
                for note in notes {
                    cards.extend(rows.cards_of_note(*note)?.into_iter().map(|c| c.id));
                }
                cards
            }
            Self::UnburyDeck { deck, mode } => {
                let mut cards = Vec::new();
                for deck in subtree(rows, *deck)? {
                    cards.extend(
                        rows.cards_of_deck(deck)?
                            .into_iter()
                            .filter(|c| mode.includes(c.queue))
                            .map(|c| c.id),
                    );
                }
                cards
            }
            Self::EmptyFiltered { deck } => {
                let row = rows.deck(*deck)?.ok_or_else(|| missing(Table::Decks, deck.0))?;
                if !matches!(row.kind.kind, DeckKind::Filtered(_)) {
                    return Err(refused(Refusal::NotFiltered { deck: *deck }));
                }
                rows.cards_of_deck(*deck)?.into_iter().map(|c| c.id).collect()
            }
        };
        if !matches!(self, Self::Reset { .. } | Self::GradeNow { .. }) {
            ids.sort();
        }
        Ok(ids)
    }
}
fn subtree(rows: &impl ReadRows, deck: DeckId) -> Result<Vec<DeckId>, EngineError> {
    let Some(root) = rows.deck(deck)? else {
        return Ok(Vec::new());
    };
    let prefix = format!("{}\u{1f}", root.name);
    Ok(rows
        .deck_rows()?
        .into_iter()
        .filter(|d| d.id == deck || d.name.starts_with(&prefix))
        .map(|d| d.id)
        .collect())
}
pub fn buried_kinds(collection: &Collection, deck: DeckId) -> Result<BuriedKinds, EngineError> {
    collection.check_rollover()?;
    let lease = collection.reader(ReaderRole::Interactive)?;
    let rows = lease.snapshot()?;
    let mut kinds = BuriedKinds::default();
    for deck in subtree(&*rows, deck)? {
        for card in rows.cards_of_deck(deck)? {
            kinds.manual |= card.queue == CardQueue::BuriedByUser;
            kinds.siblings |= card.queue == CardQueue::BuriedByScheduler;
        }
    }
    Ok(kinds)
}
/// Raw engine owner. A hosted owner always goes through ReviewService::sched_op
/// so accepted answers are drained and admission remains exclusive through diff.
pub fn run(
    collection: &Collection,
    request: Request,
    options: &RunOptions,
) -> Result<OpResult<ResultValue>, EngineError> {
    request.validate()?;
    if collection.core.exec.active_session.lock().unwrap().is_some() {
        return Err(EngineError::SessionActive);
    }
    run_inner(collection, request, None, options, true)
}
pub(crate) fn run_admitted(
    collection: &Collection,
    request: Request,
    session: Option<SessionId>,
    options: &RunOptions,
) -> Result<OpResult<ResultValue>, EngineError> {
    run_inner(collection, request, session, options, false)
}
fn run_inner(
    collection: &Collection,
    request: Request,
    session: Option<SessionId>,
    options: &RunOptions,
    require_idle: bool,
) -> Result<OpResult<ResultValue>, EngineError> {
    request.validate()?;
    let day = collection.check_rollover()?;
    // Deck/note expansion is repeated in the transaction. This count only
    // chooses the existing bulk catalog declaration; it is never a write plan.
    let estimated = {
        let rows = collection.reader(ReaderRole::Query)?;
        request.cards(&*rows)?.len()
    };
    let mut spec = crate::tags::op_spec("sched_ops", request.step(), estimated.saturating_mul(5), options)
        .stamp(StampMode::CallerSupplied);
    if let Some(id) = session {
        spec = spec.session(id);
    }
    let bound = collection.clone();
    collection.run(spec, move |ctx| {
        request.validate()?;
        if require_idle && bound.core.exec.active_session.lock().unwrap().is_some() {
            return Err(EngineError::SessionActive);
        }
        let ids = request.cards(ctx.scope())?;
        let now = ctx.now();
        // Missing normal-operation targets are skipped. Grade Now must refuse
        // its complete selection, including every unsupported preset, first.
        let mut cards = Vec::with_capacity(ids.len());
        let mut unsupported = BTreeSet::new();
        let mut models = FsrsCache::default();
        let mut starting = BTreeMap::new();
        for id in ids {
            let Some(card) = ctx.scope().card(id)? else {
                if matches!(request, Request::GradeNow { .. }) {
                    return Err(missing(Table::Cards, id.0));
                }
                continue;
            };
            if matches!(request, Request::GradeNow { .. }) {
                if card.original_deck_id.0 != 0 {
                    return Err(refused(Refusal::InFilteredDeck { card: id }));
                }
                let home = ctx
                    .scope()
                    .deck(card.deck_id)?
                    .ok_or_else(|| missing(Table::Decks, card.deck_id.0))?;
                let DeckKind::Normal(n) = &home.kind.kind else {
                    return Err(refused(Refusal::InFilteredDeck { card: id }));
                };
                let preset = PresetView::for_config(ctx.scope(), DeckConfigId(n.config_id))?;
                if models.model(&preset).is_err() {
                    unsupported.insert(preset.source().id);
                }
            }
            if matches!(request, Request::SetDue { .. }) {
                let home_id = CardSched::new(&card).home_deck();
                let home = ctx
                    .scope()
                    .deck(home_id)?
                    .ok_or_else(|| missing(Table::Decks, home_id.0))?;
                let DeckKind::Normal(n) = &home.kind.kind else {
                    return Err(refused(Refusal::HomeDeckFiltered { deck: home_id }));
                };
                let config = match ctx.scope().deck_config(DeckConfigId(n.config_id))? {
                    Some(c) => Some(c),
                    None => ctx.scope().deck_config(DeckConfigId(1))?,
                };
                starting.insert(
                    id,
                    config
                        .as_ref()
                        .map(|c| PresetView::new(c).fields().initial_ease)
                        .unwrap_or(2.5),
                );
            }
            cards.push(card);
        }
        if !unsupported.is_empty() {
            return Err(refused(Refusal::PresetUnsupported {
                preset_ids: unsupported.into_iter().collect(),
            }));
        }
        if cards.is_empty() {
            return Ok(ResultValue {
                count: 0,
                head_diff: None,
            });
        }
        let mut count = 0;
        if let Request::GradeNow { rating, .. } = request {
            let mut balance = grade_balance(ctx.scope(), day.today, day.next_day_at, bound.core.exec.clock.as_ref())?;
            let enabled = SchedConfig::new(ctx.scope()).load_balancer_enabled()?;
            for original in cards {
                let card = ctx
                    .scope()
                    .card(original.id)?
                    .ok_or_else(|| missing(Table::Cards, original.id.0))?;
                let preset =
                    PresetView::for_card(ctx.scope(), &card)?.ok_or_else(|| missing(Table::Decks, card.deck_id.0))?;
                let mut chooser = balance.chooser(
                    preset.source().id,
                    preset.fields().bury_reviews.then_some(card.note_id),
                    fuzz::seed(card.id.0 as u64, u64::from(card.reps)),
                );
                let prediction = answer::predict_in(
                    ctx.scope(),
                    card.id,
                    &mut models,
                    day.today,
                    day.next_day_at,
                    now,
                    if enabled { Some(&mut chooser) } else { None },
                )?;
                let result = answer::apply_in(ctx, &bound, prediction.answer(rating, 0), None, None, false)?;
                // Grade Now's private W10 chooser observes earlier answers; it
                // never attaches AnswerMetadata or mutates a W8/W9 owner.
                if enabled {
                    let written = ctx
                        .scope()
                        .card(card.id)?
                        .ok_or_else(|| missing(Table::Cards, card.id.0))?;
                    if written.queue == CardQueue::Review {
                        balance.append(
                            preset.source().id,
                            written.interval,
                            Entry {
                                card: written.id,
                                note: written.note_id,
                            },
                        );
                    }
                }
                drop(result);
                count += 1;
            }
        } else {
            let mut next = crate::cardgen::position::read_counter(ctx.scope())?;
            let days = match &request {
                Request::SetDue { days, .. } => Some(Days::parse(days).map_err(|e| refused(Refusal::InvalidDays(e)))?),
                _ => None,
            };
            for mut card in cards {
                let old_interval = card.interval;
                let changed = match &request {
                    Request::Bury(_) => values::bury::apply(&mut card),
                    Request::Suspend(_) => values::suspend::apply(&mut card),
                    Request::Unsuspend(_) | Request::UnburyDeck { .. } => values::suspend::restore(&mut card),
                    Request::SetDue { .. } => {
                        let d = days.expect("parsed days").draw(ctx.random());
                        let ease = starting[&card.id];
                        values::set_due::apply(&mut card, d, day.today, day.next_day_at, ease);
                        true
                    }
                    Request::Reset {
                        restore_position,
                        reset_counts,
                        ..
                    } => {
                        values::reset::apply(&mut card, &mut next, *restore_position, *reset_counts);
                        true
                    }
                    Request::EmptyFiltered { .. } => {
                        values::filtered_empty::apply(&mut card);
                        true
                    }
                    Request::GradeNow { .. } => unreachable!(),
                };
                if !changed {
                    continue;
                }
                if matches!(request, Request::Bury(_)) {
                    put_config(
                        ctx.scope(),
                        "lastUnburied",
                        &ConfigValue::from(u64::from(day.today)),
                        now,
                    )?;
                }
                card.mtime = now.whole_secs();
                card.usn = Usn::PENDING;
                ctx.scope().put_card_forced(&card)?;
                if matches!(request, Request::SetDue { .. } | Request::Reset { .. }) {
                    let reset = matches!(request, Request::Reset { .. });
                    let log = RevlogEntry {
                        id: RevlogId(0),
                        card_id: card.id,
                        usn: Usn::PENDING,
                        button: 0,
                        interval: if reset {
                            0
                        } else {
                            card.interval.min(i32::MAX as u32) as i32
                        },
                        last_interval: old_interval.min(i32::MAX as u32) as i32,
                        ease_factor: if reset { 0 } else { values::set_due::log_factor(&card) },
                        answer_millis: 0,
                        kind: RevlogKind::Manual,
                    };
                    ctx.scope().add_answer_revlog(&log, now)?;
                }
                count += 1;
            }
            if count > 0 {
                match request {
                    Request::SetDue {
                        days,
                        context: Some(context),
                        ..
                    } => put_config(
                        ctx.scope(),
                        match context {
                            Context::Reviewer => "setDueReviewer",
                            Context::Browser => "setDueBrowser",
                        },
                        &ConfigValue::from(days.as_str()),
                        now,
                    )?,
                    Request::Reset {
                        restore_position,
                        reset_counts,
                        context,
                        ..
                    } => {
                        crate::cardgen::position::write_counter(ctx.scope(), next)?;
                        if let Some(context) = context {
                            let (restore, counts) = match context {
                                Context::Reviewer => ("restorePositionReviewer", "resetCountsReviewer"),
                                Context::Browser => ("restorePositionBrowser", "resetCountsBrowser"),
                            };
                            put_config(ctx.scope(), restore, &ConfigValue::from(restore_position), now)?;
                            put_config(ctx.scope(), counts, &ConfigValue::from(reset_counts), now)?;
                        }
                    }
                    _ => (),
                }
                ctx.scope().set_col_mod(now)?;
            }
        }
        Ok(ResultValue { count, head_diff: None })
    })
}
fn put_config(scope: &mut WriteScope<'_>, key: &str, value: &ConfigValue, at: EpochMillis) -> Result<(), EngineError> {
    if scope.config_value(key)?.as_ref() != Some(value) {
        scope.put_config_entry(&ConfigEntry {
            key: key.into(),
            value: value.compact().into_bytes(),
            mtime: EpochSecs(at.whole_secs().0),
            usn: Usn::PENDING,
        })?;
    }
    Ok(())
}
fn grade_balance(
    rows: &impl ReadRows,
    today: u32,
    boundary: i64,
    clock: &dyn meguri_model::clock::Clock,
) -> Result<LoadBalancer, EngineError> {
    let enabled = SchedConfig::new(rows).load_balancer_enabled()?;
    let decks = rows
        .deck_rows()?
        .into_iter()
        .filter_map(|d| match d.kind.kind {
            DeckKind::Normal(ref n) => Some((d.id, DeckConfigId(n.config_id))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let presets = if enabled {
        rows.deck_config_rows()?
            .into_iter()
            .map(|p| (p.id, p.config.easy_days_percentages.clone()))
            .collect()
    } else {
        Vec::new()
    };
    let counted = if enabled {
        rows.answer_due_range(today)?
            .into_iter()
            .filter_map(|r| {
                decks.get(&r.deck).map(|p| {
                    (
                        *p,
                        r.day,
                        Entry {
                            card: r.card,
                            note: r.note,
                        },
                    )
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    LoadBalancer::new(today, boundary, clock, presets, counted)
        .map_err(|e| EngineError::InvalidArgument { message: e.to_string() })
}
