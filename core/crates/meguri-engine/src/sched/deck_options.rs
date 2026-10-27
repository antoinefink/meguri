//! U49: staged editing and one cancellable, journalled save on the live writer.
use crate::{
    Collection, EngineError, Generation, OpResult, OpSpec, SchemaReason, Seq,
    executor::{catalog, preconditions::Precondition},
    tags::RunOptions,
    undo::{entry::StepKind, names},
};
use meguri_model::{
    card::CardType,
    changes::OpSource,
    deck_config::DeckConfig,
    ids::{DeckConfigId, DeckId},
    proto::deck::{DayLimit, DeckKind},
    rng::RandomSource,
};
use meguri_sched::{
    model::{
        deck_sched::{DeckSched, clear_today_limit},
        preset::PresetView,
        sched_config::{self, SchedConfig},
    },
    options::{self, Field, Issue, Severity, Value},
};
use meguri_storage::{ReadRows, ReaderRole, Table};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Preset {
    pub key: i64,
    pub name: String,
    pub use_count: u64,
    pub values: BTreeMap<Field, Value>,
    source: DeckConfig,
}
#[derive(Clone, Debug)]
pub struct TodayLimit {
    pub limit: u32,
    pub today: u32,
    pub active: bool,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub seq: Seq,
    pub generation: Generation,
    pub deck: DeckId,
    pub name: String,
    pub assigned: i64,
    pub opening: i64,
    pub preset_missing: bool,
    pub today: u32,
    pub today_date: String,
    pub presets: Vec<Preset>,
    pub deck_values: BTreeMap<Field, Value>,
    pub globals: BTreeMap<Field, Value>,
    pub new_today: Option<TodayLimit>,
    pub review_today: Option<TodayLimit>,
    pub ignore_count: Option<meguri_storage::queries::revlog_fsrs::IncludedCount>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitScope {
    Preset,
    ThisDeck,
    TodayOnly,
}
#[derive(Clone, Debug)]
pub struct Draft {
    pub base: Snapshot,
    pub selected: i64,
    pub presets: Vec<Preset>,
    pub removals: BTreeSet<i64>,
    pub deck_values: BTreeMap<Field, Value>,
    pub globals: BTreeMap<Field, Value>,
    pub subtree: bool,
    next_handle: i64,
}
fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::InvalidArgument {
        message: message.into(),
    }
}
fn sort(presets: &mut [Preset]) {
    presets.sort_by(|a, b| {
        meguri_text::fold::compare_folded(&a.name, &b.name)
            .then_with(|| a.name.as_bytes().cmp(b.name.as_bytes()))
            .then(a.key.cmp(&b.key))
    });
}
fn globals(rows: &impl ReadRows) -> Result<BTreeMap<Field, Value>, EngineError> {
    let c = SchedConfig::new(rows);
    Ok([
        (
            Field::NewCardsIgnoreReviewLimit,
            Value::Boolean(c.new_cards_ignore_review_limit()?),
        ),
        (
            Field::ApplyAllParentLimits,
            Value::Boolean(c.apply_all_parent_limits()?),
        ),
        (Field::LoadBalancerEnabled, Value::Boolean(c.load_balancer_enabled()?)),
    ]
    .into())
}
fn number(v: Option<u32>) -> Value {
    v.map_or(Value::None, |n| Value::Number(f64::from(n)))
}
fn deck_values(normal: &meguri_model::proto::deck::NormalDeck, today: u32) -> BTreeMap<Field, Value> {
    [
        (Field::NewLimit, number(normal.new_limit)),
        (Field::ReviewLimit, number(normal.review_limit)),
        (
            Field::NewLimitToday,
            number(
                normal
                    .new_limit_today
                    .as_ref()
                    .filter(|l| l.today == today)
                    .map(|l| l.limit),
            ),
        ),
        (
            Field::ReviewLimitToday,
            number(
                normal
                    .review_limit_today
                    .as_ref()
                    .filter(|l| l.today == today)
                    .map(|l| l.limit),
            ),
        ),
        (
            Field::DeckDesiredRetention,
            normal
                .desired_retention
                .map_or(Value::None, |r| Value::Number(f64::from(r))),
        ),
    ]
    .into()
}
/// Gregorian date of the day start, using the injected offset at that instant.
fn today_date(collection: &Collection, next: i64) -> Result<String, EngineError> {
    let start = next - 86400;
    let offset = collection
        .core
        .exec
        .clock
        .utc_offset_at(start.saturating_mul(1000))
        .seconds();
    let millis = (start + i64::from(offset)).div_euclid(86400) * 86400000;
    let date_text = |y: i64, m: i64, d: i64| {
        if (0..=9999).contains(&y) {
            format!("{y:04}-{m:02}-{d:02}")
        } else {
            format!("{y:+}-{m:02}-{d:02}")
        }
    };
    let cutoff = |y: i64, m: i64, d: i64| options_date(&date_text(y, m, d));
    let mut low = -262143;
    let mut high = 262142;
    while low < high {
        let mid = (low + high + 1) / 2;
        if cutoff(mid, 1, 1)? <= millis {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let year = low;
    let mut month = 1;
    while month < 12 && cutoff(year, month + 1, 1)? <= millis {
        month += 1;
    }
    let day = (millis - cutoff(year, month, 1)?) / 86400000 + 1;
    let text = date_text(year, month, day);
    options_date(&text)?;
    Ok(text)
}
fn options_date(text: &str) -> Result<i64, EngineError> {
    meguri_sched::memory::ignore_before::cutoff_millis(text).map_err(|_| invalid("invalid calendar date"))
}
fn search(
    rows: &impl ReadRows,
    collection: &Collection,
    day: super::DayReport,
) -> Result<meguri_search::SearchContext, EngineError> {
    let bool_key = |key, default| -> Result<bool, EngineError> {
        Ok(rows.config_value(key)?.and_then(|v| v.as_bool()).unwrap_or(default))
    };
    Ok(meguri_search::SearchContext {
        now: collection.core.exec.clock.now_ms() / 1000,
        today: day.today,
        next_day_at: day.next_day_at,
        learn_ahead_secs: SchedConfig::new(rows).collapse_time()?,
        current_deck: crate::decks::current(rows)?,
        normalize_note_text: bool_key("normalize_note_text", true)?,
        ignore_accents: bool_key("ignoreAccentsInSearch", false)?,
        fold: meguri_search::FoldMode::Unicode,
    })
}
pub fn snapshot(collection: &Collection, deck: DeckId) -> Result<Snapshot, EngineError> {
    let day = collection.check_rollover()?;
    let today_date = today_date(collection, day.next_day_at)?;
    let lease = collection.reader(ReaderRole::Maintenance)?;
    let rows = lease.snapshot()?;
    let target = crate::decks::require(&*rows, deck)?;
    let DeckKind::Normal(normal) = &target.kind.kind else {
        return Err(crate::decks::refused(crate::decks::Refusal::FilteredDeck));
    };
    let decks = rows.deck_rows()?;
    let mut presets = rows
        .deck_config_rows()?
        .into_iter()
        .map(|source| Preset {
            key: source.id.0,
            name: source.name.clone(),
            use_count: decks
                .iter()
                .filter(|d| matches!(&d.kind.kind,DeckKind::Normal(n) if n.config_id==source.id.0))
                .count() as u64,
            values: options::fields::read(&source.config.for_use()),
            source,
        })
        .collect::<Vec<_>>();
    sort(&mut presets);
    let missing = !presets.iter().any(|p| p.key == normal.config_id);
    let opening = if missing { 1 } else { normal.config_id };
    if !presets.iter().any(|p| p.key == opening) {
        let source = crate::deck_config::defaults();
        presets.push(Preset {
            key: 1,
            name: source.name.clone(),
            use_count: 0,
            values: options::fields::read(&source.config),
            source,
        });
        sort(&mut presets);
    }
    let preset = presets.iter().find(|p| p.key == opening).expect("opening preset");
    let date = &preset.source.config.ignore_revlogs_before_date;
    let ignore_count = if date.is_empty() || date == "1970-01-01" {
        None
    } else {
        Some(super::memory::ignored_before_count(
            &*rows,
            &PresetView::new(&preset.source),
            date,
            &search(&*rows, collection, day)?,
        )?)
    };
    let today_limit = |value: &Option<DayLimit>| {
        value.as_ref().map(|v| TodayLimit {
            limit: v.limit,
            today: v.today,
            active: v.today == day.today,
        })
    };
    Ok(Snapshot {
        seq: rows.seq(),
        generation: rows.generation(),
        deck,
        name: crate::decks::names::human_name(&target.name),
        assigned: normal.config_id,
        opening,
        preset_missing: missing,
        today: day.today,
        today_date,
        presets,
        deck_values: deck_values(normal, day.today),
        globals: globals(&*rows)?,
        new_today: today_limit(&normal.new_limit_today),
        review_today: today_limit(&normal.review_limit_today),
        ignore_count,
    })
}
impl Draft {
    pub fn new(base: Snapshot) -> Self {
        Self {
            selected: base.opening,
            presets: base.presets.clone(),
            deck_values: base.deck_values.clone(),
            globals: base.globals.clone(),
            base,
            removals: BTreeSet::new(),
            subtree: false,
            next_handle: -1,
        }
    }
    pub fn select(&mut self, key: i64) -> Result<(), EngineError> {
        self.preset(key)?;
        self.selected = key;
        Ok(())
    }
    pub fn preset(&self, key: i64) -> Result<&Preset, EngineError> {
        self.presets
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| invalid("unknown draft preset"))
    }
    pub fn edit(&mut self, key: Option<i64>, field: Field, value: Value) -> Result<(), EngineError> {
        if let Some(key) = key {
            if !Field::PRESET.contains(&field) {
                return Err(invalid("not an editable preset field"));
            }
            self.preset(key)?;
            let value = if matches!(field, Field::LearnSteps | Field::RelearnSteps) {
                match value {
                    Value::Text(text) => match options::steps_parse::parse(&text, field, Some(key)) {
                        Ok(v) => Value::Steps(v),
                        Err(_) => Value::Text(text),
                    },
                    other => other,
                }
            } else {
                value
            };
            self.presets
                .iter_mut()
                .find(|p| p.key == key)
                .unwrap()
                .values
                .insert(field, value);
        } else if Field::DECK.contains(&field) {
            self.deck_values.insert(field, value);
        } else if Field::GLOBAL.contains(&field) {
            self.globals.insert(field, value);
        } else {
            return Err(invalid("not a deck options field"));
        }
        Ok(())
    }
    /// O6.2: selecting a less specific tab clears the more specific values.
    pub fn set_limit(&mut self, review: bool, scope: LimitScope, value: f64) -> Result<(), EngineError> {
        let (preset, deck, today) = if review {
            (Field::ReviewsPerDay, Field::ReviewLimit, Field::ReviewLimitToday)
        } else {
            (Field::NewPerDay, Field::NewLimit, Field::NewLimitToday)
        };
        match scope {
            LimitScope::Preset => {
                self.edit(Some(self.selected), preset, Value::Number(value))?;
                self.edit(None, deck, Value::None)?;
                self.edit(None, today, Value::None)?;
            }
            LimitScope::ThisDeck => {
                self.edit(None, deck, Value::Number(value))?;
                self.edit(None, today, Value::None)?;
            }
            LimitScope::TodayOnly => self.edit(None, today, Value::Number(value))?,
        }
        Ok(())
    }
    pub fn limit_scope(&self, review: bool) -> LimitScope {
        let (deck, today) = if review {
            (Field::ReviewLimit, Field::ReviewLimitToday)
        } else {
            (Field::NewLimit, Field::NewLimitToday)
        };
        if !matches!(self.deck_values.get(&today), Some(Value::None) | None) {
            LimitScope::TodayOnly
        } else if !matches!(self.deck_values.get(&deck), Some(Value::None) | None) {
            LimitScope::ThisDeck
        } else {
            LimitScope::Preset
        }
    }
    pub fn set_retention(&mut self, this_deck: bool, value: f64) -> Result<(), EngineError> {
        if this_deck {
            self.edit(None, Field::DeckDesiredRetention, Value::Number(value))
        } else {
            self.edit(Some(self.selected), Field::DesiredRetention, Value::Number(value))?;
            self.edit(None, Field::DeckDesiredRetention, Value::None)
        }
    }
    pub fn restore_default(&mut self, key: Option<i64>, field: Field) -> Result<(), EngineError> {
        self.edit(key, field, field.metadata().default)
    }
    fn menu_name(
        &self,
        text: &str,
        exclude: Option<i64>,
        trim: bool,
        clock: &dyn meguri_model::clock::Clock,
    ) -> Option<String> {
        crate::deck_config::menu_name(
            text,
            &self
                .presets
                .iter()
                .filter(|p| Some(p.key) != exclude)
                .map(|p| p.name.clone())
                .collect::<Vec<_>>(),
            clock,
            trim,
        )
    }
    pub fn add(
        &mut self,
        text: &str,
        clone: bool,
        clock: &dyn meguri_model::clock::Clock,
    ) -> Result<Option<i64>, EngineError> {
        let Some(name) = self.menu_name(text, None, true, clock) else {
            return Ok(None);
        };
        let source = if clone {
            self.preset(self.selected)?.source.clone()
        } else {
            crate::deck_config::added_defaults()
        };
        let values = if clone {
            self.preset(self.selected)?.values.clone()
        } else {
            options::fields::read(&source.config.for_use())
        };
        let key = self.next_handle;
        self.next_handle = self
            .next_handle
            .checked_sub(1)
            .ok_or_else(|| invalid("too many staged presets"))?;
        self.presets.push(Preset {
            key,
            name,
            use_count: 0,
            values,
            source,
        });
        sort(&mut self.presets);
        self.selected = key;
        Ok(Some(key))
    }
    pub fn rename(&mut self, text: &str, clock: &dyn meguri_model::clock::Clock) -> Result<(), EngineError> {
        if self.preset(self.selected)?.name == text {
            return Ok(());
        }
        let name = self
            .menu_name(text, Some(self.selected), false, clock)
            .expect("rename accepts empty text");
        self.presets.iter_mut().find(|p| p.key == self.selected).unwrap().name = name;
        sort(&mut self.presets);
        Ok(())
    }
    pub fn remove(&mut self, key: i64) -> Result<(), EngineError> {
        if key == 1 {
            return Err(crate::decks::refused(crate::decks::Refusal::DefaultPreset));
        }
        let index = self
            .presets
            .iter()
            .position(|p| p.key == key)
            .ok_or_else(|| invalid("unknown draft preset"))?;
        if key > 0 {
            self.removals.insert(key);
        }
        self.presets.remove(index);
        self.selected = self.presets[index.saturating_sub(1).min(self.presets.len() - 1)].key;
        Ok(())
    }
    pub fn use_count(&self, key: i64) -> Result<u64, EngineError> {
        let p = self.preset(key)?;
        if key < 0 {
            return Ok(0);
        }
        Ok(p.use_count.saturating_sub(u64::from(key == self.base.assigned)) + u64::from(key == self.selected))
    }
    fn materialize(&self, p: &Preset) -> Result<DeckConfig, EngineError> {
        let mut out = p.source.clone();
        out.name = p.name.clone();
        let base = options::fields::read(&p.source.config.for_use());
        for field in Field::PRESET {
            let value = p.values.get(field).ok_or_else(|| invalid("missing editor field"))?;
            if !value.same(&base[field], *field) {
                options::fields::patch(&mut out.config, *field, value);
            }
        }
        Ok(out)
    }
    pub fn validate(&self, today: &str) -> Vec<Issue> {
        let mut issues = Vec::new();
        for p in &self.presets {
            let base = self.base.presets.iter().find(|b| b.key == p.key);
            for &field in Field::PRESET {
                if let Some(value) = p.values.get(&field) {
                    issues.extend(options::validate::field(
                        field,
                        Some(p.key),
                        value,
                        base.and_then(|b| b.values.get(&field)),
                        today,
                    ));
                }
            }
        }
        for &field in Field::DECK.iter().chain(Field::GLOBAL) {
            let (values, base) = if Field::DECK.contains(&field) {
                (&self.deck_values, &self.base.deck_values)
            } else {
                (&self.globals, &self.base.globals)
            };
            if let Some(value) = values.get(&field) {
                issues.extend(options::validate::field(field, None, value, base.get(&field), today));
            }
        }
        if let Ok(p) = self.preset(self.selected) {
            issues.extend(options::validate::effective_warnings(
                p.key,
                &p.values,
                &self.deck_values,
            ));
        }
        issues
    }
    pub fn schema_reasons(&self) -> Vec<SchemaReason> {
        if self.removals.is_empty() {
            Vec::new()
        } else {
            vec![SchemaReason::PresetRemoved]
        }
    }
    pub fn has_changes(&self) -> bool {
        !self.removals.is_empty()
            || self.selected != self.base.opening
            || self.presets.len() != self.base.presets.len()
            || self.presets.iter().any(|p| {
                self.base
                    .presets
                    .iter()
                    .find(|b| b.key == p.key)
                    .is_none_or(|b| b.name != p.name || !same_values(&p.values, &b.values))
            })
            || !same_values(&self.deck_values, &self.base.deck_values)
            || !same_values(&self.globals, &self.base.globals)
            || self.subtree
    }
}
fn same_values(a: &BTreeMap<Field, Value>, b: &BTreeMap<Field, Value>) -> bool {
    a.len() == b.len() && a.iter().all(|(f, v)| b.get(f).is_some_and(|b| v.same(b, *f)))
}
/// Counts use the staged raw preset/search and never assign it to a deck.
pub fn ignored_before_count(
    collection: &Collection,
    draft: &Draft,
    key: i64,
    date: &str,
) -> Result<meguri_storage::queries::revlog_fsrs::IncludedCount, EngineError> {
    let day = collection.check_rollover()?;
    let lease = collection.reader(ReaderRole::Maintenance)?;
    let rows = lease.snapshot()?;
    let p = draft.preset(key)?;
    // M5 uses only identity and the hidden param_search, which visible edits
    // cannot change. Do not parse unrelated, possibly invalid editor fields.
    let mut preset = p.source.clone();
    preset.id = DeckConfigId(key);
    super::memory::ignored_before_count(
        &*rows,
        &PresetView::new(&preset),
        date,
        &search(&*rows, collection, day)?,
    )
}

#[derive(Clone, Debug)]
pub struct Saved {
    pub preset: DeckConfigId,
    pub memory: super::memory::RecomputeReport,
}
fn save_spec(draft: &Draft, options: &RunOptions) -> OpSpec {
    // M4.3 permits a user's combined save to use the memory-recompute catalog
    // operation. Its cancellable transaction also owns all other U49 writes.
    let mut spec = OpSpec::new(catalog::MEMORY_RECOMPUTE, OpSource::UserUndoable)
        .schema_changes(draft.schema_reasons())
        .undo_step(names::DECK_OPTIONS, StepKind::Other)
        .declare_long_write(Duration::from_secs(1));
    for table in [Table::Decks, Table::DeckConfig, Table::Config] {
        spec = spec.precondition(Precondition::TableUntouched {
            table,
            since: draft.base.seq,
        });
    }
    if let Some(cancel) = &options.cancel {
        spec = spec.cancel(cancel.clone());
    }
    if let Some(progress) = &options.progress {
        spec = spec.progress(progress.clone());
    }
    spec
}
fn validate_save(rows: &impl ReadRows, draft: &Draft) -> Result<(), EngineError> {
    let target = crate::decks::require(rows, draft.base.deck)?;
    if !matches!(target.kind.kind, DeckKind::Normal(_)) {
        return Err(crate::decks::refused(crate::decks::Refusal::FilteredDeck));
    }
    if draft.removals.contains(&1) {
        return Err(crate::decks::refused(crate::decks::Refusal::DefaultPreset));
    }
    if draft.removals.contains(&draft.selected) {
        return Err(invalid("selected preset is removed"));
    }
    let keys = draft.presets.iter().map(|p| p.key).collect::<BTreeSet<_>>();
    if keys.len() != draft.presets.len() || !keys.contains(&draft.selected) {
        return Err(invalid("invalid preset handles"));
    }
    for &key in &draft.removals {
        crate::deck_config::require(rows, DeckConfigId(key))?;
    }
    for p in &draft.presets {
        if p.key > 0 {
            crate::deck_config::require(rows, DeckConfigId(p.key))?;
        }
        if p.values.len() != Field::PRESET.len() || p.values.keys().any(|f| !Field::PRESET.contains(f)) {
            return Err(invalid("invalid preset fields"));
        }
        draft.materialize(p)?;
    }
    if draft.deck_values.len() != Field::DECK.len() || draft.globals.len() != Field::GLOBAL.len() {
        return Err(invalid("incomplete deck options"));
    }
    Ok(())
}
pub fn save(collection: &Collection, draft: Draft, options: &RunOptions) -> Result<OpResult<Saved>, EngineError> {
    save_with(collection, draft, options, |_| Ok(()))
}
/// Composition seam for E1's hidden-option compatibility adapter. Product
/// callers use save, which can patch only the visible fields. The extension
/// runs in the same journal before assignment/remap/recompute.
pub fn save_with<F>(
    collection: &Collection,
    draft: Draft,
    options: &RunOptions,
    extension: F,
) -> Result<OpResult<Saved>, EngineError>
where
    F: FnOnce(&mut meguri_storage::WriteScope<'_>) -> Result<(), EngineError> + Send + 'static,
{
    if collection.generation() != draft.base.generation {
        return Err(EngineError::StaleGeneration {
            planned: draft.base.generation,
            current: collection.generation(),
        });
    }
    if let Some(cancel) = &options.cancel {
        cancel.check()?;
    }
    // Planning must not run check_rollover: it can unbury and clear history.
    {
        let lease = collection.reader(ReaderRole::Maintenance)?;
        let rows = lease.snapshot()?;
        validate_save(&*rows, &draft)?;
        let clock = &collection.core.exec.clock;
        let now = clock.now_ms();
        let read_integer = |key| -> Result<Option<i128>, EngineError> {
            Ok(match rows.config_value(key)? {
                Some(meguri_storage::ConfigValue::Integer(value)) => Some(value),
                _ => None,
            })
        };
        let day = meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
            crt: rows.col()?.created.0,
            creation_offset: meguri_sched::timing::creation_offset(read_integer("creationOffset")?),
            rollover_hour: meguri_sched::timing::rollover_hour(read_integer("rollover")?).unwrap_or(4),
            now: now.div_euclid(1000),
            offset: clock.utc_offset_at(now),
        });
        let issues = draft.validate(&today_date(collection, day.next_day_at)?);
        if issues.iter().any(|i| i.severity == Severity::Error) {
            return Err(EngineError::DeckOptionsInvalid(issues));
        }
    }
    let mut spec = save_spec(&draft, options);
    if spec.needs_safety_backup() {
        spec.safety_backup = Some(collection.admit_safety(&spec)?);
        spec.safety_admitted = true;
    }
    // No guarded collection write, including mutating preparation, precedes admission.
    if let Some(cancel) = &options.cancel {
        cancel.check()?;
    }
    let day = collection.check_rollover()?;
    let cancel = options.cancel.clone();
    let progress = options.progress.clone();
    collection.run(spec, move |ctx| {
        validate_save(ctx.scope(), &draft)?;
        let target = crate::decks::require(ctx.scope(), draft.base.deck)?;
        let before_decks = ctx.scope().deck_rows()?;
        let before = ctx
            .scope()
            .deck_config_rows()?
            .into_iter()
            .map(|p| (p.id.0, PresetView::new(&p)))
            .collect::<BTreeMap<_, _>>();
        let mut prepared = Vec::new();
        for p in &draft.presets {
            if p.key > 0 {
                crate::deck_config::require(ctx.scope(), DeckConfigId(p.key))?;
            }
            if p.values.len() != Field::PRESET.len() || p.values.keys().any(|f| !Field::PRESET.contains(f)) {
                return Err(invalid("invalid preset fields"));
            }
            prepared.push((p.key, draft.materialize(p)?));
        }
        if draft.deck_values.len() != Field::DECK.len() || draft.globals.len() != Field::GLOBAL.len() {
            return Err(invalid("incomplete deck options"));
        }
        ctx.check_cancel()?;
        // O7.2.1: admission preceded rollover; stamp before structural writes.
        if !draft.removals.is_empty() {
            crate::mark_schema_change(ctx.scope(), &[SchemaReason::PresetRemoved])?;
        }
        for &key in &draft.removals {
            ctx.scope().remove_deck_config(DeckConfigId(key))?;
        }
        // O7.2.2: name order, target last, including id allocation order.
        prepared.sort_by_key(|(key, _)| *key == draft.selected);
        let mut ids = BTreeMap::new();
        for (key, row) in prepared {
            ctx.check_cancel()?;
            let id = if key < 0 {
                ctx.scope().add_deck_config(&row)?
            } else {
                ctx.scope().put(&row)?;
                row.id
            };
            ids.insert(key, id);
        }
        extension(ctx.scope())?;
        let selected = ids[&draft.selected];
        // O7.2.3: live rows, with the target values for every reassigned deck.
        for mut deck in ctx.scope().deck_rows()? {
            let descendant = draft.subtree && crate::decks::names::descendant(&deck.name, &target.name);
            let DeckKind::Normal(normal) = &mut deck.kind.kind else {
                continue;
            };
            if deck.id == target.id || descendant || ctx.scope().deck_config(DeckConfigId(normal.config_id))?.is_none()
            {
                normal.config_id = selected.0;
                set_deck(normal, &draft.deck_values, day.today)?;
                crate::decks::save_in(ctx.scope(), deck)?;
            }
        }
        let mut recompute = Vec::new();
        let mut processed = 0u64;
        for deck in ctx.scope().deck_rows()? {
            ctx.check_cancel()?;
            let DeckKind::Normal(normal) = &deck.kind.kind else {
                continue;
            };
            let previous = before_decks
                .iter()
                .find(|d| d.id == deck.id)
                .and_then(|d| match &d.kind.kind {
                    DeckKind::Normal(n) => Some(n),
                    _ => None,
                });
            let old = previous.and_then(|n| before.get(&n.config_id));
            let new = PresetView::for_config(ctx.scope(), DeckConfigId(normal.config_id))?;
            // O8: use a true permutation; positions are per note, per home deck.
            let old_order = old
                .map(|p| p.insert_order())
                .unwrap_or(meguri_sched::model::preset::InsertOrder::Sequential);
            let resort = old_order != new.insert_order();
            let old_learn = old.map(|p| &p.fields().learn_steps);
            let old_relearn = old.map(|p| &p.fields().relearn_steps);
            let remap_learn = old_learn.is_some_and(|s| *s != new.fields().learn_steps);
            let remap_relearn = old_relearn.is_some_and(|s| *s != new.fields().relearn_steps);
            if resort || remap_learn || remap_relearn {
                let cards = ctx.scope().cards_of_deck(deck.id)?;
                let mut notes = if resort {
                    cards
                        .iter()
                        .filter(|c| c.card_type == CardType::New)
                        .map(|c| c.note_id)
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                if resort && new.insert_order() == meguri_sched::model::preset::InsertOrder::Random {
                    shuffle(&mut notes, ctx.random());
                }
                let positions = notes
                    .into_iter()
                    .enumerate()
                    .map(|(i, n)| (n, (i + 1) as i32))
                    .collect::<BTreeMap<_, _>>();
                for mut card in cards {
                    let mut changed = false;
                    if card.card_type == CardType::New && resort {
                        let position = positions[&card.note_id];
                        let value = if card.original_deck_id.0 != 0 {
                            &mut card.original_due
                        } else {
                            &mut card.due
                        };
                        if *value != position {
                            *value = position;
                            changed = true;
                        }
                    }
                    let lists = match card.card_type {
                        CardType::Learning if remap_learn => Some((old_learn.unwrap(), &new.fields().learn_steps)),
                        CardType::Relearning if remap_relearn => {
                            Some((old_relearn.unwrap(), &new.fields().relearn_steps))
                        }
                        _ => None,
                    };
                    if let Some((o, n)) = lists
                        && let Some(left) = options::remap::remaining(card.remaining_steps, o, n)
                    {
                        card.remaining_steps = left;
                        changed = true;
                    }
                    if changed {
                        ctx.scope().put(&card)?;
                    }
                    processed += 1;
                    if processed.is_multiple_of(256) {
                        ctx.check_cancel()?;
                        if let Some(p) = ctx.progress() {
                            p.report(processed, None);
                        }
                    }
                }
            }
            let retention = DeckSched::new(Some(normal), &deck.common).retention(new.fields().desired_retention);
            let refresh = old.is_none_or(|o| {
                o.parameters() != new.parameters()
                    || previous
                        .and_then(|n| n.desired_retention)
                        .unwrap_or(o.fields().desired_retention)
                        != retention
            });
            if refresh {
                recompute.push(deck.id);
            }
        }
        // O7.2.6: U46 owns memory history, rounding and memory-only changes.
        let planned = super::memory::plan(
            ctx.scope(),
            &super::memory::Target::Decks(recompute),
            day.next_day_at,
            &|| {
                if let Some(c) = &cancel {
                    c.check()?;
                }
                Ok(())
            },
            progress.as_ref(),
        )?;
        let memory = super::memory::apply_in(ctx, planned)?;
        write_globals(ctx.scope(), &draft.globals)?;
        ctx.check_cancel()?;
        Ok(Saved {
            preset: selected,
            memory,
        })
    })
}
fn shuffle<T>(items: &mut [T], random: &dyn RandomSource) {
    for i in (1..items.len()).rev() {
        let bound = (i + 1) as u64;
        let threshold = bound.wrapping_neg() % bound;
        let j = loop {
            let value = random.next_u64();
            if value >= threshold {
                break (value % bound) as usize;
            }
        };
        items.swap(i, j);
    }
}
fn optional(values: &BTreeMap<Field, Value>, field: Field) -> Result<Option<f64>, EngineError> {
    match values.get(&field) {
        Some(Value::None) => Ok(None),
        Some(Value::Number(v)) => Ok(Some(*v)),
        _ => Err(invalid("invalid deck field")),
    }
}
fn set_deck(
    normal: &mut meguri_model::proto::deck::NormalDeck,
    values: &BTreeMap<Field, Value>,
    today: u32,
) -> Result<(), EngineError> {
    normal.new_limit = optional(values, Field::NewLimit)?.map(|n| n as u32);
    normal.review_limit = optional(values, Field::ReviewLimit)?.map(|n| n as u32);
    normal.desired_retention = optional(values, Field::DeckDesiredRetention)?.map(|n| n as f32);
    for (field, limit) in [
        (Field::NewLimitToday, &mut normal.new_limit_today),
        (Field::ReviewLimitToday, &mut normal.review_limit_today),
    ] {
        if let Some(value) = optional(values, field)? {
            if let Some(stored) = limit {
                stored.limit = value as u32;
                stored.today = today;
            } else {
                *limit = Some(DayLimit::new(value as u32, today));
            }
        } else {
            clear_today_limit(limit, today);
        }
    }
    Ok(())
}
fn write_globals(
    scope: &mut meguri_storage::WriteScope<'_>,
    values: &BTreeMap<Field, Value>,
) -> Result<(), EngineError> {
    let stored = globals(scope)?;
    for &field in Field::GLOBAL {
        if values.get(&field) != stored.get(&field) {
            let Some(Value::Boolean(value)) = values.get(&field) else {
                return Err(invalid("invalid global option"));
            };
            match field {
                Field::NewCardsIgnoreReviewLimit => {
                    sched_config::set_new_cards_ignore_review_limit(scope, *value)?;
                }
                Field::ApplyAllParentLimits => {
                    sched_config::set_apply_all_parent_limits(scope, *value)?;
                }
                Field::LoadBalancerEnabled => {
                    sched_config::set_load_balancer_enabled(scope, *value)?;
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct Preferences {
    pub rollover: u8,
    pub learn_ahead_minutes: u32,
    pub load_balancer: bool,
}
#[derive(Clone, Debug, Default)]
pub struct PreferencesChange {
    pub rollover: Option<f64>,
    pub learn_ahead_minutes: Option<f64>,
    pub load_balancer: Option<bool>,
}
pub fn preferences(collection: &Collection) -> Result<Preferences, EngineError> {
    let rows = collection.reader(ReaderRole::Query)?;
    let c = SchedConfig::new(&*rows);
    Ok(Preferences {
        rollover: c.rollover_hour()?,
        learn_ahead_minutes: c.collapse_time()? / 60,
        load_balancer: c.load_balancer_enabled()?,
    })
}
pub fn save_preferences(
    collection: &Collection,
    change: PreferencesChange,
    options: &RunOptions,
) -> Result<OpResult<()>, EngineError> {
    if change.rollover.is_none() && change.learn_ahead_minutes.is_none() && change.load_balancer.is_none() {
        return Err(EngineError::InvalidArgument {
            message: "at least one scheduling preference is required".into(),
        });
    }
    let issues = validate_preferences(&change);
    if !issues.is_empty() {
        return Err(EngineError::DeckOptionsInvalid(issues));
    }
    let mut spec =
        OpSpec::named("scheduling_preferences", OpSource::UserUndoable).undo_step(names::PREFERENCES, StepKind::Other);
    if let Some(cancel) = &options.cancel {
        spec = spec.cancel(cancel.clone());
    }
    if let Some(progress) = &options.progress {
        spec = spec.progress(progress.clone());
    }
    let result = collection.run(spec, move |ctx| {
        let (old_hour, seconds, load_balancer) = {
            let config = SchedConfig::new(ctx.scope());
            (
                config.rollover_hour()?,
                config.collapse_time()?,
                config.load_balancer_enabled()?,
            )
        };
        let hour = change.rollover.map(|hour| hour as u8).unwrap_or(old_hour);
        let seconds = change
            .learn_ahead_minutes
            .map(|minutes| (minutes as u32) * 60)
            .unwrap_or(seconds);
        let load_balancer = change.load_balancer.unwrap_or(load_balancer);
        if hour != old_hour {
            sched_config::set_rollover_hour(ctx.scope(), hour)?;
        }
        sched_config::set_collapse_time(ctx.scope(), seconds)?;
        sched_config::set_load_balancer_enabled(ctx.scope(), load_balancer)?;
        Ok(())
    })?;
    // U40 invalidates from the committed change set. Wait for its automatic
    // recomputation so counts and the next queue see the new hour at return.
    collection
        .check_rollover()
        .map_err(|error| EngineError::CommitCompleted {
            seq: result.seq,
            message: error.to_string(),
        })?;
    Ok(result)
}

pub fn stage_add(
    collection: &Collection,
    draft: &mut Draft,
    name: &str,
    clone: bool,
) -> Result<Option<i64>, EngineError> {
    draft.add(name, clone, collection.core.exec.clock.as_ref())
}
pub fn stage_rename(collection: &Collection, draft: &mut Draft, name: &str) -> Result<(), EngineError> {
    draft.rename(name, collection.core.exec.clock.as_ref())
}

pub fn validate_preferences(change: &PreferencesChange) -> Vec<Issue> {
    let mut issues = Vec::new();
    for (field, value) in [
        (Field::Rollover, change.rollover),
        (Field::LearnAheadMinutes, change.learn_ahead_minutes),
    ] {
        if let Some(value) = value {
            issues.extend(options::validate::field(
                field,
                None,
                &Value::Number(value),
                None,
                "1970-01-01",
            ));
        }
    }
    issues
}
