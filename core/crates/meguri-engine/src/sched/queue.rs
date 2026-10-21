//! U41 product query provider. Maintenance finishes before any reader lease;
//! returned values own all data, and no reader crosses the calling boundary.
use crate::{
    Collection, EngineError, Generation,
    changes::observers::ObservedCommit,
    ports::{ChangeObserver, GenerationListener},
};
use meguri_model::ids::DeckId;
use meguri_sched::{
    counts::{
        deck_tree::{self, DeckTree},
        finished::{self, DeckStudyState},
    },
    queue::{
        self, Environment,
        rebuild_rules::{self, Action, CardImages, Context, Stamp},
    },
};
use meguri_storage::{ReadRows, ReaderRole};
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone)]
pub struct QueueValidity(Arc<Mutex<Watch>>, Collection, i64);
struct Watch {
    stamp: Stamp,
    day: u32,
    action: Action,
    ownership: Arc<dyn Fn(Stamp) -> bool + Send + Sync>,
}
impl QueueValidity {
    pub fn action(&self) -> Action {
        let mut watch = self.0.lock().unwrap();
        if self.1.current_generation().0 != watch.stamp.generation
            || !self.1.core.exec.day_clock.queue_day_is_current(watch.day, self.2)
        {
            watch.action = Action::Rebuild;
        }
        watch.action
    }
}
pub struct QueueSnapshot {
    pub value: queue::Queue,
    pub stamp: Stamp,
    pub validity: QueueValidity,
}
#[derive(Clone)]
struct CachedTree {
    stamp: Stamp,
    day: u32,
    boundary: i64,
    colmod: i64,
    now: i64,
    value: DeckTree,
}
#[derive(Default)]
pub struct QueueProvider {
    tree: Mutex<Option<CachedTree>>,
    watches: Mutex<Vec<Weak<Mutex<Watch>>>>,
    #[cfg(feature = "test-hooks")]
    before_reader: Mutex<Option<std::sync::mpsc::Sender<ReaderRole>>>,
}
impl QueueProvider {
    /// Notify once after the prelude, just before acquiring the role reader.
    #[cfg(feature = "test-hooks")]
    pub fn notify_before_next_reader(&self, sender: std::sync::mpsc::Sender<ReaderRole>) {
        *self.before_reader.lock().unwrap() = Some(sender);
    }

    fn stamp(snapshot: &crate::readers::Snapshot<'_>) -> Stamp {
        Stamp {
            seq: snapshot.seq().0,
            generation: snapshot.generation().0,
        }
    }
    fn day(&self, day: u32) {
        self.watches.lock().unwrap().retain(|w| {
            let Some(w) = w.upgrade() else { return false };
            let mut w = w.lock().unwrap();
            if w.day != day {
                w.action = Action::Rebuild;
            }
            true
        });
    }
    fn watch(
        &self,
        collection: &Collection,
        stamp: Stamp,
        day: u32,
        boundary: i64,
        ownership: Arc<dyn Fn(Stamp) -> bool + Send + Sync>,
    ) -> QueueValidity {
        let validity = QueueValidity(
            Arc::new(Mutex::new(Watch {
                stamp,
                day,
                action: Action::None,
                ownership,
            })),
            collection.clone(),
            boundary,
        );
        self.watches.lock().unwrap().push(Arc::downgrade(&validity.0));
        validity
    }
    /// Establish a D8-complete point, then pin that exact committed state.
    /// A racing commit releases the reader and starts a fresh prelude; no
    /// maintenance is ever requested with a lease or transaction held.
    fn read<T>(
        &self,
        collection: &Collection,
        role: ReaderRole,
        read: impl FnOnce(&crate::readers::Snapshot<'_>, &Environment) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        self.read_timed(collection, role, |snapshot, env, _| read(snapshot, env))
    }

    /// Shared D8 snapshot protocol for answer prediction and cache construction.
    pub(super) fn read_timed<T>(
        &self,
        collection: &Collection,
        role: ReaderRole,
        read: impl FnOnce(&crate::readers::Snapshot<'_>, &Environment, i64) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        loop {
            let day = collection.check_rollover()?;
            let now_ms = collection.core.exec.clock.now_ms();
            let offset = collection.core.exec.clock.utc_offset_at(now_ms);
            let zone = collection.core.exec.clock.zone_id();
            // Even an empty maintenance op reserves a seq. Pin the state
            // after it completes, rather than rejecting our own prelude.
            let seq = collection.last_seq();
            // The commit lock is released before day invalidation and the
            // observers run. Settle this seq before trusting its day report;
            // a snapshot that races ahead of it must still retry below.
            collection.core.exec.stream.wait_settled(seq);
            if collection.core.exec.clock.now_ms().div_euclid(1000) != now_ms.div_euclid(1000) {
                continue;
            }
            #[cfg(feature = "test-hooks")]
            if let Some(sender) = self.before_reader.lock().unwrap().take() {
                let _ = sender.send(role);
            }
            let lease = collection.reader(role)?;
            let snapshot = lease.snapshot()?;
            if snapshot.seq() != seq
                || collection.core.exec.clock.now_ms().div_euclid(1000) != now_ms.div_euclid(1000)
                || collection.core.exec.clock.utc_offset_at(now_ms) != offset
                || collection.core.exec.clock.zone_id() != zone
                || !collection
                    .core
                    .exec
                    .day_clock
                    .queue_report_is_current(day, now_ms.div_euclid(1000), offset, &zone)
            {
                drop(snapshot);
                drop(lease);
                continue;
            }
            let env = Environment::read(&*snapshot, day.today, now_ms.div_euclid(1000), day.next_day_at)?;
            self.day(day.today);
            return read(&snapshot, &env, now_ms);
        }
    }
    pub fn queue(&self, collection: &Collection, deck: Option<DeckId>) -> Result<QueueSnapshot, EngineError> {
        self.queue_with_ownership(collection, deck, Arc::new(|_| false))
    }
    /// U43 supplies proof for its own answer/undo/redo seq in this generation.
    /// The callback runs on the writer and must not wait for another op.
    pub fn queue_with_ownership(
        &self,
        collection: &Collection,
        deck: Option<DeckId>,
        ownership: Arc<dyn Fn(Stamp) -> bool + Send + Sync>,
    ) -> Result<QueueSnapshot, EngineError> {
        self.read(collection, ReaderRole::Interactive, |snapshot, env| {
            let stamp = Self::stamp(snapshot);
            let value = queue::build(&**snapshot, env, deck.unwrap_or(env.current))?;
            let validity = self.watch(collection, stamp, env.today, env.next_day_at, ownership);
            if collection.last_seq().0 != stamp.seq {
                validity.0.lock().unwrap().action = Action::Rebuild;
            }
            Ok(QueueSnapshot { value, stamp, validity })
        })
    }
    /// Queue and real U45 head states from one short product snapshot.
    pub fn queue_with_head_states(
        &self,
        collection: &Collection,
        deck: Option<DeckId>,
        cache: &mut meguri_sched::states::fsrs_glue::FsrsCache,
    ) -> Result<(QueueSnapshot, Option<meguri_sched::states::engine::SchedulingStates>), EngineError> {
        self.read(collection, ReaderRole::Interactive, |snapshot, env| {
            let stamp = Self::stamp(snapshot);
            let value = queue::build(&**snapshot, env, deck.unwrap_or(env.current))?;
            let head = value.presentation().first().map(|e| e.id);
            let states = head.map(|id| head_states(&**snapshot, env, id, cache)).transpose()?;
            let validity = self.watch(collection, stamp, env.today, env.next_day_at, Arc::new(|_| false));
            if collection.last_seq().0 != stamp.seq {
                validity.0.lock().unwrap().action = Action::Rebuild;
            }
            Ok((QueueSnapshot { value, stamp, validity }, states))
        })
    }
    pub fn deck_counts(&self, collection: &Collection) -> Result<DeckTree, EngineError> {
        self.counts(collection, false)
    }
    /// Force a grouped refresh (also used by U12 to measure actual work).
    pub fn refresh_deck_counts(&self, collection: &Collection) -> Result<DeckTree, EngineError> {
        self.counts(collection, true)
    }
    fn counts(&self, collection: &Collection, refresh: bool) -> Result<DeckTree, EngineError> {
        self.read(collection, ReaderRole::Query, |snapshot, env| {
            let stamp = Self::stamp(snapshot);
            let colmod = snapshot.col()?.modified.0;
            {
                let cache = self.tree.lock().unwrap();
                if let Some(c) = cache.as_ref().filter(|c| {
                    !refresh
                        && c.stamp == stamp
                        && c.day == env.today
                        && c.boundary == env.next_day_at
                        && c.colmod == colmod
                        && env.now >= c.now
                        && env.now < c.value.next_change_at
                }) {
                    return Ok(c.value.clone());
                }
            }
            let groups = snapshot.sched_deck_aggregates(env.today, env.now, env.next_day_at, env.ahead)?;
            let value = deck_tree::build(env, &groups, false);
            *self.tree.lock().unwrap() = Some(CachedTree {
                stamp,
                day: env.today,
                boundary: env.next_day_at,
                colmod,
                now: env.now,
                value: value.clone(),
            });
            Ok(value)
        })
    }
    pub fn deck_study_state(
        &self,
        collection: &Collection,
        deck: Option<DeckId>,
    ) -> Result<DeckStudyState, EngineError> {
        self.read(collection, ReaderRole::Interactive, |snapshot, env| {
            let deck = deck.unwrap_or(env.current);
            let q = queue::build(&**snapshot, env, deck)?;
            let groups = snapshot.sched_deck_aggregates(env.today, env.now, env.next_day_at, env.ahead)?;
            let list = deck_tree::build(env, &groups, false);
            let tomorrow = deck_tree::build(env, &groups, true);
            let empty = snapshot.sched_subtree_empty(&env.subtree(deck, false))?;
            Ok(finished::build(env, &q, &list, &tomorrow, &groups, empty))
        })
    }
}
impl ChangeObserver for QueueProvider {
    fn name(&self) -> &str {
        "deck-counts"
    }
    fn observe(&self, c: &ObservedCommit<'_>) -> Result<(), EngineError> {
        *self.tree.lock().unwrap() = None;
        let images = c
            .cards
            .iter()
            .map(|r| CardImages {
                id: r.id,
                before: r.before.as_deref(),
                after: r.after.as_deref(),
            })
            .collect::<Vec<_>>();
        let commit = Stamp {
            seq: c.seq.0,
            generation: c.generation.0,
        };
        self.watches.lock().unwrap().retain(|w| {
            let Some(w) = w.upgrade() else { return false };
            let mut w = w.lock().unwrap();
            if commit.seq > w.stamp.seq || commit.generation != w.stamp.generation {
                let action = rebuild_rules::classify(
                    c.changes,
                    &images,
                    Context {
                        built: w.stamp,
                        commit,
                        owned: (w.ownership)(commit).then_some(commit),
                        day_changed: false,
                    },
                );
                w.action = w.action.max(action);
            }
            true
        });
        Ok(())
    }
}
impl GenerationListener for QueueProvider {
    fn name(&self) -> &str {
        "deck-counts"
    }
    fn collection_replaced(&self, _old: Generation, _new: Generation) {
        *self.tree.lock().unwrap() = None;
        self.watches.lock().unwrap().retain(|w| {
            let Some(w) = w.upgrade() else { return false };
            w.lock().unwrap().action = Action::Rebuild;
            true
        });
    }
}

fn head_states(
    rows: &impl ReadRows,
    env: &Environment,
    id: meguri_model::ids::CardId,
    cache: &mut meguri_sched::states::fsrs_glue::FsrsCache,
) -> Result<meguri_sched::states::engine::SchedulingStates, EngineError> {
    use meguri_sched::{
        model::{card_sched::CardSched, deck_sched::DeckSched, preset::PresetView, sched_config::SchedConfig},
        states::engine::{StateInput, next_states},
    };
    let card = rows.card(id)?.ok_or_else(|| EngineError::InvalidArgument {
        message: "missing queue head".into(),
    })?;
    let preset = PresetView::for_card(rows, &card)?.ok_or_else(|| EngineError::InvalidArgument {
        message: "queue head has no normal home deck".into(),
    })?;
    let home = rows
        .deck(CardSched::new(&card).home_deck())?
        .ok_or_else(|| EngineError::InvalidArgument {
            message: "missing home deck".into(),
        })?;
    let normal = match &home.kind.kind {
        meguri_model::proto::deck::DeckKind::Normal(n) => Some(n),
        _ => None,
    };
    let desired_retention = DeckSched::new(normal, &home.common).retention(preset.fields().desired_retention);
    let mut history = Vec::new();
    if card.card_type != meguri_model::card::CardType::New && card.data.memory_state.is_none() {
        rows.visit_fsrs_histories(
            &meguri_storage::queries::revlog_fsrs::HistorySelection::Cards(vec![id]),
            |_, found| -> Result<(), meguri_storage::StorageError> {
                history.extend_from_slice(found);
                Ok(())
            },
        )?;
    }
    let memory_state = meguri_sched::memory::resolve::on_the_fly(cache, &preset, &card, &history, env.next_day_at)
        .map_err(|e| EngineError::Unsupported { feature: e.to_string() })?;
    let last_review_time = match card.data.last_review_secs {
        Some(t) => Some(t),
        None => rows.fsrs_last_review_time(id)?,
    };
    next_states(
        cache,
        &StateInput {
            card: &card,
            preset: &preset,
            memory_state,
            last_review_time,
            desired_retention,
            today: env.today,
            next_day_at: env.next_day_at,
            now: env.now,
            short_term_with_steps: SchedConfig::new(rows).fsrs_short_term_with_steps()?,
            learn_ahead: env.ahead,
        },
        None,
    )
    .map_err(|e| EngineError::Unsupported { feature: e.to_string() })
}
